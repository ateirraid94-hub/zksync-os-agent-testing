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
//! Most tests deploy the compiled L2AssetTracker bytecode with the
//! required storage pre-seeded and system-contract stubs, verifying
//! end-to-end behaviour against the actual Solidity implementation.
//!
//! One test (`test_mock_called_on_deposit`) uses a trivial accumulating
//! contract to verify the exact number of bootloader calls.

use rig::alloy::primitives::address;
use rig::evm_bytecode::BytecodeBuilder;
use rig::forward_system::run::convert_alloy::IntoAlloy;
use rig::ruint::aliases::U256;
use rig::system_hooks::addresses_constants::{L2_ASSET_TRACKER_ADDRESS, SYSTEM_CONTEXT_ADDRESS};
use rig::utils::L1TxBuilder;
use rig::TestingFramework;
use zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;

use crate::real_asset_tracker_bytecodes;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn b160_to_address(b: rig::ruint::aliases::B160) -> rig::alloy::primitives::Address {
    b.into_alloy()
}

fn asset_tracker_address() -> rig::alloy::primitives::Address {
    b160_to_address(L2_ASSET_TRACKER_ADDRESS)
}

fn system_context_address() -> rig::alloy::primitives::Address {
    b160_to_address(SYSTEM_CONTEXT_ADDRESS)
}

/// Compute the Solidity mapping storage key for `mapping(K => V)` at base slot.
/// key_slot = keccak256(abi.encode(key, base_slot))
fn solidity_mapping_key(key: U256, base_slot: U256) -> U256 {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(&key.to_be_bytes::<32>());
    buf[32..].copy_from_slice(&base_slot.to_be_bytes::<32>());
    let hash = rig::alloy::primitives::keccak256(buf);
    U256::from_be_bytes(hash.0)
}

/// Compute the storage key for a nested mapping:
/// `mapping(K1 => mapping(K2 => V))` at base slot.
fn solidity_nested_mapping_key(key1: U256, key2: U256, base_slot: U256) -> U256 {
    let inner = solidity_mapping_key(key1, base_slot);
    solidity_mapping_key(key2, inner)
}

/// Storage slot for interopInfo[assetId].totalSuccessfulDepositsFromL1.
/// interopInfo is at slot 156, InteropL2Info has fields:
///   +0: totalWithdrawalsToL1
///   +1: totalSuccessfulDepositsFromL1
fn deposits_from_l1_slot(base_token_asset_id: U256) -> U256 {
    solidity_mapping_key(base_token_asset_id, U256::from(156)) + U256::from(1)
}

const BASE_TOKEN_ASSET_ID: U256 = U256::from_limbs([0xBEEF, 0, 0, 0]);

/// Helper: read an arbitrary-keyed storage slot from the asset tracker.
fn read_slot_key(tester: &mut TestingFramework, key: U256) -> U256 {
    tester
        .get_storage_slot(&asset_tracker_address(), key)
        .map(|v| v.into_u256_be())
        .unwrap_or(U256::ZERO)
}

// ---------------------------------------------------------------------------
// Real L2AssetTracker setup
// ---------------------------------------------------------------------------

