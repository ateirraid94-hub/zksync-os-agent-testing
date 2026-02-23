//! Helpers for the `zks_getProof` API.
//!
//! The module defines JSON-shaped proof structs and provides separate prover and verifier helpers.
//! The prover is only a lightweight testing utility backed by `FlatStorageBacking`.
//! The verifier is spec-accurate and can be used by external consumers.

use alloc::vec::Vec;

use ruint::aliases::B160;
use zk_ee::utils::Bytes32;

use super::TREE_HEIGHT;

pub use verifier::{compute_state_commitment, ZksGetProofVerificationError};

/// Preimage data required to recompute the L1 batch commitment.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateCommitmentPreimage {
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
    pub next_free_slot: u64,
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
    pub block_number: u64,
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::bytes32"))]
    pub last256_block_hashes_blake: Bytes32,
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
    pub last_block_timestamp: u64,
}

/// Response envelope for `zks_getProof`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZksGetProofResponse {
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::b160"))]
    pub address: B160,
    pub state_commitment_preimage: StateCommitmentPreimage,
    pub storage_proofs: Vec<StorageProof>,
}

/// A proof for a single storage key.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageProof {
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::bytes32"))]
    pub key: Bytes32,
    pub proof: StorageProofType,
}

/// A leaf and its Merkle path, used to prove non-existence.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeafWithProof {
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
    pub index: u64,
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::bytes32"))]
    pub leaf_key: Bytes32,
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::bytes32"))]
    pub value: Bytes32,
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
    pub next_index: u64,
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::vec_bytes32"))]
    pub siblings: Vec<Bytes32>,
}

/// Storage proof variants following the `zks_getProof` JSON schema.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(tag = "type", rename_all_fields = "camelCase")
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageProofType {
    Existing {
        #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
        index: u64,
        #[cfg_attr(feature = "serde", serde(with = "serde_hex::bytes32"))]
        value: Bytes32,
        #[cfg_attr(feature = "serde", serde(rename = "nextIndex"))]
        #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
        next_index: u64,
        #[cfg_attr(feature = "serde", serde(with = "serde_hex::vec_bytes32"))]
        siblings: Vec<Bytes32>,
    },
    NonExisting {
        #[cfg_attr(feature = "serde", serde(rename = "leftNeighbor"))]
        left_neighbor: LeafWithProof,
        #[cfg_attr(feature = "serde", serde(rename = "rightNeighbor"))]
        right_neighbor: LeafWithProof,
    },
}

#[cfg(feature = "serde")]
mod serde_hex {
    use alloc::string::String;
    use alloc::vec::Vec;

    use ruint::aliases::B160;
    use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
    use zk_ee::utils::Bytes32;

    fn encode_hex(bytes: &[u8]) -> String {
        let mut out = String::from("0x");
        out.push_str(&const_hex::encode(bytes));
        out
    }

    fn decode_hex_str<'de, D>(deserializer: D, expected_len: usize) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let s = raw.strip_prefix("0x").unwrap_or(&raw);
        let bytes = const_hex::decode(s).map_err(D::Error::custom)?;
        if bytes.len() != expected_len {
            return Err(D::Error::custom("invalid hex length"));
        }
        Ok(bytes)
    }

    pub mod bytes32 {
        use super::*;

        pub fn serialize<S>(value: &Bytes32, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&encode_hex(value.as_u8_array_ref()))
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<Bytes32, D::Error>
        where
            D: Deserializer<'de>,
        {
            let bytes = decode_hex_str(deserializer, 32)?;
            let mut array = [0u8; 32];
            array.copy_from_slice(&bytes);
            Ok(Bytes32::from_array(array))
        }
    }

    pub mod b160 {
        use super::*;

        pub fn serialize<S>(value: &B160, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&encode_hex(&value.to_be_bytes::<{ B160::BYTES }>()))
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<B160, D::Error>
        where
            D: Deserializer<'de>,
        {
            let bytes = decode_hex_str(deserializer, 20)?;
            let mut array = [0u8; 20];
            array.copy_from_slice(&bytes);
            Ok(B160::from_be_bytes(array))
        }
    }

    pub mod u64 {
        use super::*;

        pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&format!("0x{:x}", value))
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
        where
            D: Deserializer<'de>,
        {
            let raw = String::deserialize(deserializer)?;
            let s = raw.strip_prefix("0x").unwrap_or(&raw);
            u64::from_str_radix(s, 16).map_err(D::Error::custom)
        }
    }

    pub mod vec_bytes32 {
        use super::*;

        pub fn serialize<S>(value: &[Bytes32], serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let encoded: Vec<String> = value
                .iter()
                .map(|item| encode_hex(item.as_u8_array_ref()))
                .collect();
            encoded.serialize(serializer)
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Bytes32>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let raw: Vec<String> = Vec::deserialize(deserializer)?;
            let mut out = Vec::with_capacity(raw.len());
            for item in raw {
                let s = item.strip_prefix("0x").unwrap_or(&item);
                let bytes = const_hex::decode(s).map_err(D::Error::custom)?;
                if bytes.len() != 32 {
                    return Err(D::Error::custom("invalid hex length"));
                }
                let mut array = [0u8; 32];
                array.copy_from_slice(&bytes);
                out.push(Bytes32::from_array(array));
            }
            Ok(out)
        }
    }
}

