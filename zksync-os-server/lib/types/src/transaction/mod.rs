//! Top-level transaction envelope for the ZKsync OS sequencer.
//!
//! `ZkEnvelope` covers every transaction type the sequencer may encounter.
//! The `FriProof` variant carries block-level FRI proof transactions (`0x7c`).

pub mod encode;
pub mod system;

use system::fri_proof::FriProofTxEnvelope;

/// Unified transaction envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZkEnvelope {
    /// Standard L2 user transaction.
    L2(L2Envelope),
    /// L1→L2 priority transaction (`0x7f`).
    L1(L1PriorityEnvelope),
    /// L1 upgrade transaction (`0x7e`).
    L1Upgrade(L1UpgradeEnvelope),
    /// System/service transaction (`0x7d`).
    System(SystemTxEnvelope),
    /// FRI proof transaction (`0x7c`) — gateway mode only.
    FriProof(FriProofTxEnvelope),
}

impl ZkEnvelope {
    /// Returns the EIP-2718 type byte for this transaction.
    pub fn tx_type(&self) -> u8 {
        match self {
            Self::L2(tx) => tx.tx_type(),
            Self::L1(_) => 0x7f,
            Self::L1Upgrade(_) => 0x7e,
            Self::System(_) => 0x7d,
            Self::FriProof(_) => 0x7c,
        }
    }

    /// Returns `true` for transaction types that originate from the bootloader
    /// rather than an external user (FRI proof and system txs).
    pub fn is_bootloader_originated(&self) -> bool {
        matches!(self, Self::FriProof(_) | Self::System(_))
    }
}

// Placeholder structs — replace with actual types from the existing codebase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L2Envelope;
impl L2Envelope { pub fn tx_type(&self) -> u8 { 0x00 } }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L1PriorityEnvelope;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L1UpgradeEnvelope;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemTxEnvelope;
