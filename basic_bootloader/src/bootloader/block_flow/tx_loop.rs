//! Main transaction processing loop for the bootloader.
//!
//! Ordering invariant: `0x7c` FRI proof transactions **must not** appear after
//! any non-`0x7c` transaction.  Violations produce
//! `InvalidTransaction::FriProofTxOutOfOrder` and halt block production.

use crate::bootloader::errors::InvalidTransaction;

/// Enforces that a `0x7c` transaction does not appear after a non-`0x7c` one.
///
/// Call once per transaction, passing whether at least one non-FRI-proof tx
/// has been seen already (`regular_tx_seen`).
pub fn check_fri_proof_ordering(
    tx_type: u8,
    regular_tx_seen: bool,
) -> Result<(), InvalidTransaction> {
    if tx_type == 0x7c && regular_tx_seen {
        Err(InvalidTransaction::FriProofTxOutOfOrder)
    } else {
        Ok(())
    }
}
