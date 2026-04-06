//! Transaction abstraction layer for the REVM consistency checker.
//!
//! Extended with `is_fri_proof_tx()` so that handler methods can gate
//! FRI-proof-specific logic without matching on the raw type byte.

use crate::transaction::priority_tx::{
    FRI_PROOF_TRANSACTION_TYPE, L1_PRIORITY_TRANSACTION_TYPE,
    SERVICE_TRANSACTION_TYPE, UPGRADE_TRANSACTION_TYPE,
};

/// Extension trait adding ZKsync-specific transaction type predicates.
pub trait ZksyncTxAbstraction {
    fn tx_type(&self) -> u8;

    fn is_l1_tx(&self) -> bool {
        matches!(
            self.tx_type(),
            L1_PRIORITY_TRANSACTION_TYPE | UPGRADE_TRANSACTION_TYPE
        )
    }

    fn is_service_tx(&self) -> bool {
        self.tx_type() == SERVICE_TRANSACTION_TYPE
    }

    /// Returns `true` for FRI proof transactions (`0x7c`).
    ///
    /// These share nonce-less, balance-free semantics with service txs: the
    /// caller is always `BOOTLOADER_FORMAL_ADDRESS` and no gas is charged.
    fn is_fri_proof_tx(&self) -> bool {
        self.tx_type() == FRI_PROOF_TRANSACTION_TYPE
    }

    /// Returns `true` for any transaction that bypasses standard validation
    /// (L1 priority, upgrade, service, or FRI proof).
    fn is_system_level_tx(&self) -> bool {
        self.is_l1_tx() || self.is_service_tx() || self.is_fri_proof_tx()
    }
}