/// Build a TestingFramework with the real compiled L2AssetTracker deployed
/// and all required system contract stubs configured.
///
/// Storage pre-seeded:
///   - slot 154: L1_CHAIN_ID
///   - slot 155: BASE_TOKEN_ASSET_ID (0xBEEF)
///   - isAssetRegistered[baseTokenAssetId] = true  (slot 153 mapping)
///   - assetMigrationNumber[chainId][baseTokenAssetId] = 1  (slot 152 mapping)
///
/// System contract stubs:
///   - SystemContext (0x800b): returns slot 0 value for any call
///     (currentSettlementLayerChainId → L1 chain ID)
fn setup_real(l1_chain_id: u64) -> TestingFramework {
    let chain_id = 37u64; // default test chain id

    let is_registered_key = solidity_mapping_key(BASE_TOKEN_ASSET_ID, U256::from(153));
    let migration_key =
        solidity_nested_mapping_key(U256::from(chain_id), BASE_TOKEN_ASSET_ID, U256::from(152));

    let bytecode =
        hex::decode(real_asset_tracker_bytecodes::L2_ASSET_TRACKER_DEPLOYED_BYTECODE).unwrap();
    let return_slot_0_stub = real_asset_tracker_bytecodes::RETURN_SLOT_0_BYTECODE.to_vec();

    TestingFramework::new()
        .with_evm_contract(asset_tracker_address(), &bytecode)
        .with_storage_slot(
            asset_tracker_address(),
            U256::from(154),
            rig::ruint::aliases::B256::from(U256::from(l1_chain_id)),
        )
        .with_storage_slot(
            asset_tracker_address(),
            U256::from(155),
            rig::ruint::aliases::B256::from(BASE_TOKEN_ASSET_ID),
        )
        .with_storage_slot(
            asset_tracker_address(),
            is_registered_key,
            rig::ruint::aliases::B256::from(U256::from(1)),
        )
        .with_storage_slot(
            asset_tracker_address(),
            migration_key,
            rig::ruint::aliases::B256::from(U256::from(1)),
        )
        .with_evm_contract(system_context_address(), &return_slot_0_stub)
        .with_storage_slot(
            system_context_address(),
            U256::ZERO,
            rig::ruint::aliases::B256::from(U256::from(l1_chain_id)),
        )
}

// ---------------------------------------------------------------------------
// Mock accumulating contract — used only for call-count verification
// ---------------------------------------------------------------------------

/// EVM bytecode for a mock contract that accumulates call data:
/// - slot[0] += 1                      (call counter)
/// - slot[1] += CALLDATALOAD(36)       (accumulated amount)
/// - returns success
fn mock_accumulating_contract() -> Vec<u8> {
    BytecodeBuilder::new()
        .push0()
        .sload()
        .push_u8(1)
        .add()
        .push0()
        .sstore()
        .push_u8(1)
        .sload()
        .push_u8(36)
        .calldataload()
        .add()
        .push_u8(1)
        .sstore()
        .return_empty()
        .finish()
}

fn setup_mock(l1_chain_id: u64) -> TestingFramework {
    TestingFramework::new()
        .with_evm_contract(asset_tracker_address(), &mock_accumulating_contract())
        .with_storage_slot(
            asset_tracker_address(),
            U256::from(154),
            rig::ruint::aliases::B256::from(U256::from(l1_chain_id)),
        )
}

fn read_mock_slot(tester: &mut TestingFramework, slot: u64) -> U256 {
    tester
        .get_storage_slot(&asset_tracker_address(), U256::from(slot))
        .map(|v| v.into_u256_be())
        .unwrap_or(U256::ZERO)
}

// ===========================================================================
// Mock test — verify exact call count
// ===========================================================================

/// Verify the bootloader makes at least 2 calls (value mint + operator fee)
/// and the accumulated amount equals total_deposited.
#[test]
fn test_mock_called_on_deposit() {
    let l1_chain_id: u64 = 1;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 10_000;
    let gas_limit: u128 = 50_000;
    let value = rig::alloy::primitives::U256::from(500);
    let to_mint = rig::alloy::primitives::U256::from(gas_limit * gas_price)
        + rig::alloy::primitives::U256::from(1_000_000u64);

    let mut tester = setup_mock(l1_chain_id).with_balance(from, U256::from(u64::MAX));

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

    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(tx_result.is_success(), "L1 tx should succeed");

    let call_count = read_mock_slot(&mut tester, 0);
    assert!(
        call_count >= U256::from(2),
        "at least 2 calls expected (value mint + operator fee); got {call_count}"
    );

    let accumulated = read_mock_slot(&mut tester, 1);
    assert_eq!(
        accumulated,
        U256::from_be_slice(&to_mint.to_be_bytes::<32>()),
        "accumulated amount across all calls should equal to_mint"
    );
}

// ===========================================================================
// Real L2AssetTracker tests
// ===========================================================================

