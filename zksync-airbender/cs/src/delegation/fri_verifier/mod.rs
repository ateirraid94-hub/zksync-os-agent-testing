//! FRI Verifier AIR delegation circuit.
//!
//! This module defines the Algebraic Intermediate Representation (AIR) circuit
//! that proves correct execution of the FRI verification delegation at CSR
//! address `0x7CC`.
//!
//! ## High-level structure
//! The circuit implements FRI folding + Merkle inclusion verification over
//! `Mersenne31Quartic` field elements, consistent with the rest of the airbender
//! proving system.
//!
//! The `proof_payload` input is the binary layout defined by `ProofSkeleton` in
//! `verifier/src/skeleton.rs`, serialised as `u32` words by `proof_flattener.rs`.
//!
//! ## WARNING — implementation size
//! FRI verification is computationally expensive in-circuit.  This circuit is
//! expected to be comparable in size to the full statement verifier.  Do not
//! underestimate the engineering effort required.
//!
//! ## Open questions that block finalisation
//! - Confirm proof format: flat `ProofSkeleton` vs SNARK wrapper.
//! - Confirm recursion depth: base-layer or aggregated proof.
//! - Profile AIR row count to set `FRI_VERIFIER_CAPACITY` correctly.
//! - Determine whether a new Merkle setup caps ceremony is required.

pub mod air;
pub mod verifier;

pub use air::FriVerifierDelegationCircuit;
