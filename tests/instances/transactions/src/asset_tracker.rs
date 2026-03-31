//!
//! Tests for the L2AssetTracker.handleFinalizeBaseTokenBridgingOnL2 calls
//! that the bootloader makes during L1 transaction processing.
//!
//! When an L1 transaction deposits base tokens (total_deposited > 0), the
//! bootloader calls handleFinalizeBaseTokenBridgingOnL2(uint256, uint256)
//! on the L2AssetTracker contract up to three times — once for the value
//! mint, once for the operator fee, and once for the refund. If any of
//! these amounts is zero the corresponding call is skipped.
//!
//! The mock contract at L2_ASSET_TRACKER_ADDRESS accumulates call data:
//! - slot 0: call count (incremented each invocation)
//! - slot 1: accumulated sum of the `_amount` argument across all calls
//! - slot 2: last `_fromChainId` argument
//! - slot 3: last `_amount` argument

use rig::alloy::primitives::address;
use rig::evm_bytecode::BytecodeBuilder;
use rig::forward_system::run::convert_alloy::IntoAlloy;
use rig::ruint::aliases::U256;
use rig::system_hooks::addresses_constants::{L2_ASSET_TRACKER_ADDRESS, SYSTEM_CONTEXT_ADDRESS};
use rig::utils::L1TxBuilder;
use rig::TestingFramework;
use zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;

fn b160_to_address(b: rig::ruint::aliases::B160) -> rig::alloy::primitives::Address {
    b.into_alloy()
}

fn asset_tracker_address() -> rig::alloy::primitives::Address {
    b160_to_address(L2_ASSET_TRACKER_ADDRESS)
}

fn system_context_address() -> rig::alloy::primitives::Address {
    b160_to_address(SYSTEM_CONTEXT_ADDRESS)
}

/// EVM bytecode for a mock contract that accumulates call data:
/// - slot[0] += 1                      (call counter)
/// - slot[1] += CALLDATALOAD(36)       (accumulated amount)
/// - slot[2] = CALLDATALOAD(4)         (last chain ID)
/// - slot[3] = CALLDATALOAD(36)        (last amount)
/// - returns success
fn mock_accumulating_contract() -> Vec<u8> {
    use rig::evm_bytecode::BytecodeBuilder;

    BytecodeBuilder::new()
        // slot[0] += 1
        .push0()
        .sload()
        .push_u8(1)
        .add()
        .push0()
        .sstore()
        // slot[1] += CALLDATALOAD(36)
        .push_u8(1)
        .sload()
        .push_u8(36)
        .calldataload()
        .add()
        .push_u8(1)
        .sstore()
        // slot[2] = CALLDATALOAD(4)
        .push_u8(4)
        .calldataload()
        .push_u8(2)
        .sstore()
        // slot[3] = CALLDATALOAD(36)
        .push_u8(36)
        .calldataload()
        .push_u8(3)
        .sstore()
        // return success
        .return_empty()
        .finish()
}

/// Build a TestingFramework with:
/// - A mock accumulating contract at L2_ASSET_TRACKER_ADDRESS
/// - SystemContext slot 0 set to the given chain ID
fn setup_with_chain_id(sl_chain_id: u64) -> TestingFramework {
    TestingFramework::new()
        .with_evm_contract(asset_tracker_address(), &mock_accumulating_contract())
        .with_storage_slot(
            system_context_address(),
            U256::ZERO,
            rig::ruint::aliases::B256::from(U256::from(sl_chain_id)),
        )
}

/// Helper: read a storage slot from the asset tracker, returning U256.
fn read_slot(tester: &mut TestingFramework, slot: u64) -> Option<U256> {
    tester
        .get_storage_slot(&asset_tracker_address(), U256::from(slot))
        .map(|v| v.into_u256_be())
}