impl ZksGetProofResponse {
    /// Verifies the proof against a batch hash using the default tree height.
    pub fn verify(
        &self,
        expected_batch_hash: &Bytes32,
    ) -> Result<Vec<Bytes32>, ZksGetProofVerificationError> {
        verifier::verify_response::<TREE_HEIGHT>(self, expected_batch_hash)
    }
}

/// Verifier utilities and error types.
pub mod verifier {
    use alloc::alloc::Global;
    use alloc::vec::Vec;
    use ruint::aliases::B160;

    use crate::system_implementation::flat_storage_model::simple_growable_storage::FlatStorageHasher;
    use crate::system_implementation::flat_storage_model::StorageProof;
    use crypto::MiniDigest;
    use zk_ee::common_structs::{derive_flat_storage_key_with_hasher, ChainStateCommitment};
    use zk_ee::utils::Bytes32;

    use super::super::{
        compute_empty_hashes, recompute_root_from_leaf_and_path, Blake2sStorageHasher,
        FlatStorageLeaf,
    };
    use super::{StateCommitmentPreimage, StorageProofType, ZksGetProofResponse};

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum ZksGetProofVerificationError {
        SiblingsTooLong { len: usize },
        NonExistingRootMismatch,
        NeighborOrderInvalid,
        NeighborLinkInvalid,
        StateCommitmentMismatch,
    }

    /// Computes the L1 batch commitment from a state root and its preimage.
    pub fn compute_state_commitment(
        state_root: &Bytes32,
        preimage: &StateCommitmentPreimage,
    ) -> Bytes32 {
        let commitment = ChainStateCommitment {
            state_root: *state_root,
            next_free_slot: preimage.next_free_slot,
            block_number: preimage.block_number,
            last_256_block_hashes_blake: preimage.last256_block_hashes_blake,
            last_block_timestamp: preimage.last_block_timestamp,
        };
        Bytes32::from_array(commitment.hash())
    }

    /// Output of verifying a single proof.
    /// Note that we include the computed state root, to avoid recomputing
    /// the state commitment for every proof.
    struct SingleVerificationResult {
        computed_root: Bytes32,
        value: Bytes32,
    }

