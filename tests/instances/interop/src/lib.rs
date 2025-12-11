//!
//! These tests are focused on interop support in ZKsync OS
//!
#![cfg(test)]

mod bytecodes;

use alloy::signers::local::PrivateKeySigner;
use bytecodes::*;
use rig::alloy::consensus::TxLegacy;
use rig::alloy::primitives::address;
use rig::chain::BlockToRun;
use rig::crypto::MiniDigest;
use rig::ruint::aliases::{B160, U256};
use rig::system_hooks::addresses_constants::{
    L2_INTEROP_ROOT_STORAGE_ADDRESS, SYSTEM_CONTEXT_ADDRESS,
};
use rig::utils::{
    encode_interop_root_import_calldata, encode_set_settlement_layer_chain_id_calldata,
};
use rig::zk_ee::common_structs::interop_root_storage::InteropRoot as StoredInteropRoot;
use rig::zk_ee::system::tracer::NopTracer;
use rig::zk_ee::utils::Bytes32;
use rig::zksync_os_interface::types::ExecutionOutput;
use rig::zksync_os_interface::types::ExecutionResult;
use rig::{alloy, zksync_web3_rs, BlockContext, Chain};
use std::str::FromStr;
use std::vec;
use zksync_web3_rs::signers::{LocalWallet, Signer};

#[test]
fn run_processes_one_interop_root() {
    // Create some dummy interop root
    let interop_roots = vec![StoredInteropRoot {
        root: Bytes32::from_u256_be(&U256::ONE),
        block_or_batch_number: U256::from(42),
        chain_id: U256::ONE,
    }];

    run_test(interop_roots);
}

#[test]
#[should_panic(expected = "Transaction should be successful")]
fn run_fails_if_interop_root_is_incorrect() {
    // Create some dummy interop root
    let interop_roots = vec![StoredInteropRoot {
        root: Bytes32::zero(), // Root can't be zero
        block_or_batch_number: U256::from(42),
        chain_id: U256::ONE,
    }];

    run_test(interop_roots);
}

#[test]
fn run_processes_several_interop_roots() {
    let mut interop_roots = Vec::new();
    // Create several interop roots to test batch processing and resource costs
    for i in 1..=20 {
        interop_roots.push(StoredInteropRoot {
            root: Bytes32::from_u256_be(&U256::from(0x1000 + i)),
            block_or_batch_number: U256::from(100 + i),
            chain_id: U256::from(i), // Use different chain IDs
        });
    }

    run_test(interop_roots);
}

#[test]
fn run_processes_empty_interop_roots() {
    run_test(vec![]);
}

#[test]
fn run_processes_interop_roots_max_amount() {
    let interop_roots = vec![
        // Edge case: Maximum values
        StoredInteropRoot {
            root: Bytes32::from_u256_be(&U256::MAX),
            block_or_batch_number: U256::MAX,
            chain_id: U256::MAX,
        },
        // Edge case: Minimum valid values (chain_id = 1, block = 0)
        StoredInteropRoot {
            root: Bytes32::from_u256_be(&U256::from(1)),
            block_or_batch_number: U256::ZERO,
            chain_id: U256::ONE,
        }, // Edge case: Large root hash with small numbers
        StoredInteropRoot {
            root: Bytes32::from_u256_be(&(U256::MAX - U256::from(1))),
            block_or_batch_number: U256::ONE,
            chain_id: U256::ONE,
        },
    ];

    run_test(interop_roots);
}

fn run_test(interop_roots: Vec<StoredInteropRoot>) {
    run_interop_roots_test_inner(interop_roots)
}

