//! AIR constraint system for the FRI verifier delegation.
//!
//! **Skeleton file** — the actual constraint implementation requires significant
//! cryptographic engineering effort.  The structure below documents the intended
//! layout; constraint bodies are left as `todo!()` placeholders.

use crate::delegation::DelegationCircuit;
use common_constants::delegation_types::fri_verifier::{
    FRI_VERIFIER_CAPACITY, FRI_VERIFIER_DELEGATION_TYPE_ID,
};

/// AIR circuit proving correct FRI verification for one proof payload.
#[derive(Debug, Clone)]
pub struct FriVerifierDelegationCircuit;

impl DelegationCircuit for FriVerifierDelegationCircuit {
    const TYPE_ID: u32 = FRI_VERIFIER_DELEGATION_TYPE_ID;
    const CAPACITY: usize = FRI_VERIFIER_CAPACITY;

    fn circuit_name() -> &'static str {
        "FriVerifier"
    }
}

/// Witness layout for a single FRI verification instance.
///
/// All values are `Mersenne31Quartic` field elements, consistent with the rest
/// of the airbender proving system.
#[derive(Debug, Default)]
pub struct FriVerifierWitness {
    /// Serialised proof payload (u32 words as produced by proof_flattener).
    pub proof_words: Vec<u32>,
    /// Expected public inputs (Merkle root, etc.) extracted from the payload.
    pub public_inputs: Vec<u32>,
    // FRI folding intermediate values, Merkle paths, etc. go here.
    // These must be determined from the proof format spec.
}

/// Generate a `FriVerifierWitness` by running native FRI verification.
///
/// Called by the transpiler VM handler before circuit synthesis.
///
/// # Errors
/// Returns `Err` if `proof_bytes` fails native verification (malformed proof).
pub fn generate_witness(
    _proof_bytes: &[u8],
) -> Result<FriVerifierWitness, FriVerifierError> {
    // TODO: integrate with `verifier` or `full_statement_verifier` crate to:
    // 1. Deserialise proof_bytes via proof_flattener.
    // 2. Run native FRI folding checks.
    // 3. Populate FriVerifierWitness fields.
    todo!("FRI verifier witness generation not yet implemented")
}

#[derive(Debug, thiserror::Error)]
pub enum FriVerifierError {
    #[error("proof deserialization failed: {0}")]
    DeserializationFailed(String),
    #[error("FRI folding check failed at round {round}")]
    FriFoldingFailed { round: usize },
    #[error("Merkle inclusion check failed")]
    MerkleInclusionFailed,
}
