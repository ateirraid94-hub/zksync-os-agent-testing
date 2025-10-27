use zk_ee::utils::write_bytes::WriteBytes;
use zk_ee::utils::Bytes32;

#[cfg(feature = "aggregation")]
mod blake2s_commitment_generator;
mod keccak256_commitment_generator;

#[cfg(feature = "aggregation")]
pub use blake2s_commitment_generator::Blake2sCommitmentGenerator;
pub use keccak256_commitment_generator::Keccak256CommitmentGenerator;

pub trait DACommitmentGenerator: WriteBytes {
    // we accept Box here to make trait dyn compatible
    fn da_commitment(self: alloc::boxed::Box<Self>) -> Bytes32;
}

pub struct NopCommitmentGenerator;

impl WriteBytes for NopCommitmentGenerator {
    fn write(&mut self, _buf: &[u8]) {}
}

impl DACommitmentGenerator for NopCommitmentGenerator {
    fn da_commitment(self: alloc::boxed::Box<Self>) -> Bytes32 {
        Bytes32::zero()
    }
}