fn assert_reverted_deposit_asset_tracker(
    tester: &mut TestingFramework,
    output: &rig::zksync_os_interface::types::BlockOutput,
    expected_total_deposited: rig::alloy::primitives::U256,
) {
    assert_eq!(output.tx_results.len(), 1);
    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(
        !tx_result.is_success(),
        "L1 tx should revert during execution, not be rejected"
    );

    let call_count = read_slot(tester, 0).unwrap_or(U256::ZERO);
    assert_eq!(
        call_count,
        U256::from(2u64),
        "only operator fee and refund should reach the asset tracker after execution revert"
    );

    let accumulated = read_slot(tester, 1).unwrap_or(U256::ZERO);
    assert_eq!(
        accumulated,
        U256::from_be_slice(&expected_total_deposited.to_be_bytes::<32>()),
        "accumulated amount should equal total_deposited after execution revert"
    );
}

/// Verify that when an L1 tx has a deposit (total_deposited > 0), the bootloader
/// calls handleFinalizeBaseTokenBridgingOnL2 on L2AssetTracker multiple times,
/// and the accumulated amount equals total_deposited.
#[test]
fn test_asset_tracker_called_on_deposit() {
    let sl_chain_id: u64 = 1;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 10_000;
    let gas_limit: u128 = 50_000;
    let value = rig::alloy::primitives::U256::from(500);
    let to_mint = rig::alloy::primitives::U256::from(gas_limit * gas_price)
        + rig::alloy::primitives::U256::from(1_000_000u64);

    let mut tester = setup_with_chain_id(sl_chain_id).with_balance(from, U256::from(u64::MAX));

    let tx: ZKsyncTxEnvelope = L1TxBuilder::new()
        .from(from)
        .to(to)
        .gas_price(gas_price)
        .gas_limit(gas_limit)
        .value(value)
        .to_mint(to_mint)
        .build()
        .into();

    let output = tester.execute_block(vec![tx]);

    assert_eq!(output.tx_results.len(), 1);
    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(tx_result.is_success(), "L1 tx should succeed");

    // The bootloader makes up to 3 calls (value mint, operator fee, refund).
    // The mock accumulates all amounts. With nonzero gas_price and value,
    // all three portions should be nonzero.
    let call_count = read_slot(&mut tester, 0).unwrap_or(U256::ZERO);
    assert!(
        call_count >= U256::from(2),
        "at least 2 calls expected (value mint + operator fee); got {call_count}"
    );

    // The accumulated amount (slot 1) must equal total_deposited.
    let accumulated = read_slot(&mut tester, 1).unwrap_or(U256::ZERO);
    assert_eq!(
        accumulated,
        U256::from_be_slice(&to_mint.to_be_bytes::<32>()),
        "accumulated amount across all calls should equal to_mint"
    );

    // Chain ID should be correct in every call.
    let chain_id = read_slot(&mut tester, 2).unwrap_or(U256::ZERO);
    assert_eq!(
        chain_id,
        U256::from(sl_chain_id),
        "chain ID argument should match settlement layer"
    );

    // computational_native_used reflects the main tx body computation
    // plus intrinsic native. Post-execution operations (asset tracker
    // notifications, coinbase transfer, refund) run on FORMAL_INFINITE
    // and their cost is covered by L1_TX_INTRINSIC_NATIVE_COST, not
    // measured at runtime.
    assert!(
        tx_result.computational_native_used > 0,
        "computational_native_used should be nonzero"
    );
}

/// Verify that no call to L2AssetTracker is made when total_deposited == 0.
#[test]
fn test_asset_tracker_not_called_without_deposit() {
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 0;
    let gas_limit: u128 = 50_000;
    let to_mint = rig::alloy::primitives::U256::ZERO;

    let mut tester = setup_with_chain_id(1).with_balance(from, U256::from(u64::MAX));

    let tx: ZKsyncTxEnvelope = L1TxBuilder::new()
        .from(from)
        .to(to)
        .gas_price(gas_price)
        .gas_limit(gas_limit)
        .value(rig::alloy::primitives::U256::ZERO)
        .to_mint(to_mint)
        .build()
        .into();

    let output = tester.execute_block(vec![tx]);

    assert_eq!(output.tx_results.len(), 1);
    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(tx_result.is_success(), "L1 tx should succeed");

    // Slot 0 (call count) should be 0 — no calls made.
    let call_count = read_slot(&mut tester, 0);
    assert!(
        call_count.is_none() || call_count.unwrap() == U256::ZERO,
        "asset tracker should NOT be called when total_deposited == 0"
    );
}

