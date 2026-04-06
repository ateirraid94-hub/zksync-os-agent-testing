//! REVM execution handler — extended for FRI proof transaction replay.
//!
//! Four methods contain new `is_fri_proof_tx()` branches:
//!
//! | Method | Change |
//! |---|---|
//! | `validate_env` | Skip env validation entirely (same bypass as L1 txs) |
//! | `validate_initial_tx_gas` | Saturate gas limit check |
//! | `validate_against_state_and_deduct_caller` | Skip balance/nonce; enforce caller constraint |
//! | `reimburse_caller` / `reward_beneficiary` | No-op — bootloader pays no gas |

use crate::constants::BOOTLOADER_FORMAL_ADDRESS;
use crate::transaction::abstraction::ZksyncTxAbstraction;
use crate::transaction::error::ZKsyncTxError;

/// Validate the execution environment for a transaction.
///
/// FRI proof txs bypass all environment validation (block base-fee, chain-id,
/// etc.) — they are sequencer-generated and assumed structurally valid.
pub fn validate_env<Tx: ZksyncTxAbstraction>(tx: &Tx) -> Result<(), ZKsyncTxError> {
    if tx.is_l1_tx() || tx.is_fri_proof_tx() {
        return Ok(());
    }
    // ... existing validation for L2 txs ...
    Ok(())
}

/// Validate and saturate the gas limit for a transaction.
///
/// FRI proof txs use the same saturating behaviour as L1 txs: the gas limit
/// in the tx is ignored and the full block gas limit is granted.
pub fn validate_initial_tx_gas<Tx: ZksyncTxAbstraction>(
    tx: &Tx,
    block_gas_limit: u64,
) -> Result<u64, ZKsyncTxError> {
    if tx.is_l1_tx() || tx.is_fri_proof_tx() {
        return Ok(block_gas_limit);
    }
    // ... existing gas validation for L2 txs ...
    Ok(block_gas_limit)
}

/// Validate sender state and deduct gas from caller balance.
///
/// FRI proof txs:
/// - Must originate from `BOOTLOADER_FORMAL_ADDRESS` (hard error if not).
/// - Nonce and balance checks are skipped — the bootloader account has neither.
pub fn validate_against_state_and_deduct_caller<Tx, Journal>(
    tx: &Tx,
    journal: &mut Journal,
) -> Result<(), ZKsyncTxError>
where
    Tx: ZksyncTxAbstraction + CallerProvider,
    Journal: AccountLoader,
{
    if tx.is_fri_proof_tx() {
        if tx.caller() != BOOTLOADER_FORMAL_ADDRESS {
            return Err(ZKsyncTxError::InvalidFriProofTxCaller);
        }
        // Touch the account so the journal has an entry, but make no deductions.
        journal.load_account_with_code_mut(tx.caller())?;
        return Ok(());
    }

    if tx.is_l1_tx() {
        // Existing L1 tx logic — no deduction.
        return Ok(());
    }

    // ... existing L2 tx balance/nonce logic ...
    Ok(())
}

/// Reimburse unused gas to the caller after execution.
///
/// FRI proof txs and system-level txs pay no gas; this is a no-op for them.
pub fn reimburse_caller<Tx: ZksyncTxAbstraction>(
    tx: &Tx,
    _refund_gas: u64,
) -> Result<(), ZKsyncTxError> {
    if tx.is_system_level_tx() {
        return Ok(());
    }
    // ... existing reimbursement logic ...
    Ok(())
}

/// Distribute the base-fee to the block beneficiary.
///
/// No-op for FRI proof txs — the bootloader address pays no fees.
pub fn reward_beneficiary<Tx: ZksyncTxAbstraction>(
    tx: &Tx,
    _beneficiary: revm_primitives::Address,
    _gas_used: u64,
) -> Result<(), ZKsyncTxError> {
    if tx.is_system_level_tx() {
        return Ok(());
    }
    // ... existing fee distribution logic ...
    Ok(())
}

// Placeholder traits — match actual types in the codebase.
pub trait CallerProvider {
    fn caller(&self) -> revm_primitives::Address;
}

pub trait AccountLoader {
    fn load_account_with_code_mut(
        &mut self,
        addr: revm_primitives::Address,
    ) -> Result<(), ZKsyncTxError>;
}
