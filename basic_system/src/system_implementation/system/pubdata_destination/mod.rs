use crypto::blake2s::Blake2s256;
use crypto::MiniDigest;
use zk_ee::oracle::IOOracle;
use zk_ee::utils::Bytes32;
use zk_ee::utils::write_bytes::WriteBytes;

pub mod blob_commitment_generator;
pub mod keccak256_commitment_generator;

pub trait DACommitmentGenerator: WriteBytes {
    // TODO: self? Issues with dyn impl
    fn da_commitment(&mut self) -> Bytes32;
}

pub struct NopCommitmentGenerator;

impl WriteBytes for NopCommitmentGenerator {
    fn write(&mut self, _buf: &[u8]) {}
}


impl DACommitmentGenerator for NopCommitmentGenerator {
    fn da_commitment(&mut self) -> Bytes32 {
        Bytes32::zero()
    }
}

pub struct Blake2sCommitmentGenerator {
    pubdata_hasher: Blake2s256
}

impl Blake2sCommitmentGenerator {
    pub fn new() -> Self {
        Self {
            pubdata_hasher: Blake2s256::new()
        }
    }
}

impl WriteBytes for Blake2sCommitmentGenerator {
    fn write(&mut self, buf: &[u8]) {
        self.pubdata_hasher.update(buf)
    }
}

impl DACommitmentGenerator for Blake2sCommitmentGenerator {
    fn da_commitment(&mut self) -> Bytes32 {
        self.pubdata_hasher.finalize_reset().into()
    }
}