    /// Verify a single proof, returning the leaf value and recomputed
    /// state root.
    /// Note: caller is expected to check that the recomputed root is
    /// consistent with the expected one.
    fn verify_single_proof<const N: usize>(
        address: &B160,
        proof: &StorageProof,
        empty_hashes: &[Bytes32; N],
        key_hasher: &mut crypto::blake2s::Blake2s256,
    ) -> Result<SingleVerificationResult, ZksGetProofVerificationError> {
        let flat_key = derive_flat_storage_key_with_hasher(&address, &proof.key, key_hasher);
        let (state_root, value) = match &proof.proof {
            StorageProofType::Existing {
                index,
                value,
                next_index,
                siblings,
            } => {
                let leaf = FlatStorageLeaf::<N> {
                    key: flat_key,
                    value: *value,
                    next: *next_index,
                };
                let mut hasher = Blake2sStorageHasher::new();
                let root = compute_root_from_siblings::<N>(
                    &mut hasher,
                    *index,
                    &leaf,
                    siblings,
                    &empty_hashes,
                )?;
                (root, *value)
            }
            StorageProofType::NonExisting {
                left_neighbor,
                right_neighbor,
            } => {
                if !(left_neighbor.leaf_key < flat_key && flat_key < right_neighbor.leaf_key) {
                    return Err(ZksGetProofVerificationError::NeighborOrderInvalid);
                }
                if left_neighbor.next_index != right_neighbor.index {
                    return Err(ZksGetProofVerificationError::NeighborLinkInvalid);
                }
                let mut hasher = Blake2sStorageHasher::new();
                let left_leaf = FlatStorageLeaf::<N> {
                    key: left_neighbor.leaf_key,
                    value: left_neighbor.value,
                    next: left_neighbor.next_index,
                };
                let right_leaf = FlatStorageLeaf::<N> {
                    key: right_neighbor.leaf_key,
                    value: right_neighbor.value,
                    next: right_neighbor.next_index,
                };
                let left_root = compute_root_from_siblings::<N>(
                    &mut hasher,
                    left_neighbor.index,
                    &left_leaf,
                    &left_neighbor.siblings,
                    &empty_hashes,
                )?;
                let right_root = compute_root_from_siblings::<N>(
                    &mut hasher,
                    right_neighbor.index,
                    &right_leaf,
                    &right_neighbor.siblings,
                    &empty_hashes,
                )?;
                if left_root != right_root {
                    return Err(ZksGetProofVerificationError::NonExistingRootMismatch);
                }
                (left_root, Bytes32::ZERO)
            }
        };

        Ok(SingleVerificationResult {
            computed_root: state_root,
            value,
        })
    }

    /// Verifies all storage proofs against the expected batch hash.
    pub fn verify_response<const N: usize>(
        response: &ZksGetProofResponse,
        expected_batch_hash: &Bytes32,
    ) -> Result<Vec<Bytes32>, ZksGetProofVerificationError> {
        // Handle case for 0 proofs:
        if response.storage_proofs.is_empty() {
            return Ok(vec![]);
        }

        let mut empty_hasher = Blake2sStorageHasher::new();
        let empty_hashes =
            compute_empty_hashes::<N, Blake2sStorageHasher, Global>(&mut empty_hasher, Global);

        let mut values = Vec::with_capacity(response.storage_proofs.len());
        let mut key_hasher = crypto::blake2s::Blake2s256::new();

        // Handle first proof (must exist due to previous check)
        let SingleVerificationResult {
            computed_root: first_proof_computed_root,
            value,
        } = verify_single_proof(
            &response.address,
            &response.storage_proofs[0],
            &empty_hashes,
            &mut key_hasher,
        )?;

        // For the first proof, we recompute state commitment and check against expected batch hash
        let commitment = compute_state_commitment(
            &first_proof_computed_root,
            &response.state_commitment_preimage,
        );
        if &commitment != expected_batch_hash {
            return Err(ZksGetProofVerificationError::StateCommitmentMismatch);
        }
        values.push(value);

        // Now, verify all remaining proofs by checking against the
        // root computed for the first one
        for proof in response.storage_proofs.iter().skip(1) {
            let SingleVerificationResult {
                computed_root,
                value,
            } = verify_single_proof(&response.address, proof, &empty_hashes, &mut key_hasher)?;
            if computed_root != first_proof_computed_root {
                return Err(ZksGetProofVerificationError::StateCommitmentMismatch);
            }
            values.push(value);
        }

        Ok(values)
    }

    fn compute_root_from_siblings<const N: usize>(
        hasher: &mut Blake2sStorageHasher,
        index: u64,
        leaf: &FlatStorageLeaf<N>,
        siblings: &[Bytes32],
        empty_hashes: &[Bytes32; N],
    ) -> Result<Bytes32, ZksGetProofVerificationError> {
        if siblings.len() > N {
            return Err(ZksGetProofVerificationError::SiblingsTooLong {
                len: siblings.len(),
            });
        }

        let mut path = [Bytes32::ZERO; N];
        for i in 0..siblings.len() {
            path[i] = siblings[i];
        }
        for i in siblings.len()..N {
            path[i] = empty_hashes[i];
        }

        Ok(recompute_root_from_leaf_and_path(
            hasher, index, leaf, &path,
        ))
    }
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
    use zk_ee::utils::Bytes32;

