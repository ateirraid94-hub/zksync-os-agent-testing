//! ZKsync OS transaction encoding — maps `ZkEnvelope` variants to
//! `TransactionData` for VM ingestion.

use crate::transaction::system::fri_proof::{FriProofTxEnvelope, BOOTLOADER_FORMAL_ADDRESS};

/// Opaque VM transaction data.  Matches the layout expected by the bootloader.
pub struct TransactionData {
    pub tx_type: u8,
    pub from: [u8; 20],
    pub data: Vec<u8>,
}

/// Encode a `FriProofTxEnvelope` into the `TransactionData` format consumed
/// by the VM block-building pipeline.
pub fn encode_fri_proof_tx(env: &FriProofTxEnvelope) -> TransactionData {
    TransactionData {
        tx_type: 0x7c,
        from: *BOOTLOADER_FORMAL_ADDRESS,
        // proof_payload is passed as raw calldata; the bootloader parses it.
        data: env.proof_payload().to_vec(),
    }
}
