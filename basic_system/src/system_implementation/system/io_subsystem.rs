//! IO subsystem — extended with FRI proof context storage.
//!
//! `FriProofBlockContext` follows the same pattern as
//! `new_settlement_layer_chain_id_storage`: written once during the pre-tx
//! loop and read-only thereafter by the precompile at address `0x12`.

use alloc::vec::Vec;

/// A single FRI proof payload stored for in-block lookup.
#[derive(Debug, Clone)]
pub struct FriProofEntry {
    /// Raw RLP-encoded proof payload from the `0x7c` transaction.
    pub payload: Vec<u8>,
}

/// Block-scoped storage for all FRI proof entries, indexed by arrival order.
#[derive(Debug, Default)]
pub struct FriProofBlockContext {
    entries: Vec<FriProofEntry>,
}

impl FriProofBlockContext {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Store a new FRI proof entry at `index`.
    ///
    /// Entries must be inserted in order (index == entries.len()); the
    /// pre-tx loop guarantees this.
    pub fn insert(&mut self, index: usize, payload: Vec<u8>) {
        debug_assert_eq!(index, self.entries.len(), "FriProofBlockContext: non-sequential insert");
        self.entries.push(FriProofEntry { payload });
    }

    /// Look up a FRI proof entry by index.  Returns `None` for out-of-range
    /// indices (precompile should return `(false, "")` in that case).
    pub fn get(&self, index: usize) -> Option<&FriProofEntry> {
        self.entries.get(index)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Extension trait — implementations must add `fri_proof_context` to their
/// `ZKBasicBlockDataKeeper` analogue and implement these methods.
pub trait IOSubsystemFriExt {
    /// Store a FRI proof entry during the pre-tx loop.
    fn add_fri_proof_entry(&mut self, index: usize, payload: Vec<u8>);

    /// Retrieve a stored FRI proof entry (called by the precompile).
    fn get_fri_proof_entry(&self, index: usize) -> Option<&FriProofEntry>;
}
