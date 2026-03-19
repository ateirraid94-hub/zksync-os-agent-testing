//!
//! These tests are focused on multiblock batch proving inputs.
//!
#![cfg(test)]

use rig::alloy::consensus::{TxEip1559, TxLegacy};
use rig::alloy::primitives::address;
use rig::alloy::primitives::TxKind;
use rig::chain::RunConfig;
use rig::forward_system::run::{
    generate_batch_proof_input, generate_legacy_batch_proof_input, NativeBatchBlockInput,
};
use rig::log::debug;
use rig::ruint::aliases::U256;
use rig::utils::{ERC_20_BYTECODE, ERC_20_MINT_CALLDATA, ERC_20_TRANSFER_CALLDATA};
use rig::zk_ee::common_structs::DACommitmentScheme;
use rig::zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;
use rig::{testing_signer, BlockContext, TestingFramework};
use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
use std::path::PathBuf;

const TEST_STACK_SIZE: usize = 64 << 20;

fn first_mismatch<T: PartialEq>(lhs: &[T], rhs: &[T]) -> Option<usize> {
    lhs.iter()
        .zip(rhs.iter())
        .position(|(lhs_item, rhs_item)| lhs_item != rhs_item)
        .or_else(|| (lhs.len() != rhs.len()).then_some(lhs.len().min(rhs.len())))
}

fn legacy_singleblock_run_config() -> RunConfig {
    let mut config = RunConfig::with_riscv_run();
    config.app = Some("singleblock_batch".to_string());
    config.check_storage_diff_hashes = false;
    config
}

fn new_multiblock_batch_tester() -> TestingFramework {
    let wallet = testing_signer(0);
    let to = address!("0000000000000000000000000000000000010002");
    let bytecode = hex::decode(ERC_20_BYTECODE).unwrap();

    TestingFramework::new()
        .with_evm_contract(to, &bytecode)
        .with_balance(wallet.address(), U256::from(1_000_000_000_000_000_u64))
        .with_minted_tokens_to_treasury()
}

