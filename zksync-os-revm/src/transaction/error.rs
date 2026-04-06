//! REVM consistency checker error types for ZKsync transaction validation.

use revm_primitives::InvalidTransaction;

/// Errors specific to ZKsync OS transaction validation in the REVM checker.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum ZKsyncTxError {
    /// Wraps a standard REVM `InvalidTransaction` error.
    #[error("base transaction error: {0:?}")]
    Base(InvalidTransaction),

    /// A `0x7c` FRI proof transaction was submitted with a caller address other
    /// than `BOOTLOADER_FORMAL_ADDRESS`.
    #[error("FRI proof transaction caller must be BOOTLOADER_FORMAL_ADDRESS")]
    InvalidFriProofTxCaller,
}

impl From<InvalidTransaction> for ZKsyncTxError {
    fn from(e: InvalidTransaction) -> Self {
        Self::Base(e)
    }
}