    use super::super::{compute_empty_hashes, Blake2sStorageHasher, FlatStorageBacking};
    use super::{
        LeafWithProof, StateCommitmentPreimage, StorageProof, StorageProofType, ZksGetProofResponse,
    };

    fn compress_siblings<const N: usize>(
        path: &[Bytes32; N],
        empty_hashes: &[Bytes32; N],
    ) -> Vec<Bytes32> {
        let mut last_non_empty: Option<usize> = None;
        for i in (0..N).rev() {
            if path[i] != empty_hashes[i] {
                last_non_empty = Some(i);
                break;
            }
        }
        let len = last_non_empty.map(|idx| idx + 1).unwrap_or(0);
        path[..len].to_vec()
    }

    fn leaf_with_proof<const N: usize>(
        proof: &super::super::LeafProof<N, Blake2sStorageHasher, impl Allocator>,
        empty_hashes: &[Bytes32; N],
    ) -> LeafWithProof {
        let path: &[Bytes32; N] = &*proof.path;
        LeafWithProof {
            index: proof.index,
            leaf_key: proof.leaf.key,
            value: proof.leaf.value,
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
            last256_block_hashes_blake: Bytes32,
            last_block_timestamp: u64,
        ) -> StateCommitmentPreimage {
            StateCommitmentPreimage {
                next_free_slot: self.next_free_slot,
                block_number,
                last256_block_hashes_blake,
                last_block_timestamp,
            }
        }

        /// Produces a `zks_getProof`-style response for the requested keys.
        pub fn prove_zks_get_proof(
            &self,
            address: B160,
            keys: &[Bytes32],
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
                        let path: &[Bytes32; N] = &*proof.existing.path;
                        StorageProofType::Existing {
                            index: proof.existing.index,
                            value: proof.existing.leaf.value,
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

                storage_proofs.push(StorageProof { key: *key, proof });
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
    use rand::rngs::StdRng;
    use rand::{RngCore, SeedableRng};
    use zk_ee::common_structs::derive_flat_storage_key;

    use super::super::TestingTree;

    fn random_bytes32(rng: &mut impl RngCore) -> Bytes32 {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        Bytes32::from_array(bytes)
    }

    fn random_address(rng: &mut impl RngCore) -> B160 {
        let mut bytes = [0u8; 20];
        rng.fill_bytes(&mut bytes);
        B160::from_be_bytes(bytes)
    }

    fn build_response<const RANDOMIZED: bool>() -> (ZksGetProofResponse, Bytes32, Bytes32) {
        let mut rng = StdRng::seed_from_u64(0x5eedd00d);
        let address = random_address(&mut rng);

        let mut leaves_map: BTreeMap<Bytes32, Bytes32> = BTreeMap::new();
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
        let batch_hash = compute_state_commitment(tree.root(), &response.state_commitment_preimage);
        let existing_flat_key = derive_flat_storage_key(&address, &existing_keys[0]);
        let expected_existing_value = leaves_map.get(&existing_flat_key).copied().unwrap();

        (response, batch_hash, expected_existing_value)
    }

    #[test]
    fn zks_get_proof_roundtrip_deterministic_positions() {
        let (response, batch_hash, expected_existing_value) = build_response::<false>();
        let values = response.verify(&batch_hash).expect("proof must verify");
        assert_eq!(values[0], expected_existing_value);
        assert_eq!(values[1], Bytes32::ZERO);
    }

    #[test]
    fn zks_get_proof_roundtrip_random_positions() {
        let (response, batch_hash, expected_existing_value) = build_response::<true>();
        let values = response.verify(&batch_hash).expect("proof must verify");
        assert_eq!(values[0], expected_existing_value);
        assert_eq!(values[1], Bytes32::ZERO);
    }

    #[test]
    fn zks_get_proof_json_roundtrip() {
        let (response, _batch_hash, _expected_existing_value) = build_response::<false>();
        let encoded = serde_json::to_vec(&response).expect("response json serialization failed");
        let decoded: ZksGetProofResponse =
            serde_json::from_slice(&encoded).expect("response json deserialization failed");
        assert_eq!(response, decoded);
    }
}
