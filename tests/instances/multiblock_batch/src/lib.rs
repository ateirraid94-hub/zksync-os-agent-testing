//!
//! These tests are focused on multiblock batch proving inputs.
//!
#![cfg(test)]

use rig::alloy::consensus::{TxEip1559, TxLegacy};
use rig::alloy::primitives::address;
use rig::alloy::primitives::TxKind;
use rig::chain::RunConfig;
use rig::forward_system::run::convert_alloy::FromAlloy;
use rig::forward_system::run::{
    generate_batch_proof_input, generate_legacy_batch_proof_input, NativeBatchBlockInput,
};
use rig::log::debug;
use rig::ruint::aliases::U256;
use rig::utils::{ERC_20_BYTECODE, ERC_20_MINT_CALLDATA, ERC_20_TRANSFER_CALLDATA};
use rig::zk_ee::common_structs::DACommitmentScheme;
use rig::zksync_os_tests_common::zksync_tx::encoding::ZKsyncOsEncodable;
use rig::zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;
use rig::{testing_signer, BlockContext, Chain};
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

fn run_multiblock_batch_proof_run(da_commitment_scheme: DACommitmentScheme) {
    let wallet = testing_signer(0);
    let to = address!("0000000000000000000000000000000000010002");
    let bytecode = hex::decode(ERC_20_BYTECODE).unwrap();

    let mut batch_chain = Chain::empty(None);
    batch_chain.set_evm_bytecode(rig::ruint::aliases::B160::from_alloy(to), &bytecode);
    batch_chain.set_balance(
        rig::ruint::aliases::B160::from_alloy(wallet.address()),
        U256::from(1_000_000_000_000_000_u64),
    );
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
    batch_chain.mint_tokens_to_treasury();
    let initial_proof_data = batch_chain.prepare_native_batch_initial_proof_data();
    let batch_state = batch_chain.prepare_native_batch_state();
    let block1_encoded = mint_tx.encode();
    let block1_batch_input = batch_chain.prepare_native_batch_block_input(
        vec![block1_encoded.clone()],
        Some(block1_context.clone()),
    );
    let mut legacy_chain = batch_chain.clone();
    legacy_chain.mint_tokens_to_treasury();
    let legacy_run_config = legacy_singleblock_run_config();
    let (_block1_output, _stats, block1_proof_input, block1_pubdata) = legacy_chain
        .run_block_with_extra_stats(
            vec![block1_encoded],
            Some(block1_context.clone()),
            Some(da_commitment_scheme),
            Some(legacy_run_config.clone()),
            &mut rig::zk_ee::system::tracer::NopTracer::default(),
            &mut rig::zk_ee::system::validator::NopTxValidator,
        )
        .unwrap();
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
    let block2_encoded = transfer_tx.encode();
    legacy_chain.mint_tokens_to_treasury();
    let block2_batch_input = legacy_chain.prepare_native_batch_block_input(
        vec![block2_encoded.clone()],
        Some(block2_context.clone()),
    );
    let (_block2_output, _stats, block2_proof_input, block2_pubdata) = legacy_chain
        .run_block_with_extra_stats(
            vec![block2_encoded],
            Some(block2_context.clone()),
            Some(da_commitment_scheme),
            Some(legacy_run_config),
            &mut rig::zk_ee::system::tracer::NopTracer::default(),
            &mut rig::zk_ee::system::validator::NopTxValidator,
        )
        .unwrap();
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
    let chain = Chain::empty(None);
    let initial_proof_data = chain.prepare_native_batch_initial_proof_data();
    let batch_state = chain.prepare_native_batch_state();

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
