//! Transaction type constants for the REVM consistency checker.

/// EIP-2718 type byte for L1 upgrade transactions.
pub const UPGRADE_TRANSACTION_TYPE: u8 = 0x7E;

/// EIP-2718 type byte for L1 priority transactions.
pub const L1_PRIORITY_TRANSACTION_TYPE: u8 = 0x7F;

/// EIP-2718 type byte for system/service transactions.
pub const SERVICE_TRANSACTION_TYPE: u8 = 0x7D;

/// EIP-2718 type byte for FRI proof transactions.
///
/// Added to support re-execution of `0x7c` blocks in the consistency checker.
/// FRI proof txs share the nonce-less, signature-less semantics of system txs
/// because both originate from `BOOTLOADER_FORMAL_ADDRESS`.
pub const FRI_PROOF_TRANSACTION_TYPE: u8 = 0x7c;
