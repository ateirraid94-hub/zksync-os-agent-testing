//! Helpers for the `zks_getProof` API.
//!
//! This module keeps prover/testing integration for `FlatStorageBacking`.
//! Shared `zks_getProof` types and verification logic live in the standalone
//! `zks_get_proof_verifier` crate.

use zk_ee::utils::Bytes32 as StorageBytes32;

pub use zks_get_proof_verifier::{
    compute_state_commitment, verify_response, LeafWithProof, StateCommitmentPreimage,
    StorageProof, StorageProofType, ZksGetProofHasher, ZksGetProofResponse,
    ZksGetProofVerificationError, ZERO_32_BYTES,
};

#[inline(always)]
fn to_proof_bytes32(value: StorageBytes32) -> [u8; 32] {
    value.as_u8_array()
}

#[cfg(test)]
#[inline(always)]
fn from_proof_bytes32(value: [u8; 32]) -> StorageBytes32 {
    StorageBytes32::from_array(value)
}

/// Prover helpers (testing only).
pub mod prover {
    use alloc::alloc::Global;
    use alloc::vec::Vec;
    use core::alloc::Allocator;

    use crate::system_implementation::flat_storage_model::simple_growable_storage::FlatStorageHasher;
    use crypto::MiniDigest;
    use ruint::aliases::B160;
    use zk_ee::common_structs::derive_flat_storage_key_with_hasher;
    use zk_ee::utils::Bytes32 as StorageBytes32;

    use super::super::{compute_empty_hashes, Blake2sStorageHasher, FlatStorageBacking};
    use super::{
        to_proof_bytes32, LeafWithProof, StateCommitmentPreimage, StorageProof, StorageProofType,
        ZksGetProofResponse,
    };

    fn compress_siblings<const N: usize>(
        path: &[StorageBytes32; N],
        empty_hashes: &[StorageBytes32; N],
    ) -> Vec<[u8; 32]> {
        let mut last_non_empty: Option<usize> = None;
        for i in (0..N).rev() {
            if path[i] != empty_hashes[i] {
                last_non_empty = Some(i);
                break;
            }
        }
        let len = last_non_empty.map(|idx| idx + 1).unwrap_or(0);
        path[..len]
            .iter()
            .copied()
            .map(to_proof_bytes32)
            .collect::<Vec<_>>()
    }

    fn leaf_with_proof<const N: usize>(
        proof: &super::super::LeafProof<N, Blake2sStorageHasher, impl Allocator>,
        empty_hashes: &[StorageBytes32; N],
    ) -> LeafWithProof {
        let path: &[StorageBytes32; N] = &proof.path;
        LeafWithProof {
            index: proof.index,
            leaf_key: to_proof_bytes32(proof.leaf.key),
            value: to_proof_bytes32(proof.leaf.value),
            next_index: proof.leaf.next,
            siblings: compress_siblings(path, empty_hashes),
        }
    }

