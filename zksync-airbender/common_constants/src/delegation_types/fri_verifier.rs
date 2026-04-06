//! Constants for the FRI Verifier delegation circuit.
//!
//! The ZK-OS bootloader invokes this delegation via:
//!   `CSRRW x0, 0x7CC, x0`
//!
//! Airbender's transpiler intercepts this CSR address, runs the native FRI
//! verification handler, generates the witness, and the prover proves it using
//! the circuit defined in `cs/src/delegation/fri_verifier/`.
//!
//! # Unresolved questions before finalising these constants
//! - What is the exact proof format in `proof_payload`?  (Flat `ProofSkeleton`
//!   vs SNARK/Groth16 wrapper — determines the circuit implementation.)
//! - Is this a base-layer proof or a recursive/aggregated proof?
//! - What `capacity` value is acceptable given proof verification cost?
//! - Is a new trusted setup (Merkle caps ceremony) required?

/// Base CSR address for non-determinism delegations.
/// Matches `NON_DETERMINISM_CSR` in `common_constants/src/lib.rs`.
const NON_DETERMINISM_CSR: u32 = 0x7C0;

/// CSR address used to invoke the FRI verifier delegation.
/// Assigned as `NON_DETERMINISM_CSR + 12 = 0x7CC`.
///
/// Existing assignments for reference:
/// - 0x7C7 (NON_DETERMINISM_CSR + 7)  → Blake2s
/// - 0x7CA (NON_DETERMINISM_CSR + 10) → BigInt
/// - 0x7CB (NON_DETERMINISM_CSR + 11) → Keccak
/// - 0x7CC (NON_DETERMINISM_CSR + 12) → **FRI Verifier (this file)**
pub const FRI_VERIFIER_DELEGATION_CSR: u32 = NON_DETERMINISM_CSR + 12; // 0x7CC

/// Delegation type ID assigned to the FRI verifier circuit.
/// Must be unique across all delegation type IDs and sorted in
/// `ALL_DELEGATION_CIRCUITS_PARAMS` (compile-time assert enforces ordering).
pub const FRI_VERIFIER_DELEGATION_TYPE_ID: u32 = 1996;

/// Maximum number of FRI verification instances that can be batched into one
/// delegation invocation.
///
/// **TBD** — requires profiling of the AIR circuit row count vs the target
/// proof size.  Set to `1` as a conservative placeholder.
pub const FRI_VERIFIER_CAPACITY: usize = 1;

/// RAM ABI layout for the FRI verifier delegation.
/// The bootloader writes these fields into RAM before issuing the CSRRW.
pub mod ram_abi {
    /// Byte offset of the proof payload length word.
    pub const PROOF_LEN_OFFSET: usize = 0;
    /// Byte offset where the raw proof payload bytes begin.
    pub const PROOF_DATA_OFFSET: usize = 4;
    /// Byte offset where the delegation writes the output public inputs.
    pub const PUBLIC_INPUTS_OUTPUT_OFFSET: usize = 4096;
}