/// Executes a transaction with specified interop roots and verifies success
fn run_interop_roots_test_inner(interop_roots: Vec<StoredInteropRoot>) {
    let mut chain = Chain::empty(None);
    // We can't set interop roots for block 0
    chain.set_last_block_number(0);
    let wallet = PrivateKeySigner::from_str(
        "dcf2cbdd171a21c480aa7f53d77f31bb102282b3ff099c78e3118b37348c72f7",
    )
    .unwrap();
    let wallet_ethers = LocalWallet::from_bytes(wallet.to_bytes().as_slice()).unwrap();

    let from = wallet_ethers.address();

    chain.set_balance(
        B160::from_be_bytes(from.0),
        U256::from(1_000_000_000_000_000_u64),
    );

    let bytecode = hex::decode(L2_INTEROP_ROOT_STORAGE_BYTECODE).unwrap();
    chain.set_evm_bytecode(L2_INTEROP_ROOT_STORAGE_ADDRESS, &bytecode);

    // Compute expected rolling hash
    let expected_rolling_hash = rig::basic_system::system_implementation::system::interop_roots::calculate_interop_roots_rolling_hash(
        Bytes32::ZERO,
        interop_roots.iter(),
        &mut rig::crypto::sha3::Keccak256::new(),
    );

    // Construct calldata
    let n_interop_roots = interop_roots.len();
    let calldata = encode_interop_root_import_calldata(interop_roots);

    let tx = rig::utils::encode_service_tx(
        0,
        50_000_000,
        &L2_INTEROP_ROOT_STORAGE_ADDRESS.to_be_bytes::<20>(),
        &calldata,
    );

    let mut tracer = NopTracer::default();

    let block_context = BlockContext {
        eip1559_basefee: U256::ZERO,
        ..Default::default()
    };

    let (output, pi_batch_output) = chain
        .run_block_pi_output(vec![tx], Some(block_context), None, None, &mut tracer)
        .expect("Block should run successfully");

    // Verify the transaction succeeded
    assert_eq!(output.tx_results.len(), 1);
    assert!(output.tx_results[0].is_ok(), "Transaction should succeed");
    let tx_result = output.tx_results[0].as_ref().unwrap();
    assert!(tx_result.is_success(), "Transaction should be successful");
    // check the output to ensure the contract was called
    match &tx_result.execution_result {
        ExecutionResult::Success(ExecutionOutput::Call(_)) => (),
        _ => panic!("Execution result must be a successful call"),
    }
    // Check there's an event for every root
    assert!(tx_result.logs.len() == n_interop_roots);
    // Check the rolling hash in public input is the expected one
    assert_eq!(
        expected_rolling_hash, pi_batch_output.interop_roots_rolling_hash,
        "Mismatch in interop_roots_rolling_hash"
    );
}

const L2_CHAIN_ASSET_HANDLER_ADDRESS: B160 = B160::from_limbs([0x1000a, 0, 0]);

#[test]
fn test_new_sl_chain_id_no_update() {
    let mut chain = Chain::empty(None);

    let bytecode = hex::decode(SYSTEM_CONTEXT_BYTECODE).unwrap();
    chain.set_evm_bytecode(SYSTEM_CONTEXT_ADDRESS, &bytecode);
    let bytecode = hex::decode(L2_CHAIN_ASSET_HANDLER_BYTECODE).unwrap();
    chain.set_evm_bytecode(L2_CHAIN_ASSET_HANDLER_ADDRESS, &bytecode);
    let mut tracer = NopTracer::default();

    // We check that a block with no updates to sl chain id
    // has a 0 on the PI field
    let wallet = PrivateKeySigner::from_str(
        "dcf2cbdd171a21c480aa7f53d77f31bb102282b3ff099c78e3118b37348c72f7",
    )
    .unwrap();
    let wallet_ethers = LocalWallet::from_bytes(wallet.to_bytes().as_slice()).unwrap();
    let from = wallet_ethers.address();
    chain.set_balance(
        B160::from_be_bytes(from.0),
        U256::from(1_000_000_000_000_000_u64),
    );
    let to = address!("4200000000000000000000000000000000000000");
    // Simple tx to populate the block
    let tx = {
        let mint_tx = TxLegacy {
            chain_id: 37u64.into(),
            nonce: 0,
            gas_price: 1000,
            gas_limit: 21_000,
            to: alloy::primitives::TxKind::Call(to),
            value: Default::default(),
            input: Default::default(),
        };
        rig::utils::sign_and_encode_alloy_tx(mint_tx, &wallet)
    };
    let (output, pi_batch_output) = chain
        .run_block_pi_output(vec![tx], None, None, None, &mut tracer)
        .expect("Block should run successfully");
    // Verify the transaction succeeded
    assert_eq!(output.tx_results.len(), 1);
    assert!(output.tx_results[0].is_ok(), "Transaction should succeed");
    let tx_result = output.tx_results[0].as_ref().unwrap();
    assert!(tx_result.is_success(), "Transaction should be successful");
    // Check the pi
    assert_eq!(
        U256::ZERO,
        pi_batch_output.new_settlement_layer_chain_id,
        "Mismatch in new_settlement_layer_chain_id"
    );
}

