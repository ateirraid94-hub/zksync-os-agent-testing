use crate::system_functions::keccak256::keccak256_native_cost_u64;
use crypto::MiniDigest;
use zk_ee::{common_structs::interop_root::InteropRoot, utils::Bytes32};

/// Calculates a rolling keccak256 hash over a sequence of interop roots.
/// This creates a cumulative digest that can be verified on settlement layers.
///
/// For each root: rolling_hash = keccak256(old_rolling_hash || chain_id || block_number || root_hash)
pub fn calculate_interop_roots_rolling_hash(
    old_rolling_hash: Bytes32,
    roots: &[InteropRoot],
    hasher: &mut crypto::sha3::Keccak256,
) -> Bytes32 {
    let mut data = [0u8; 96];

    let mut rolling_hash = old_rolling_hash;
    for root in roots {
        data[0..32].copy_from_slice(&rolling_hash.as_u8_ref());
        data[56..64].copy_from_slice(&root.chain_id.to_be_bytes());
        data[88..96].copy_from_slice(&root.block_or_batch_number.to_be_bytes());
        hasher.update(data);

        // Note: now we have only one side
        hasher.update(root.root.as_u8_ref());

        rolling_hash = hasher.finalize_reset().into()
    }

    rolling_hash
}

/// Calculates native computational cost for hashing interop roots.
/// Used for gas estimation and resource tracking.
pub fn native_resource_cost_of_hashing_interop_roots(roots: &[InteropRoot]) -> u64 {
    // old_hash + chain_id + block_number = 96 bytes
    // 1 side = 32 bytes
    let len = 96 + 32;
    let cost_per_root = keccak256_native_cost_u64(len);

    cost_per_root * roots.len() as u64
}