fn run_multiblock_batch_proof_run(da_commitment_scheme: DACommitmentScheme) {
    let wallet = testing_signer(0);
    let to = address!("0000000000000000000000000000000000010002");
    let batch_tester = new_multiblock_batch_tester();
    let block1_context = BlockContext {
        timestamp: 42,
        ..Default::default()
    };
    let block2_context = BlockContext {
        timestamp: 43,
        ..Default::default()
    };

    let mint_tx = {
        let mint_tx = TxLegacy {
            chain_id: 37u64.into(),
            nonce: 0,
            gas_price: 1000,
            gas_limit: 80_000,
            to: TxKind::Call(to),
            value: Default::default(),
            input: hex::decode(ERC_20_MINT_CALLDATA).unwrap().into(),
        };
        ZKsyncTxEnvelope::from_eth_tx(mint_tx, wallet.clone())
    };
    let initial_proof_data = batch_tester.prepare_native_batch_initial_proof_data();
    let batch_state = batch_tester.prepare_native_batch_state();
    let block1_batch_input = batch_tester
        .prepare_native_batch_block_input(vec![mint_tx.clone()], Some(block1_context.clone()));
    let mut legacy_tester = new_multiblock_batch_tester()
        .with_da_commitment_scheme(da_commitment_scheme)
        .with_run_config(legacy_singleblock_run_config());
    legacy_tester.set_block_context(Some(block1_context.clone()));
    let _ = legacy_tester.execute_block(vec![mint_tx]);
    let block1_run = legacy_tester
        .last_executed_block_info()
        .expect("legacy block1 run must record proof input");
    let block1_proof_input = block1_run.proof_input.clone();
    let block1_pubdata = block1_run.pubdata.clone();
    assert!(
        !block1_proof_input.is_empty(),
        "block1 proof input must be non-empty; proving run is required"
    );

    let transfer_tx = {
        let transfer_tx = TxEip1559 {
            chain_id: 37u64,
            nonce: 1,
            max_fee_per_gas: 1000,
            max_priority_fee_per_gas: 1000,
            gas_limit: 60_000,
            to: TxKind::Call(to),
            value: Default::default(),
            access_list: Default::default(),
            input: hex::decode(ERC_20_TRANSFER_CALLDATA).unwrap().into(),
        };
        ZKsyncTxEnvelope::from_eth_tx(transfer_tx, wallet.clone())
    };
    let block2_batch_input = legacy_tester
        .prepare_native_batch_block_input(vec![transfer_tx.clone()], Some(block2_context.clone()));
    legacy_tester.set_block_context(Some(block2_context.clone()));
    let _ = legacy_tester.execute_block(vec![transfer_tx]);
    let block2_run = legacy_tester
        .last_executed_block_info()
        .expect("legacy block2 run must record proof input");
    let block2_proof_input = block2_run.proof_input.clone();
    let block2_pubdata = block2_run.pubdata.clone();
    assert!(
        !block2_proof_input.is_empty(),
        "block2 proof input must be non-empty; proving run is required"
    );

    let legacy_batch_input = generate_legacy_batch_proof_input(
        vec![&block1_proof_input, &block2_proof_input],
        da_commitment_scheme,
        vec![block1_pubdata.as_slice(), block2_pubdata.as_slice()],
    );
    let native_batch_output = generate_batch_proof_input(
        initial_proof_data,
        batch_state,
        vec![block1_batch_input, block2_batch_input],
        da_commitment_scheme,
    )
    .expect("native batch prover input generation failed");

    let proof_input_mismatch =
        first_mismatch(&native_batch_output.prover_input, &legacy_batch_input);
    let mismatch_window = proof_input_mismatch.map(|idx| {
        let start = idx.saturating_sub(4);
        let end = (idx + 5)
            .min(native_batch_output.prover_input.len())
            .min(legacy_batch_input.len());
        (
            start,
            end,
            native_batch_output.prover_input[start..end].to_vec(),
            legacy_batch_input[start..end].to_vec(),
        )
    });
    assert!(
        native_batch_output.prover_input == legacy_batch_input,
        "batch-native prover input mismatch at {:?} (native len {}, legacy len {}, window {:?})",
        proof_input_mismatch,
        native_batch_output.prover_input.len(),
        legacy_batch_input.len(),
        mismatch_window,
    );
    assert_eq!(
        native_batch_output.batch_output.first_block_timestamp, block1_context.timestamp,
        "batch-native first block timestamp mismatch"
    );
    assert_eq!(
        native_batch_output.batch_output.last_block_timestamp, block2_context.timestamp,
        "batch-native last block timestamp mismatch"
    );

    let multinblock_program_path = PathBuf::from(std::env::var("CARGO_WORKSPACE_DIR").unwrap())
        .join("zksync_os")
        .join("multiblock_batch.bin");
    let proof_output = zksync_os_runner::run(
        multinblock_program_path,
        None,
        1 << 36,
        QuasiUARTSource::new_with_reads(native_batch_output.prover_input),
    );

    debug!("Proof running output = 0x");
    for word in proof_output.into_iter() {
        debug!("{word:08x}");
    }

    assert!(proof_output.into_iter().any(|word| word != 0));
}

#[test]
fn run_multiblock_batch_proof_run_calldata() {
    // The proving flow overflows the default libtest stack in this instance.
    std::thread::Builder::new()
        .name("multiblock_batch_calldata".to_owned())
        .stack_size(TEST_STACK_SIZE)
        .spawn(|| run_multiblock_batch_proof_run(DACommitmentScheme::BlobsAndPubdataKeccak256))
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn run_multiblock_batch_proof_run_blobs() {
    std::thread::Builder::new()
        .name("multiblock_batch_blobs".to_owned())
        .stack_size(TEST_STACK_SIZE)
        .spawn(|| run_multiblock_batch_proof_run(DACommitmentScheme::BlobsZKsyncOS))
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn run_multiblock_batch_proof_run_validium() {
    std::thread::Builder::new()
        .name("multiblock_batch_validium".to_owned())
        .stack_size(TEST_STACK_SIZE)
        .spawn(|| run_multiblock_batch_proof_run(DACommitmentScheme::EmptyNoDA))
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_batch_helpers_reject_empty_batches() {
    let tester = new_multiblock_batch_tester();
    let initial_proof_data = tester.prepare_native_batch_initial_proof_data();
    let batch_state = tester.prepare_native_batch_state();

    let native_batch_rejected = std::panic::catch_unwind(|| {
        let empty_blocks: Vec<
            NativeBatchBlockInput<rig::zksync_os_interface::traits::TxListSource>,
        > = Vec::new();
        let _ = generate_batch_proof_input(
            initial_proof_data,
            batch_state,
            empty_blocks,
            DACommitmentScheme::BlobsAndPubdataKeccak256,
        );
    });
    assert!(
        native_batch_rejected.is_err(),
        "native batch prover input should reject empty batches"
    );
}