#[test]
fn test_new_sl_chain_id_one_update() {
    let mut chain = Chain::empty(None);

    let bytecode = hex::decode(SYSTEM_CONTEXT_BYTECODE).unwrap();
    chain.set_evm_bytecode(SYSTEM_CONTEXT_ADDRESS, &bytecode);
    let bytecode = hex::decode(L2_CHAIN_ASSET_HANDLER_BYTECODE).unwrap();
    chain.set_evm_bytecode(L2_CHAIN_ASSET_HANDLER_ADDRESS, &bytecode);
    let mut tracer = NopTracer::default();

    // We check that sending a single update works
    let new_sl_chain_id = U256::from(42);

    // Construct calldata
    let calldata = encode_set_settlement_layer_chain_id_calldata(new_sl_chain_id);
    let tx = rig::utils::encode_service_tx(
        0,
        50_000_000,
        &SYSTEM_CONTEXT_ADDRESS.to_be_bytes::<20>(),
        &calldata,
    );
    let block_context = BlockContext {
        eip1559_basefee: U256::ZERO,
        ..Default::default()
    };
    let (output, pi_batch_output) = chain
        .run_block_pi_output(vec![tx], Some(block_context), None, None, &mut tracer)
        .expect("Block should run successfully");

    // Verify the transaction succeeded
    assert_eq!(output.tx_results.len(), 1);
    assert!(output.tx_results[0].is_ok(), "Transaction should succeed");
    let tx_result = output.tx_results[0].as_ref().unwrap();
    assert!(tx_result.is_success(), "Transaction should be successful");
    // check the output to ensure the contract was called
    match &tx_result.execution_result {
        ExecutionResult::Success(ExecutionOutput::Call(_)) => (),
        _ => panic!("Execution result must be a successful call"),
    }
    // Check there's an event
    assert!(tx_result.logs.len() == 1);
    // Check the pi
    assert_eq!(
        new_sl_chain_id, pi_batch_output.new_settlement_layer_chain_id,
        "Mismatch in new_settlement_layer_chain_id"
    );
}

#[test]
fn test_new_sl_chain_id_two_updates_fail() {
    let mut chain = Chain::empty(None);

    let bytecode = hex::decode(SYSTEM_CONTEXT_BYTECODE).unwrap();
    chain.set_evm_bytecode(SYSTEM_CONTEXT_ADDRESS, &bytecode);
    let bytecode = hex::decode(L2_CHAIN_ASSET_HANDLER_BYTECODE).unwrap();
    chain.set_evm_bytecode(L2_CHAIN_ASSET_HANDLER_ADDRESS, &bytecode);
    let mut tracer = NopTracer::default();

    // We check that attempting two updates in a block fails
    let calldata1 = encode_set_settlement_layer_chain_id_calldata(U256::from(43));
    let tx1 = rig::utils::encode_service_tx(
        0,
        50_000_000,
        &SYSTEM_CONTEXT_ADDRESS.to_be_bytes::<20>(),
        &calldata1,
    );
    let calldata2 = encode_set_settlement_layer_chain_id_calldata(U256::from(44));
    let tx2 = rig::utils::encode_service_tx(
        1,
        50_000_000,
        &SYSTEM_CONTEXT_ADDRESS.to_be_bytes::<20>(),
        &calldata2,
    );
    let block_context = BlockContext {
        eip1559_basefee: U256::ZERO,
        ..Default::default()
    };
    chain
        .run_block_pi_output(vec![tx1, tx2], Some(block_context), None, None, &mut tracer)
        .expect_err("Block with two updates to sl chain id should fail");
}