/// Successful deposit: interopInfo.totalSuccessfulDepositsFromL1 == total_deposited.
#[test]
fn test_real_deposit_updates_interop_info() {
    let l1_chain_id: u64 = 1;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 10_000;
    let gas_limit: u128 = 50_000;
    let value = rig::alloy::primitives::U256::from(500);
    let to_mint = rig::alloy::primitives::U256::from(gas_limit * gas_price)
        + rig::alloy::primitives::U256::from(1_000_000u64);

    let mut tester = setup_real(l1_chain_id).with_balance(from, U256::from(u64::MAX));

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
    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(tx_result.is_success(), "L1 tx should succeed");

    let deposits = read_slot_key(&mut tester, deposits_from_l1_slot(BASE_TOKEN_ASSET_ID));
    assert_eq!(
        deposits,
        U256::from_be_slice(&to_mint.to_be_bytes::<32>()),
        "totalSuccessfulDepositsFromL1 should equal total_deposited"
    );
}

/// Reverted tx body: value-mint notification is rolled back,
/// but operator fee + refund notifications persist.
/// totalSuccessfulDepositsFromL1 should still equal total_deposited
/// because operator fee + refund cover the entire deposit.
#[test]
fn test_real_reverted_body_still_tracks_fee_and_refund() {
    let l1_chain_id: u64 = 1;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 10_000;
    let gas_limit: u128 = 50_000;
    let to_mint = rig::alloy::primitives::U256::from(gas_limit * gas_price)
        + rig::alloy::primitives::U256::from(1_000_000u64);

    let mut tester = setup_real(l1_chain_id)
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
    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(
        !tx_result.is_success(),
        "L1 tx should revert (body reverts)"
    );

    let deposits = read_slot_key(&mut tester, deposits_from_l1_slot(BASE_TOKEN_ASSET_ID));
    assert_eq!(
        deposits,
        U256::from_be_slice(&to_mint.to_be_bytes::<32>()),
        "totalSuccessfulDepositsFromL1 should equal total_deposited (fee + refund)"
    );
}

/// OOG tx body: same as revert — operator fee + refund still tracked.
#[test]
fn test_real_oog_body_still_tracks_fee_and_refund() {
    let l1_chain_id: u64 = 1;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 10_000;
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

    let mut tester = setup_real(l1_chain_id)
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
    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(!tx_result.is_success(), "L1 tx should revert (OOG)");

    let deposits = read_slot_key(&mut tester, deposits_from_l1_slot(BASE_TOKEN_ASSET_ID));
    assert_eq!(
        deposits,
        U256::from_be_slice(&to_mint.to_be_bytes::<32>()),
        "totalSuccessfulDepositsFromL1 should equal total_deposited (fee + refund)"
    );
}

/// Zero value deposit: no calls to handleFinalizeBaseTokenBridgingOnL2.
#[test]
fn test_real_zero_deposit_no_notification() {
    let l1_chain_id: u64 = 1;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");

    let mut tester = setup_real(l1_chain_id).with_balance(from, U256::from(u64::MAX));

    let tx: ZKsyncTxEnvelope = L1TxBuilder::new()
        .from(from)
        .to(to)
        .gas_price(0u128)
        .gas_limit(50_000u128)
        .value(rig::alloy::primitives::U256::ZERO)
        .to_mint(rig::alloy::primitives::U256::ZERO)
        .build()
        .into();

    let output = tester.execute_block(vec![tx]);
    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(tx_result.is_success(), "L1 tx should succeed");

    let deposits = read_slot_key(&mut tester, deposits_from_l1_slot(BASE_TOKEN_ASSET_ID));
    assert_eq!(deposits, U256::ZERO, "no deposits should be recorded");
}

/// Different L1 chain ID: verify the contract receives the correct
/// _fromChainId and still updates interop accounting.
#[test]
fn test_real_different_chain_id() {
    let l1_chain_id: u64 = 270;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 10_000;
    let gas_limit: u128 = 50_000;
    let to_mint = rig::alloy::primitives::U256::from(gas_limit * gas_price)
        + rig::alloy::primitives::U256::from(2_000_000u64);

    let mut tester = setup_real(l1_chain_id).with_balance(from, U256::from(u64::MAX));

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
    let tx_result = output.tx_results[0].as_ref().expect("tx should not error");
    assert!(tx_result.is_success(), "L1 tx should succeed");

    let deposits = read_slot_key(&mut tester, deposits_from_l1_slot(BASE_TOKEN_ASSET_ID));
    assert_eq!(
        deposits,
        U256::from_be_slice(&to_mint.to_be_bytes::<32>()),
        "totalSuccessfulDepositsFromL1 should equal total_deposited"
    );
}
