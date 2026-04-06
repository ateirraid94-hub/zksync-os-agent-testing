//! FRI proof transaction subpool.
//!
//! `FriProofSubpool` holds `0x7c` transactions that are waiting to be included
//! in the next block.  Key properties:
//!
//! - **Gateway-only**: the pool rejects additions when not in gateway mode.
//! - **Ephemeral**: entries are not persisted across sequencer restarts (a FRI
//!   proof is specific to a particular block's prover output).
//! - **Ordered above L2**: FRI proof txs are placed before L2 txs in every
//!   block, consistent with the bootloader pre-tx loop ordering constraint.

use std::collections::VecDeque;

use crate::transaction::system::fri_proof::FriProofTxEnvelope;

#[derive(Debug, Default)]
pub struct FriProofSubpool {
    /// FIFO queue — FRI proof txs must be included in arrival order.
    queue: VecDeque<FriProofTxEnvelope>,
}

#[derive(Debug, thiserror::Error)]
pub enum FriProofSubpoolError {
    #[error("FRI proof transactions are only accepted in gateway mode")]
    NotGatewayMode,
    #[error("FRI proof subpool is at capacity ({0} entries)")]
    AtCapacity(usize),
}

/// Maximum number of FRI proof txs held in the pool at once.
/// Must not exceed `MAX_FRI_PROOF_TXS_PER_BLOCK` in the bootloader.
const MAX_CAPACITY: usize = 8;

impl FriProofSubpool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attempt to add a new FRI proof transaction.
    ///
    /// `is_gateway` must be `true`; the caller is responsible for passing the
    /// current node-mode flag.
    pub fn add(
        &mut self,
        tx: FriProofTxEnvelope,
        is_gateway: bool,
    ) -> Result<(), FriProofSubpoolError> {
        if !is_gateway {
            return Err(FriProofSubpoolError::NotGatewayMode);
        }
        if self.queue.len() >= MAX_CAPACITY {
            return Err(FriProofSubpoolError::AtCapacity(MAX_CAPACITY));
        }
        self.queue.push_back(tx);
        Ok(())
    }

    /// Drain all pending FRI proof txs for block production.
    pub fn drain_for_block(&mut self) -> Vec<FriProofTxEnvelope> {
        self.queue.drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