#[test]
fn test_set_sl_chain_id_first_block_batch() {
    let mut chain = Chain::empty(None);

    let bytecode = hex::decode(SYSTEM_CONTEXT_BYTECODE).unwrap();
    chain.set_evm_bytecode(SYSTEM_CONTEXT_ADDRESS, &bytecode);
    let bytecode = hex::decode(L2_CHAIN_ASSET_HANDLER_BYTECODE).unwrap();
    chain.set_evm_bytecode(L2_CHAIN_ASSET_HANDLER_ADDRESS, &bytecode);

    let wallet = PrivateKeySigner::from_str(
        "dcf2cbdd171a21c480aa7f53d77f31bb102282b3ff099c78e3118b37348c72f7",
    )
    .unwrap();
    let wallet_ethers = LocalWallet::from_bytes(wallet.to_bytes().as_slice()).unwrap();

    let from = wallet_ethers.address();
    let to = address!("0000000000000000000000000000000000010002");

    chain.set_balance(
        B160::from_be_bytes(from.0),
        U256::from(1_000_000_000_000_000_u64),
    );

    let calldata = encode_set_settlement_layer_chain_id_calldata(U256::from(42));
    let tx = rig::utils::encode_service_tx(
        0,
        50_000_000,
        &SYSTEM_CONTEXT_ADDRESS.to_be_bytes::<20>(),
        &calldata,
    );

    let block1 = BlockToRun {
        transactions: vec![tx],
        block_context: Some(BlockContext {
            eip1559_basefee: U256::ZERO,
            ..Default::default()
        }),
    };

    let tx = {
        let mint_tx = TxLegacy {
            chain_id: 37u64.into(),
            nonce: 0,
            gas_price: 1000,
            gas_limit: 21_000,
            to: alloy::primitives::TxKind::Call(to),
            value: Default::default(),
            input: Default::default(),
        };
        rig::utils::sign_and_encode_alloy_tx(mint_tx, &wallet)
    };
    let block2 = BlockToRun {
        transactions: vec![tx],
        block_context: None,
    };

    chain.run_multiblock_batch_proof_run_on_two_blocks(
        block1,
        block2,
        rig::zk_ee::common_structs::DACommitmentScheme::BlobsAndPubdataKeccak256,
    )
}

#[test]
#[should_panic]
fn test_set_sl_chain_id_not_first_block_batch_fails() {
    let mut chain = Chain::empty(None);

    let bytecode = hex::decode(SYSTEM_CONTEXT_BYTECODE).unwrap();
    chain.set_evm_bytecode(SYSTEM_CONTEXT_ADDRESS, &bytecode);
    let bytecode = hex::decode(L2_CHAIN_ASSET_HANDLER_BYTECODE).unwrap();
    chain.set_evm_bytecode(L2_CHAIN_ASSET_HANDLER_ADDRESS, &bytecode);

    let wallet = PrivateKeySigner::from_str(
        "dcf2cbdd171a21c480aa7f53d77f31bb102282b3ff099c78e3118b37348c72f7",
    )
    .unwrap();
    let wallet_ethers = LocalWallet::from_bytes(wallet.to_bytes().as_slice()).unwrap();

    let from = wallet_ethers.address();
    let to = address!("0000000000000000000000000000000000010002");

    chain.set_balance(
        B160::from_be_bytes(from.0),
        U256::from(1_000_000_000_000_000_u64),
    );

    let tx = {
        let mint_tx = TxLegacy {
            chain_id: 37u64.into(),
            nonce: 0,
            gas_price: 1000,
            gas_limit: 21_000,
            to: alloy::primitives::TxKind::Call(to),
            value: Default::default(),
            input: Default::default(),
        };
        rig::utils::sign_and_encode_alloy_tx(mint_tx, &wallet)
    };
    let block1 = BlockToRun {
        transactions: vec![tx],
        block_context: None,
    };

    let calldata = encode_set_settlement_layer_chain_id_calldata(U256::from(42));
    let tx = rig::utils::encode_service_tx(
        0,
        50_000_000,
        &SYSTEM_CONTEXT_ADDRESS.to_be_bytes::<20>(),
        &calldata,
    );

    let block2 = BlockToRun {
        transactions: vec![tx],
        block_context: Some(BlockContext {
            eip1559_basefee: U256::ZERO,
            ..Default::default()
        }),
    };

    chain.run_multiblock_batch_proof_run_on_two_blocks(
        block1,
        block2,
        rig::zk_ee::common_structs::DACommitmentScheme::BlobsAndPubdataKeccak256,
    )
}
