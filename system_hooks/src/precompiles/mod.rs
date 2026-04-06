mod fri_verifier;

use fri_verifier::{fri_verifier_precompile, FRI_VERIFIER_ADDRESS};
use zk_ee::{
    gas::Gas,
    memory::MemorySlice,
    system::SystemInterface,
};

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

pub fn get_precompile_handler<S: SystemInterface>(
    address: u64,
) -> Option<fn(&mut S, &MemorySlice, &mut MemorySlice) -> PrecompileCallResult> {
    match address {
        FRI_VERIFIER_ADDRESS => Some(fri_verifier_precompile),
        _ => None,
    }
}
