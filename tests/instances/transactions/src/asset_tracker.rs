//!
//! Tests for the L2AssetTracker.handleFinalizeBaseTokenBridgingOnL2 call
//! that the bootloader makes during L1 transaction processing.
//!
//! When an L1 transaction deposits base tokens (total_deposited > 0), the
//! bootloader calls handleFinalizeBaseTokenBridgingOnL2(uint256, uint256)
//! on the L2AssetTracker contract before executing the transaction body.
//! These tests verify the call is made with the correct arguments.
//!
//! The mock contract at L2_ASSET_TRACKER_ADDRESS records calldata in storage:
//! - slot 0: CALLDATASIZE
//! - slot 1: calldata[0..32] (selector + first 28 bytes of arg1)
//! - slot 2: calldata[4..36] (first ABI argument: _fromChainId)
//! - slot 3: calldata[36..68] (second ABI argument: _amount)

use rig::alloy::primitives::address;
use rig::forward_system::run::convert_alloy::IntoAlloy;
use rig::ruint::aliases::U256;
use rig::system_hooks::addresses_constants::{L2_ASSET_TRACKER_ADDRESS, SYSTEM_CONTEXT_ADDRESS};
use rig::utils::L1TxBuilder;
use rig::TestingFramework;
use zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;

/// handleFinalizeBaseTokenBridgingOnL2(uint256,uint256) selector
const HANDLE_FINALIZE_SELECTOR: [u8; 4] = [0x03, 0x11, 0x7c, 0x8c];

fn b160_to_address(b: rig::ruint::aliases::B160) -> rig::alloy::primitives::Address {
    b.into_alloy()
}

fn asset_tracker_address() -> rig::alloy::primitives::Address {
    b160_to_address(L2_ASSET_TRACKER_ADDRESS)
}

fn system_context_address() -> rig::alloy::primitives::Address {
    b160_to_address(SYSTEM_CONTEXT_ADDRESS)
}

/// EVM bytecode for a mock contract that records its calldata in storage:
/// - slot 0: CALLDATASIZE
/// - slot 1: CALLDATALOAD(0)  — first 32 bytes (selector + part of arg1)
/// - slot 2: CALLDATALOAD(4)  — first ABI argument (uint256 _fromChainId)
/// - slot 3: CALLDATALOAD(36) — second ABI argument (uint256 _amount)
/// Then returns success.
fn mock_recording_contract() -> Vec<u8> {
    vec![
        // CALLDATASIZE, PUSH1 0, SSTORE
        0x36, 0x60, 0x00, 0x55, // PUSH1 0, CALLDATALOAD, PUSH1 1, SSTORE
        0x60, 0x00, 0x35, 0x60, 0x01, 0x55, // PUSH1 4, CALLDATALOAD, PUSH1 2, SSTORE
        0x60, 0x04, 0x35, 0x60, 0x02, 0x55, // PUSH1 36, CALLDATALOAD, PUSH1 3, SSTORE
        0x60, 0x24, 0x35, 0x60, 0x03, 0x55, // PUSH0, PUSH0, RETURN
        0x5f, 0x5f, 0xf3,
    ]
}

/// Build a TestingFramework with:
/// - A mock recording contract at L2_ASSET_TRACKER_ADDRESS
/// - SystemContext slot 0 set to the given chain ID
fn setup_with_chain_id(sl_chain_id: u64) -> TestingFramework {
    TestingFramework::new()
        .without_revm_consistency_check()
        .with_evm_contract(asset_tracker_address(), &mock_recording_contract())
        .with_storage_slot(
            system_context_address(),
            U256::ZERO,
            rig::ruint::aliases::B256::from(U256::from(sl_chain_id)),
        )
}

/// Verify that when an L1 tx has a deposit (total_deposited > 0), the bootloader
/// calls handleFinalizeBaseTokenBridgingOnL2 on L2AssetTracker with the correct
/// selector, settlement layer chain ID, and deposit amount.
#[test]
fn test_asset_tracker_called_on_deposit() {
    let sl_chain_id: u64 = 1;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 1000;
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

    // Slot 0: calldatasize = 68 (4 selector + 32 arg1 + 32 arg2)
    let calldatasize = tester
        .get_storage_slot(&asset_tracker_address(), U256::from(0))
        .expect("slot 0 should be written");
    assert_eq!(
        calldatasize.into_u256_be(),
        U256::from(68),
        "calldata should be 68 bytes (4 + 32 + 32)"
    );

    // Slot 1: first 32 bytes = selector in the leading 4 bytes
    let first_word = tester
        .get_storage_slot(&asset_tracker_address(), U256::from(1))
        .expect("slot 1 should be written");
    let first_word_bytes = first_word.as_u8_array();
    assert_eq!(
        &first_word_bytes[0..4],
        &HANDLE_FINALIZE_SELECTOR,
        "selector should be handleFinalizeBaseTokenBridgingOnL2"
    );

    // Slot 2: first ABI argument = settlement layer chain ID
    let arg1 = tester
        .get_storage_slot(&asset_tracker_address(), U256::from(2))
        .expect("slot 2 should be written");
    assert_eq!(
        arg1.into_u256_be(),
        U256::from(sl_chain_id),
        "first arg should be settlement layer chain ID"
    );

    // Slot 3: second ABI argument = total_deposited (= to_mint)
    let arg2 = tester
        .get_storage_slot(&asset_tracker_address(), U256::from(3))
        .expect("slot 3 should be written");
    assert_eq!(
        arg2.into_u256_be(),
        U256::from_be_slice(&to_mint.to_be_bytes::<32>()),
        "second arg should be the deposited amount (to_mint)"
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

    // Slot 0 should not be written (no call made)
    let calldatasize = tester.get_storage_slot(&asset_tracker_address(), U256::from(0));
    assert!(
        calldatasize.is_none() || calldatasize.unwrap().into_u256_be() == U256::ZERO,
        "asset tracker should NOT be called when total_deposited == 0"
    );
}

/// Verify the call works with a different settlement layer chain ID.
#[test]
fn test_asset_tracker_uses_correct_chain_id() {
    let sl_chain_id: u64 = 270;
    let from = address!("1234000000000000000000000000000000000000");
    let to = address!("abcd000000000000000000000000000000000000");
    let gas_price: u128 = 1000;
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
    let arg1 = tester
        .get_storage_slot(&asset_tracker_address(), U256::from(2))
        .expect("slot 2 should be written");
    assert_eq!(
        arg1.into_u256_be(),
        U256::from(sl_chain_id),
        "chain ID argument should be 270"
    );

    // Verify deposit amount
    let arg2 = tester
        .get_storage_slot(&asset_tracker_address(), U256::from(3))
        .expect("slot 3 should be written");
    assert_eq!(
        arg2.into_u256_be(),
        U256::from_be_slice(&to_mint.to_be_bytes::<32>()),
        "deposit amount should match to_mint"
    );
}
