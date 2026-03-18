//!
//! These tests are focused on multiblock batch proving inputs.
//!
#![cfg(test)]

use rig::alloy::consensus::{TxEip1559, TxLegacy};
use rig::alloy::primitives::address;
use rig::alloy::primitives::TxKind;
use rig::chain::RunConfig;
use rig::forward_system::run::convert_alloy::FromAlloy;
use rig::forward_system::run::{generate_batch_proof_input, generate_batch_proof_input_native};
use rig::ruint::aliases::U256;
use rig::utils::{ERC_20_BYTECODE, ERC_20_MINT_CALLDATA, ERC_20_TRANSFER_CALLDATA};
use rig::zk_ee::common_structs::DACommitmentScheme;
use rig::zksync_os_tests_common::zksync_tx::encoding::ZKsyncOsEncodable;
use rig::zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;
use rig::{testing_signer, Chain};

const TEST_STACK_SIZE: usize = 64 << 20;

fn first_mismatch<T: PartialEq>(lhs: &[T], rhs: &[T]) -> Option<usize> {
    lhs.iter()
        .zip(rhs.iter())
        .position(|(lhs_item, rhs_item)| lhs_item != rhs_item)
        .or_else(|| (lhs.len() != rhs.len()).then_some(lhs.len().min(rhs.len())))
}

fn run_multiblock_batch_proof_run(da_commitment_scheme: DACommitmentScheme) {
    let wallet = testing_signer(0);
    let to = address!("0000000000000000000000000000000000010002");
    let bytecode = hex::decode(ERC_20_BYTECODE).unwrap();

    let mut chain = Chain::empty(None);
    chain.set_evm_bytecode(rig::ruint::aliases::B160::from_alloy(to), &bytecode);
    chain.set_balance(
        rig::ruint::aliases::B160::from_alloy(wallet.address()),
        U256::from(1_000_000_000_000_000_u64),
    );

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
    chain.mint_tokens_to_treasury();
    let initial_proof_data = chain.prepare_native_batch_initial_proof_data();
    let block1_encoded = mint_tx.encode();
    let block1_batch_input =
        chain.prepare_native_batch_block_input(vec![block1_encoded.clone()], None);
    let (block1_output, _stats, block1_proof_input, block1_pubdata) = chain
        .run_block_with_extra_stats(
            vec![block1_encoded],
            None,
            Some(da_commitment_scheme),
            Some(RunConfig::without_riscv_run()),
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
    chain.mint_tokens_to_treasury();
    let block2_encoded = transfer_tx.encode();
    let block2_batch_input =
        chain.prepare_native_batch_block_input(vec![block2_encoded.clone()], None);
    let (block2_output, _stats, block2_proof_input, block2_pubdata) = chain
        .run_block_with_extra_stats(
            vec![block2_encoded],
            None,
            Some(da_commitment_scheme),
            Some(RunConfig::without_riscv_run()),
            &mut rig::zk_ee::system::tracer::NopTracer::default(),
            &mut rig::zk_ee::system::validator::NopTxValidator,
        )
        .unwrap();
    assert!(
        !block2_proof_input.is_empty(),
        "block2 proof input must be non-empty; proving run is required"
    );

    let assembled_batch_input = generate_batch_proof_input(
        vec![&block1_proof_input, &block2_proof_input],
        da_commitment_scheme,
        vec![block1_pubdata.as_slice(), block2_pubdata.as_slice()],
    );
    let native_batch_output = generate_batch_proof_input_native(
        initial_proof_data,
        vec![block1_batch_input, block2_batch_input],
        da_commitment_scheme,
    )
    .expect("native batch prover input generation failed");

    let proof_input_mismatch =
        first_mismatch(&native_batch_output.prover_input, &assembled_batch_input);
    let mismatch_window = proof_input_mismatch.map(|idx| {
        let start = idx.saturating_sub(4);
        let end = (idx + 5)
            .min(native_batch_output.prover_input.len())
            .min(assembled_batch_input.len());
        (
            start,
            end,
            native_batch_output.prover_input[start..end].to_vec(),
            assembled_batch_input[start..end].to_vec(),
        )
    });
    assert!(
        native_batch_output.prover_input == assembled_batch_input,
        "batch-native prover input mismatch at {:?} (native len {}, assembled len {}, window {:?})",
        proof_input_mismatch,
        native_batch_output.prover_input.len(),
        assembled_batch_input.len(),
        mismatch_window,
    );
    assert!(
        native_batch_output.pubdata == [block1_pubdata.clone(), block2_pubdata.clone()].concat(),
        "batch-native pubdata does not match concatenated block-native pubdata"
    );
    assert_eq!(
        native_batch_output.block_outputs[0].pubdata_used, block1_output.pubdata_used,
        "block 1 pubdata_used mismatch between batch-native and block-native paths"
    );
    assert_eq!(
        native_batch_output.block_outputs[1].pubdata_used, block2_output.pubdata_used,
        "block 2 pubdata_used mismatch between batch-native and block-native paths"
    );
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
