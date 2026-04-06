//! Transaction-level error codes used by the bootloader.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidTransaction {
    // --- existing variants ---
    /// A `0x7c` FRI proof transaction appeared after at least one non-FRI-proof
    /// transaction in the same block.  All `0x7c` txs must precede all L2 txs.
    FriProofTxOutOfOrder,

    /// The block contains more `0x7c` FRI proof transactions than the
    /// `MAX_FRI_PROOF_TXS_PER_BLOCK` constant allows.
    TooManyFriProofTxs,

    /// A `0x7c` transaction has a malformed or empty proof payload.
    MalformedFriProofTx,
    // ... existing variants follow ...
}
