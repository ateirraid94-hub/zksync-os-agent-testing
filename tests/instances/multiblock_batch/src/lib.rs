//!
//! These tests are focused on different tx types
//!
#![cfg(test)]

use alloy::consensus::{TxEip1559, TxLegacy};
use alloy::primitives::TxKind;
use alloy::signers::local::PrivateKeySigner;
use rig::alloy::primitives::address;
use rig::chain::BlockToRun;
use rig::ruint::aliases::{B160, U256};
use rig::utils::{ERC_20_BYTECODE, ERC_20_MINT_CALLDATA, ERC_20_TRANSFER_CALLDATA};
use rig::zk_ee::common_structs::DACommitmentScheme;
use rig::{alloy, zksync_web3_rs, Chain};

use std::str::FromStr;
use zksync_web3_rs::signers::{LocalWallet, Signer};

fn run_multiblock_batch_proof_run(da_commitment_scheme: DACommitmentScheme) {
    let mut chain = Chain::empty(None);
    let wallet = PrivateKeySigner::from_str(
        "dcf2cbdd171a21c480aa7f53d77f31bb102282b3ff099c78e3118b37348c72f7",
    )
    .unwrap();
    let wallet_ethers = LocalWallet::from_bytes(wallet.to_bytes().as_slice()).unwrap();

    let from = wallet_ethers.address();
    let to = address!("0000000000000000000000000000000000010002");

    let bytecode = hex::decode(ERC_20_BYTECODE).unwrap();
    chain.set_evm_bytecode(B160::from_be_bytes(to.into_array()), &bytecode);

    chain.set_balance(
        B160::from_be_bytes(from.0),
        U256::from(1_000_000_000_000_000_u64),
    );

    let encoded_mint_tx = {
        let mint_tx = TxLegacy {
            chain_id: 37u64.into(),
            nonce: 0,
            gas_price: 1000,
            gas_limit: 80_000,
            to: TxKind::Call(to),
            value: Default::default(),
            input: hex::decode(ERC_20_MINT_CALLDATA).unwrap().into(),
        };
        rig::utils::sign_and_encode_alloy_tx(mint_tx, &wallet)
    };

    let block1 = BlockToRun {
        transactions: vec![encoded_mint_tx],
        block_context: None,
    };

    let encoded_transfer_tx = {
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
        rig::utils::sign_and_encode_alloy_tx(transfer_tx, &wallet)
    };
    let block2 = BlockToRun {
        transactions: vec![encoded_transfer_tx],
        block_context: None,
    };

    chain.run_multiblock_batch_proof_run_on_two_blocks(block1, block2, da_commitment_scheme)
}

#[test]
fn run_multiblock_batch_proof_run_calldata() {
    run_multiblock_batch_proof_run(DACommitmentScheme::BlobsAndPubdataKeccak256);
}

#[test]
fn run_multiblock_batch_proof_run_blobs() {
    run_multiblock_batch_proof_run(DACommitmentScheme::BlobsZKsyncOS);
}
