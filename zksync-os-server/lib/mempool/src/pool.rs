//! Transaction pool — routes incoming transactions to the correct subpool.
//!
//! Extended with a `FriProofSubpool` for type `0x7c` transactions.

use crate::subpools::fri_proof::FriProofSubpool;
use crate::transaction::system::fri_proof::FriProofTxEnvelope;
use crate::transaction::ZkEnvelope;

/// The top-level transaction pool.
pub struct Pool<T> {
    // ... existing subpools ...
    pub fri_proof: FriProofSubpool,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Pool<T> {
    pub fn new() -> Self {
        Self {
            fri_proof: FriProofSubpool::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add a FRI proof transaction to the dedicated subpool.
    ///
    /// Returns an error if the node is not in gateway mode or the subpool is
    /// at capacity.
    pub fn add_fri_proof_transaction(
        &mut self,
        tx: FriProofTxEnvelope,
        is_gateway: bool,
    ) -> Result<(), crate::subpools::fri_proof::FriProofSubpoolError> {
        self.fri_proof.add(tx, is_gateway)
    }

    /// Route an incoming `ZkEnvelope` to the appropriate subpool.
    pub fn add_transaction(
        &mut self,
        tx: ZkEnvelope,
        is_gateway: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match tx {
            ZkEnvelope::FriProof(fri_tx) => {
                self.fri_proof.add(fri_tx, is_gateway)?;
            }
            // Route other types to their existing subpools.
            _ => { /* existing routing logic */ }
        }
        Ok(())
    }
}
