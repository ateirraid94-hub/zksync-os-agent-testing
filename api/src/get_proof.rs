//! Re-exports and convenience helpers for `zks_getProof` verification.

use crypto::MiniDigest;

pub use zks_get_proof_verifier::{
    LeafWithProof, StateCommitmentPreimage, StorageProof, StorageProofType, ZksGetProofHasher,
    ZksGetProofResponse, ZksGetProofVerificationError, MAX_32_BYTES, ZERO_32_BYTES,
};

pub use zks_get_proof_verifier::compute_state_commitment as compute_state_commitment_with_hasher;
pub use zks_get_proof_verifier::verify_response as verify_response_with_hasher;

#[derive(Clone, Debug)]
pub struct Blake2sGetProofHasher {
    hasher: crypto::blake2s::Blake2s256,
}

impl Blake2sGetProofHasher {
    pub fn new() -> Self {
        Self {
            hasher: crypto::blake2s::Blake2s256::new(),
        }
    }
}

impl Default for Blake2sGetProofHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl ZksGetProofHasher for Blake2sGetProofHasher {
    fn update(&mut self, input: impl AsRef<[u8]>) {
        self.hasher.update(input);
    }

    fn finalize_reset(&mut self) -> [u8; 32] {
        self.hasher.finalize_reset()
    }
}

pub fn compute_state_commitment(
    state_root: &[u8; 32],
    preimage: &StateCommitmentPreimage,
) -> [u8; 32] {
    let mut hasher = Blake2sGetProofHasher::new();
    zks_get_proof_verifier::compute_state_commitment(&mut hasher, state_root, preimage)
}

pub fn verify_response<const N: usize>(
    response: &ZksGetProofResponse,
    expected_batch_hash: &[u8; 32],
) -> Result<Vec<[u8; 32]>, ZksGetProofVerificationError> {
    let mut hasher = Blake2sGetProofHasher::new();
    zks_get_proof_verifier::verify_response::<N, _>(response, expected_batch_hash, &mut hasher)
}
