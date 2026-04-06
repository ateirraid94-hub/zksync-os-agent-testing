//! System transaction type constants and shared utilities.

/// Type byte for system (service) transactions.
pub const SYSTEM_TX_TYPE_ID: u8 = 0x7d;

/// Type byte for FRI proof transactions.
/// Sits directly below the system slot; above regular L2 transaction types.
pub const FRI_PROOF_TX_TYPE_ID: u8 = 0x7c;

// Type bytes 0x7e (L1 upgrade) and 0x7f (L1 priority) are defined elsewhere.

/// Unified enum of all ZK-specific system transaction sub-types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTxType {
    System,
    FriProof,
}

impl SystemTxType {
    pub fn from_type_byte(byte: u8) -> Option<Self> {
        match byte {
            SYSTEM_TX_TYPE_ID => Some(Self::System),
            FRI_PROOF_TX_TYPE_ID => Some(Self::FriProof),
            _ => None,
        }
    }

    pub fn type_byte(self) -> u8 {
        match self {
            Self::System => SYSTEM_TX_TYPE_ID,
            Self::FriProof => FRI_PROOF_TX_TYPE_ID,
        }
    }
}
