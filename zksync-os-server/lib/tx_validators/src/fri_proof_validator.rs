//! Validator for FRI proof transactions (`0x7c`).
//!
//! Enforces two hard invariants before a `0x7c` tx is admitted to the pool:
//!
//! 1. The initiator must be `BOOTLOADER_FORMAL_ADDRESS`.
//! 2. The transaction may only be accepted on gateway-mode sequencers.

use crate::transaction::system::fri_proof::{FriProofTxEnvelope, BOOTLOADER_FORMAL_ADDRESS};

#[derive(Debug, thiserror::Error)]
pub enum FriProofValidationError {
    #[error(
        "FRI proof tx initiator must be BOOTLOADER_FORMAL_ADDRESS, got {actual:?}"
    )]
    InvalidInitiator { actual: alloy_primitives::Address },

    #[error("FRI proof transactions are only valid in gateway mode")]
    NotGatewayMode,

    #[error("proof_payload is empty or has an unsupported version byte 0x{version:02x}")]
    UnsupportedPayloadVersion { version: u8 },
}

/// Validate a `FriProofTxEnvelope` before pool admission.
pub fn validate(
    tx: &FriProofTxEnvelope,
    is_gateway: bool,
) -> Result<(), FriProofValidationError> {
    // Invariant 1: caller must be the bootloader address.
    if tx.initiator() != BOOTLOADER_FORMAL_ADDRESS {
        return Err(FriProofValidationError::InvalidInitiator {
            actual: tx.initiator(),
        });
    }

    // Invariant 2: gateway mode only.
    if !is_gateway {
        return Err(FriProofValidationError::NotGatewayMode);
    }

    // Invariant 3: proof_payload must have a known version byte.
    let payload = tx.proof_payload();
    if payload.is_empty() || payload[0] == 0x00 {
        let version = payload.first().copied().unwrap_or(0x00);
        return Err(FriProofValidationError::UnsupportedPayloadVersion { version });
    }

    Ok(())
}
