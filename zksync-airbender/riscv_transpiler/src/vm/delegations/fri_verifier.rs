//! Transpiler VM handler for the FRI verifier delegation (CSR `0x7CC`).
//!
//! When the transpiler encounters `CSRRW x0, 0x7CC, x0`, this handler:
//!   1. Reads the `proof_payload` from RAM at the ABI-defined offset.
//!   2. Runs native FRI verification (using the `verifier` crate).
//!   3. Writes the resulting `public_inputs` back to RAM.
//!   4. Generates the `FriVerifierWitness` for the AIR circuit.
//!
//! The handler must be registered in `mod.rs` alongside existing delegations
//! (Blake2s, BigInt, Keccak, ...).

use common_constants::delegation_types::fri_verifier::{
    ram_abi, FRI_VERIFIER_DELEGATION_CSR,
};

use crate::cs::delegation::fri_verifier::air::{generate_witness, FriVerifierWitness};
use crate::vm::delegations::DelegationHandler;
use crate::vm::memory::RamView;

pub struct FriVerifierDelegationHandler;

impl DelegationHandler for FriVerifierDelegationHandler {
    const CSR_ADDRESS: u32 = FRI_VERIFIER_DELEGATION_CSR;

    fn handle(&self, ram: &mut dyn RamView) -> Result<(), crate::vm::VmError> {
        // Step 1: read proof length from RAM.
        let proof_len = ram.read_u32(ram_abi::PROOF_LEN_OFFSET)? as usize;

        // Step 2: read proof bytes from RAM.
        let mut proof_bytes = vec![0u8; proof_len];
        ram.read_bytes(ram_abi::PROOF_DATA_OFFSET, &mut proof_bytes)?;

        // Step 3: run native FRI verification to generate witness.
        let witness: FriVerifierWitness = generate_witness(&proof_bytes)
            .map_err(|e| crate::vm::VmError::DelegationFailed(e.to_string()))?;

        // Step 4: write public inputs back to RAM.
        let pi_bytes: Vec<u8> = witness
            .public_inputs
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        ram.write_bytes(ram_abi::PUBLIC_INPUTS_OUTPUT_OFFSET, &pi_bytes)?;

        Ok(())
    }
}

// Placeholder traits — replace with the actual RAM view API from the codebase.
pub trait RamView {
    fn read_u32(&self, offset: usize) -> Result<u32, crate::vm::VmError>;
    fn read_bytes(&self, offset: usize, buf: &mut [u8]) -> Result<(), crate::vm::VmError>;
    fn write_bytes(&mut self, offset: usize, data: &[u8]) -> Result<(), crate::vm::VmError>;
}
