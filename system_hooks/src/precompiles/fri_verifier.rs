use zk_ee::{
    gas::Gas,
    memory::{MemorySlice, ReadMemory, WriteMemory},
    system::SystemInterface,
};
use crate::precompiles::{PrecompileCallResult, PrecompileError};

pub const FRI_VERIFIER_ADDRESS: u64 = 0x0101;
const FRI_BASE_GAS_COST: Gas = Gas::new(50000);

pub fn fri_verifier_precompile<S: SystemInterface>(
    system: &mut S,
    input: &MemorySlice,
    _output: &mut MemorySlice,
) -> PrecompileCallResult {
    if !system.is_gateway_mode() {
        return Err(PrecompileError::NotSupported);
    }
    
    if input.len() < 32 {
        return Err(PrecompileError::InvalidInput);
    }
    
    let mut proof_id = [0u8; 32];
    input.read_range(0..32, &mut proof_id)?;
    
    let verified_proof = system.get_verified_fri_proof(proof_id)
        .ok_or(PrecompileError::ProofNotFound)?;
    
    // Return verification result (1 byte) + public inputs hash (32 bytes)
    let mut result = vec![if verified_proof.is_valid { 1u8 } else { 0u8 }];
    result.extend_from_slice(&verified_proof.public_inputs_hash);
    
    Ok((FRI_BASE_GAS_COST, result))
}

#[derive(Debug)]
pub enum PrecompileError {
    InvalidInput,
    ProofNotFound,
    NotSupported,
    MemoryError,
}

impl From<zk_ee::memory::MemoryError> for PrecompileError {
    fn from(_: zk_ee::memory::MemoryError) -> Self {
        PrecompileError::MemoryError
    }
}

pub type PrecompileCallResult = Result<(Gas, Vec<u8>), PrecompileError>;
