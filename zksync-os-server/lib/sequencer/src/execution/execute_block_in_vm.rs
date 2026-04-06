//! Block execution — feeds ordered transactions to the VM.
//!
//! FRI proof transactions (`0x7c`) are placed at the front of the block,
//! consistent with the bootloader's pre-tx loop ordering requirement.
//!
//! Failure of a FRI proof tx is treated as fatal to block production
//! (mirrors the policy for system txs).

use crate::transaction::encode::encode_fri_proof_tx;
use crate::transaction::ZkEnvelope;

/// Execute a single transaction inside the VM.
///
/// # Errors
/// Returns a fatal error for `FriProof` failures — the block cannot proceed
/// if the proof cannot be ingested.
pub fn execute_transaction_in_vm(
    tx: &ZkEnvelope,
    vm: &mut dyn VmInterface,
) -> Result<TxExecutionResult, FatalBlockError> {
    match tx {
        ZkEnvelope::FriProof(fri_tx) => {
            let tx_data = encode_fri_proof_tx(fri_tx);
            vm.execute_fri_proof_tx(tx_data)
                .map_err(|e| FatalBlockError::FriProofTxFailed(e.to_string()))
        }
        // Existing branches for L2, L1, System, L1Upgrade follow unchanged.
        _ => unimplemented!("delegate to existing execution paths"),
    }
}

// Placeholder trait / types — match actual VM interface in the codebase.
pub trait VmInterface {
    fn execute_fri_proof_tx(&mut self, data: crate::transaction::encode::TransactionData)
        -> Result<TxExecutionResult, VmError>;
}

pub struct TxExecutionResult;

#[derive(Debug, thiserror::Error)]
pub enum FatalBlockError {
    #[error("FRI proof tx failed: {0}")]
    FriProofTxFailed(String),
}

#[derive(Debug, thiserror::Error)]
#[error("VM error")]
pub struct VmError;