/// Verify the call works with a different settlement layer chain ID
/// and that accumulated amounts are correct.
#[test]
fn test_asset_tracker_uses_correct_chain_id() {
    let sl_chain_id: u64 = 270;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 10_000;
    let gas_limit: u128 = 50_000;
    let to_mint = rig::alloy::primitives::U256::from(gas_limit * gas_price)
        + rig::alloy::primitives::U256::from(2_000_000u64);

    let mut tester = setup_with_chain_id(sl_chain_id).with_balance(from, U256::from(u64::MAX));

    let tx: ZKsyncTxEnvelope = L1TxBuilder::new()
        .from(from)
        .to(to)
        .gas_price(gas_price)
        .gas_limit(gas_limit)
        .value(rig::alloy::primitives::U256::from(100))
        .to_mint(to_mint)
        .build()
        .into();

    let output = tester.execute_block(vec![tx]);

    assert_eq!(output.tx_results.len(), 1);
    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(tx_result.is_success(), "L1 tx should succeed");

    // Verify chain ID argument
    let chain_id = read_slot(&mut tester, 2).unwrap_or(U256::ZERO);
    assert_eq!(
        chain_id,
        U256::from(sl_chain_id),
        "chain ID argument should be 270"
    );

    // Verify accumulated deposit amount equals to_mint
    let accumulated = read_slot(&mut tester, 1).unwrap_or(U256::ZERO);
    assert_eq!(
        accumulated,
        U256::from_be_slice(&to_mint.to_be_bytes::<32>()),
        "accumulated deposit amount should match to_mint"
    );
}

#[test]
fn test_asset_tracker_reverted_body_skips_value_mint_notification() {
    let sl_chain_id: u64 = 1;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 1000;
    let gas_limit: u128 = 50_000;
    let to_mint = rig::alloy::primitives::U256::from(gas_limit * gas_price)
        + rig::alloy::primitives::U256::from(1_000_000u64);

    let mut tester = setup_with_chain_id(sl_chain_id)
        .with_evm_contract(to, &rig::evm_bytecode::revert())
        .with_balance(from, U256::from(u64::MAX));

    let tx: ZKsyncTxEnvelope = L1TxBuilder::new()
        .from(from)
        .to(to)
        .gas_price(gas_price)
        .gas_limit(gas_limit)
        .value(rig::alloy::primitives::U256::from(500))
        .to_mint(to_mint)
        .build()
        .into();

    let output = tester.execute_block(vec![tx]);
    assert_reverted_deposit_asset_tracker(&mut tester, &output, to_mint);
}

#[test]
fn test_asset_tracker_oog_body_still_notifies_fee_and_refund() {
    let sl_chain_id: u64 = 1;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 1000;
    let gas_limit: u128 = 50_000;
    let to_mint = rig::alloy::primitives::U256::from(gas_limit * gas_price)
        + rig::alloy::primitives::U256::from(1_000_000u64);

    let expensive_bytecode = BytecodeBuilder::new()
        .push_u8(1)
        .push0()
        .sstore()
        .push_u8(2)
        .push_u8(1)
        .sstore()
        .push_u8(3)
        .push_u8(2)
        .sstore()
        .return_empty()
        .finish();

    let mut tester = setup_with_chain_id(sl_chain_id)
        .with_evm_contract(to, &expensive_bytecode)
        .with_balance(from, U256::from(u64::MAX));

    let tx: ZKsyncTxEnvelope = L1TxBuilder::new()
        .from(from)
        .to(to)
        .gas_price(gas_price)
        .gas_limit(gas_limit)
        .value(rig::alloy::primitives::U256::from(500))
        .to_mint(to_mint)
        .build()
        .into();

    let output = tester.execute_block(vec![tx]);
    assert_reverted_deposit_asset_tracker(&mut tester, &output, to_mint);
}