    impl<const N: usize, const RANDOMIZED: bool, A: Allocator + Clone + Default>
        FlatStorageBacking<N, Blake2sStorageHasher, RANDOMIZED, A>
    {
        /// Builds the commitment preimage from the current tree and block metadata.
        pub fn state_commitment_preimage(
            &self,
            block_number: u64,
            last256_block_hashes_blake: StorageBytes32,
            last_block_timestamp: u64,
        ) -> StateCommitmentPreimage {
            StateCommitmentPreimage {
                next_free_slot: self.next_free_slot,
                block_number,
                last256_block_hashes_blake: to_proof_bytes32(last256_block_hashes_blake),
                last_block_timestamp,
            }
        }

        /// Produces a `zks_getProof`-style response for the requested keys.
        pub fn prove_zks_get_proof(
            &self,
            address: B160,
            keys: &[StorageBytes32],
            preimage: StateCommitmentPreimage,
        ) -> ZksGetProofResponse {
            let mut empty_hasher = Blake2sStorageHasher::new();
            let empty_hashes =
                compute_empty_hashes::<N, Blake2sStorageHasher, Global>(&mut empty_hasher, Global);
            let mut key_hasher = crypto::blake2s::Blake2s256::new();

            let mut storage_proofs = Vec::with_capacity(keys.len());
            for key in keys {
                let flat_key = derive_flat_storage_key_with_hasher(&address, key, &mut key_hasher);
                let proof = match self.get(&flat_key) {
                    super::super::ReadValueWithProof::Existing { proof } => {
                        let path: &[StorageBytes32; N] = &proof.existing.path;
                        StorageProofType::Existing {
                            index: proof.existing.index,
                            value: to_proof_bytes32(proof.existing.leaf.value),
                            next_index: proof.existing.leaf.next,
                            siblings: compress_siblings(path, &empty_hashes),
                        }
                    }
                    super::super::ReadValueWithProof::New { proof, .. } => {
                        let left_neighbor = leaf_with_proof(&proof.previous, &empty_hashes);
                        let right_neighbor = leaf_with_proof(&proof.next, &empty_hashes);
                        StorageProofType::NonExisting {
                            left_neighbor,
                            right_neighbor,
                        }
                    }
                };

                storage_proofs.push(StorageProof {
                    key: to_proof_bytes32(*key),
                    proof,
                });
            }

            ZksGetProofResponse {
                address,
                state_commitment_preimage: preimage,
                storage_proofs,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::alloc::Global;
    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;
    use crypto::MiniDigest;
    use rand::rngs::StdRng;
    use rand::{RngCore, SeedableRng};
    use ruint::aliases::B160;
    use zk_ee::common_structs::derive_flat_storage_key;

    use super::super::{TestingTree, TREE_HEIGHT};

    #[derive(Clone, Debug)]
    struct Blake2sGetProofHasher {
        hasher: crypto::blake2s::Blake2s256,
    }

    impl Blake2sGetProofHasher {
        fn new() -> Self {
            Self {
                hasher: crypto::blake2s::Blake2s256::new(),
            }
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

    fn random_bytes32(rng: &mut impl RngCore) -> StorageBytes32 {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        StorageBytes32::from_array(bytes)
    }

    fn random_address(rng: &mut impl RngCore) -> B160 {
        let mut bytes = [0u8; 20];
        rng.fill_bytes(&mut bytes);
        B160::from_be_bytes(bytes)
    }

    fn build_response<const RANDOMIZED: bool>() -> (ZksGetProofResponse, [u8; 32], [u8; 32]) {
        let mut rng = StdRng::seed_from_u64(0x5eedd00d);
        let address = random_address(&mut rng);

        let mut leaves_map: BTreeMap<StorageBytes32, StorageBytes32> = BTreeMap::new();
        let mut existing_keys = Vec::new_in(Global);
        while existing_keys.len() < 4 {
            let key = random_bytes32(&mut rng);
            let value = random_bytes32(&mut rng);
            let flat_key = derive_flat_storage_key(&address, &key);
            if leaves_map.insert(flat_key, value).is_none() {
                existing_keys.push(key);
            }
        }

        // Add some extra random accounts/slots to the tree.
        for _ in 0..6 {
            let other_address = random_address(&mut rng);
            for _ in 0..2 {
                let key = random_bytes32(&mut rng);
                let value = random_bytes32(&mut rng);
                let flat_key = derive_flat_storage_key(&other_address, &key);
                leaves_map.entry(flat_key).or_insert(value);
            }
        }

        let mut leaves = Vec::new_in(Global);
        for (key, value) in leaves_map.iter() {
            leaves.push((*key, *value));
        }

        let tree = TestingTree::<RANDOMIZED>::new_in_with_leaves(Global, leaves);

        let mut missing_key = random_bytes32(&mut rng);
        let mut missing_flat_key = derive_flat_storage_key(&address, &missing_key);
        while leaves_map.contains_key(&missing_flat_key) {
            missing_key = random_bytes32(&mut rng);
            missing_flat_key = derive_flat_storage_key(&address, &missing_key);
        }

        let keys_to_prove = vec![existing_keys[0], missing_key];
        let preimage = tree.state_commitment_preimage(42, random_bytes32(&mut rng), 1_700_000_000);
        let response = tree.prove_zks_get_proof(address, &keys_to_prove, preimage);

        let mut verifier_hasher = Blake2sGetProofHasher::new();
        let batch_hash = compute_state_commitment(
            &mut verifier_hasher,
            &to_proof_bytes32(*tree.root()),
            &response.state_commitment_preimage,
        );

        let existing_flat_key = derive_flat_storage_key(&address, &existing_keys[0]);
        let expected_existing_value =
            to_proof_bytes32(leaves_map.get(&existing_flat_key).copied().unwrap());

        (response, batch_hash, expected_existing_value)
    }

    #[test]
    fn zks_get_proof_roundtrip_deterministic_positions() {
        let (response, batch_hash, expected_existing_value) = build_response::<false>();
        let mut verifier_hasher = Blake2sGetProofHasher::new();
        let values = response
            .verify_with::<TREE_HEIGHT, _>(&batch_hash, &mut verifier_hasher)
            .expect("proof must verify");
        assert_eq!(values[0], expected_existing_value);
        assert_eq!(values[1], ZERO_32_BYTES);
    }

    #[test]
    fn zks_get_proof_roundtrip_random_positions() {
        let (response, batch_hash, expected_existing_value) = build_response::<true>();
        let mut verifier_hasher = Blake2sGetProofHasher::new();
        let values = response
            .verify_with::<TREE_HEIGHT, _>(&batch_hash, &mut verifier_hasher)
            .expect("proof must verify");
        assert_eq!(values[0], expected_existing_value);
        assert_eq!(values[1], ZERO_32_BYTES);
    }

    #[test]
    fn zks_get_proof_json_roundtrip() {
        let (response, _batch_hash, _expected_existing_value) = build_response::<false>();
        let encoded = serde_json::to_vec(&response).expect("response json serialization failed");
        let decoded: ZksGetProofResponse =
            serde_json::from_slice(&encoded).expect("response json deserialization failed");
        assert!(response == decoded);
    }
}
