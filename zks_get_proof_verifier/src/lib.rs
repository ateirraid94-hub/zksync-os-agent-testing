#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use ruint::aliases::B160;

pub const ZERO_32_BYTES: [u8; 32] = [0u8; 32];
pub const MAX_32_BYTES: [u8; 32] = [0xffu8; 32];

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
    pub last256_block_hashes_blake: [u8; 32],
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
    pub last_block_timestamp: u64,
}

/// Response envelope for `zks_getProof`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Clone, PartialEq, Eq)]
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
    pub key: [u8; 32],
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
    pub leaf_key: [u8; 32],
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::bytes32"))]
    pub value: [u8; 32],
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
    pub next_index: u64,
    #[cfg_attr(feature = "serde", serde(with = "serde_hex::vec_bytes32"))]
    pub siblings: Vec<[u8; 32]>,
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
        value: [u8; 32],
        #[cfg_attr(feature = "serde", serde(rename = "nextIndex"))]
        #[cfg_attr(feature = "serde", serde(with = "serde_hex::u64"))]
        next_index: u64,
        #[cfg_attr(feature = "serde", serde(with = "serde_hex::vec_bytes32"))]
        siblings: Vec<[u8; 32]>,
    },
    NonExisting {
        #[cfg_attr(feature = "serde", serde(rename = "leftNeighbor"))]
        left_neighbor: LeafWithProof,
        #[cfg_attr(feature = "serde", serde(rename = "rightNeighbor"))]
        right_neighbor: LeafWithProof,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZksGetProofVerificationError {
    SiblingsTooLong { len: usize },
    NonExistingRootMismatch,
    NeighborOrderInvalid,
    NeighborLinkInvalid,
    StateCommitmentMismatch,
}

/// Minimal digest interface used by verifier internals.
pub trait ZksGetProofHasher {
    fn update(&mut self, input: impl AsRef<[u8]>);
    fn finalize_reset(&mut self) -> [u8; 32];
}

pub use verifier::{compute_state_commitment, verify_response};

impl ZksGetProofResponse {
    pub fn verify_with<const N: usize, H: ZksGetProofHasher>(
        &self,
        expected_batch_hash: &[u8; 32],
        hasher: &mut H,
    ) -> Result<Vec<[u8; 32]>, ZksGetProofVerificationError> {
        verifier::verify_response::<N, H>(self, expected_batch_hash, hasher)
    }
}

/// Verifier utilities and error types.
pub mod verifier {
    use alloc::vec::Vec;

    use ruint::aliases::B160;

    use super::{
        StateCommitmentPreimage, StorageProof, StorageProofType, ZksGetProofHasher,
        ZksGetProofResponse, ZksGetProofVerificationError, ZERO_32_BYTES,
    };

    #[derive(Clone, Copy, Debug)]
    struct FlatStorageLeaf {
        key: [u8; 32],
        value: [u8; 32],
        next_index: u64,
    }

    fn derive_flat_storage_key<H: ZksGetProofHasher>(
        address: &B160,
        key: &[u8; 32],
        hasher: &mut H,
    ) -> [u8; 32] {
        hasher.update([0u8; 12]);
        hasher.update(address.to_be_bytes::<{ B160::BYTES }>());
        hasher.update(key);
        hasher.finalize_reset()
    }

    fn hash_leaf<H: ZksGetProofHasher>(hasher: &mut H, leaf: &FlatStorageLeaf) -> [u8; 32] {
        hasher.update(leaf.key);
        hasher.update(leaf.value);
        hasher.update(leaf.next_index.to_le_bytes());
        hasher.finalize_reset()
    }

    fn hash_node<H: ZksGetProofHasher>(
        hasher: &mut H,
        left_node: &[u8; 32],
        right_node: &[u8; 32],
    ) -> [u8; 32] {
        hasher.update(left_node);
        hasher.update(right_node);
        hasher.finalize_reset()
    }

    /// Computes the L1 batch commitment from a state root and its preimage.
    /// Should replicate the logic in zk_ee/src/common_structs/chain_state_commitment.rs
    pub fn compute_state_commitment<H: ZksGetProofHasher>(
        hasher: &mut H,
        state_root: &[u8; 32],
        preimage: &StateCommitmentPreimage,
    ) -> [u8; 32] {
        hasher.update(state_root);
        hasher.update(preimage.next_free_slot.to_be_bytes());
        hasher.update(preimage.block_number.to_be_bytes());
        hasher.update(preimage.last256_block_hashes_blake);
        hasher.update(preimage.last_block_timestamp.to_be_bytes());
        hasher.finalize_reset()
    }

    /// Output of verifying a single proof.
    /// Note that we include the computed state root, to avoid recomputing
    /// the state commitment for every proof.
    struct SingleVerificationResult {
        computed_root: [u8; 32],
        value: [u8; 32],
    }

    /// Verify a single proof, returning the leaf value and recomputed
    /// state root.
    /// Note: caller is expected to check that the recomputed root is
    /// consistent with the expected one.
    fn verify_single_proof<const N: usize, H: ZksGetProofHasher>(
        address: &B160,
        proof: &StorageProof,
        empty_hashes: &[[u8; 32]; N],
        hasher: &mut H,
    ) -> Result<SingleVerificationResult, ZksGetProofVerificationError> {
        let flat_key = derive_flat_storage_key(address, &proof.key, hasher);
        let (state_root, value) = match &proof.proof {
            StorageProofType::Existing {
                index,
                value,
                next_index,
                siblings,
            } => {
                let leaf = FlatStorageLeaf {
                    key: flat_key,
                    value: *value,
                    next_index: *next_index,
                };
                let root = compute_root_from_siblings::<N, H>(
                    hasher,
                    *index,
                    &leaf,
                    siblings,
                    empty_hashes,
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
                let left_leaf = FlatStorageLeaf {
                    key: left_neighbor.leaf_key,
                    value: left_neighbor.value,
                    next_index: left_neighbor.next_index,
                };
                let right_leaf = FlatStorageLeaf {
                    key: right_neighbor.leaf_key,
                    value: right_neighbor.value,
                    next_index: right_neighbor.next_index,
                };
                let left_root = compute_root_from_siblings::<N, H>(
                    hasher,
                    left_neighbor.index,
                    &left_leaf,
                    &left_neighbor.siblings,
                    empty_hashes,
                )?;
                let right_root = compute_root_from_siblings::<N, H>(
                    hasher,
                    right_neighbor.index,
                    &right_leaf,
                    &right_neighbor.siblings,
                    empty_hashes,
                )?;
                if left_root != right_root {
                    return Err(ZksGetProofVerificationError::NonExistingRootMismatch);
                }
                (left_root, ZERO_32_BYTES)
            }
        };

        Ok(SingleVerificationResult {
            computed_root: state_root,
            value,
        })
    }

    /// Verifies all storage proofs against the expected batch hash.
    pub fn verify_response<const N: usize, H: ZksGetProofHasher>(
        response: &ZksGetProofResponse,
        expected_batch_hash: &[u8; 32],
        hasher: &mut H,
    ) -> Result<Vec<[u8; 32]>, ZksGetProofVerificationError> {
        // Handle case for 0 proofs:
        if response.storage_proofs.is_empty() {
            return Ok(alloc::vec![]);
        }

        let empty_hashes = compute_empty_hashes::<N, H>(hasher);
        let mut values = Vec::with_capacity(response.storage_proofs.len());

        // Handle first proof (must exist due to previous check)
        let SingleVerificationResult {
            computed_root: first_proof_computed_root,
            value,
        } = verify_single_proof::<N, H>(
            &response.address,
            &response.storage_proofs[0],
            &empty_hashes,
            hasher,
        )?;

        // For the first proof, we recompute state commitment and check against expected batch hash
        let commitment = compute_state_commitment(
            hasher,
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
            } = verify_single_proof::<N, H>(&response.address, proof, &empty_hashes, hasher)?;
            if computed_root != first_proof_computed_root {
                return Err(ZksGetProofVerificationError::StateCommitmentMismatch);
            }
            values.push(value);
        }

        Ok(values)
    }

    fn compute_root_from_siblings<const N: usize, H: ZksGetProofHasher>(
        hasher: &mut H,
        index: u64,
        leaf: &FlatStorageLeaf,
        siblings: &[[u8; 32]],
        empty_hashes: &[[u8; 32]; N],
    ) -> Result<[u8; 32], ZksGetProofVerificationError> {
        if siblings.len() > N {
            return Err(ZksGetProofVerificationError::SiblingsTooLong {
                len: siblings.len(),
            });
        }

        let mut path = [ZERO_32_BYTES; N];
        path[..siblings.len()].copy_from_slice(siblings);
        path[siblings.len()..N].copy_from_slice(&empty_hashes[siblings.len()..N]);

        Ok(recompute_root_from_leaf_and_path(
            hasher, index, leaf, &path,
        ))
    }

    fn recompute_root_from_leaf_and_path<const N: usize, H: ZksGetProofHasher>(
        hasher: &mut H,
        index: u64,
        leaf: &FlatStorageLeaf,
        path: &[[u8; 32]; N],
    ) -> [u8; 32] {
        let leaf_hash = hash_leaf(hasher, leaf);

        let mut current = leaf_hash;
        let mut index = index;
        for path in path.iter() {
            let path: &[u8; 32] = path;
            let (left, right) = if index & 1 == 0 {
                // current is left
                (&current, path)
            } else {
                (path, &current)
            };
            let next = hash_node(hasher, left, right);
            current = next;
            index >>= 1;
        }
        assert!(index == 0);

        current
    }

    fn compute_empty_hashes<const N: usize, H: ZksGetProofHasher>(hasher: &mut H) -> [[u8; 32]; N] {
        let mut result = [ZERO_32_BYTES; N];
        let empty_leaf = FlatStorageLeaf {
            key: ZERO_32_BYTES,
            value: ZERO_32_BYTES,
            next_index: 0,
        };
        let empty_leaf_hash = hash_leaf(hasher, &empty_leaf);
        result[0] = empty_leaf_hash;
        let mut previous = empty_leaf_hash;
        for i in 0..(N - 1) {
            let node_hash = hash_node(hasher, &previous, &previous);
            result[i + 1] = node_hash;
            previous = node_hash;
        }

        result
    }
}

#[cfg(feature = "serde")]
mod serde_hex {
    use alloc::string::String;
    use alloc::vec::Vec;

    use ruint::aliases::B160;
    use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

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

        pub fn serialize<S>(value: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&encode_hex(value))
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
        where
            D: Deserializer<'de>,
        {
            let bytes = decode_hex_str(deserializer, 32)?;
            let mut array = [0u8; 32];
            array.copy_from_slice(&bytes);
            Ok(array)
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
            serializer.serialize_str(&alloc::format!("0x{value:x}"))
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

        pub fn serialize<S>(value: &[[u8; 32]], serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let encoded: Vec<String> = value.iter().map(|item| encode_hex(item)).collect();
            encoded.serialize(serializer)
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<[u8; 32]>, D::Error>
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
                out.push(array);
            }
            Ok(out)
        }
    }
}
