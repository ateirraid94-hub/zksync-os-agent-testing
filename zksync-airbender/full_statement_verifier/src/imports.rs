//! Verifier imports — maps delegation type IDs to their verification functions
//! and setup parameters.
//!
//! The FRI verifier delegation entry is appended here after circuit generation.
//! The array **must** remain sorted by `delegation_type_id` — enforced by a
//! compile-time assert at the bottom of this file.

use common_constants::delegation_types::fri_verifier::{
    FRI_VERIFIER_CAPACITY, FRI_VERIFIER_DELEGATION_TYPE_ID,
};

// --- Existing delegation entries (excerpt) ---
// (blake2s, bigint, keccak entries go here unchanged)

// --- FRI Verifier ---
// These constants are populated after running `verifier_generator` and
// `recreate_verifiers.sh`.  Replace the placeholders below.

/// Merkle setup caps for the FRI verifier delegation circuit.
/// **Generated** — do not edit manually; run `recreate_verifiers.sh`.
pub const FRI_VERIFIER_SETUP_CAPS: [u8; 0] = [];

/// Verify a FRI verifier delegation proof.
///
/// Signature must match the function pointer type in
/// `BASE_LAYER_DELEGATION_CIRCUITS_VERIFICATION_PARAMETERS`.
pub fn fri_verifier_verify_proof(
    _proof: &[u8],
    _public_inputs: &[u32],
) -> Result<(), VerificationError> {
    // TODO: implement after circuit generation; delegate to the generated
    // verifier binary embedded by `recreate_verifiers.sh`.
    todo!("FRI verifier proof verification not yet generated")
}

/// Registry of all base-layer delegation circuit verification parameters.
///
/// Sorted by `delegation_type_id` — the compile-time assert below enforces
/// this invariant.
pub const BASE_LAYER_DELEGATION_CIRCUITS_VERIFICATION_PARAMETERS: &[(
    u32,   // delegation_type_id
    usize, // capacity
    &[u8], // setup caps
    fn(&[u8], &[u32]) -> Result<(), VerificationError>, // verify_fn
)] = &[
    // ... existing entries (IDs < 1996) ...
    (
        FRI_VERIFIER_DELEGATION_TYPE_ID,
        FRI_VERIFIER_CAPACITY,
        &FRI_VERIFIER_SETUP_CAPS,
        fri_verifier_verify_proof,
    ),
];

// Compile-time sort check — ensures the array is ordered by type ID.
// Extend the bound when adding more entries.
const _: () = {
    let params = BASE_LAYER_DELEGATION_CIRCUITS_VERIFICATION_PARAMETERS;
    let mut i = 1;
    while i < params.len() {
        assert!(
            params[i - 1].0 < params[i].0,
            "BASE_LAYER_DELEGATION_CIRCUITS_VERIFICATION_PARAMETERS must be sorted by type ID"
        );
        i += 1;
    }
};

#[derive(Debug)]
pub struct VerificationError;
