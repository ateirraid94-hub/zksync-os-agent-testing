use std::alloc::Allocator;

use zk_ee::common_structs::interop_root::InteropRoot;

/// Limitation to prevent overflows
const MAX_ENCODABLE_AMOUNT_OF_ROOTS: u32 = 10000;

/// Encodes calldata for a call to the L2InteropRootStorage system contract.
///
/// Function signature:
/// `function addInteropRootsInBatch(InteropRoot[] calldata interopRootsInput) external`
///
/// InteropRoot struct definition:
/// ```solidity
/// struct InteropRoot {
///     uint256 chainId;
///     uint256 blockOrBatchNumber;
///     bytes32[] sides;
/// }
/// ```
pub fn encode_interop_roots_setting_batch_call<A: Allocator>(
    interop_roots: &[InteropRoot],
    alloc: A,
) -> alloc::vec::Vec<u8, A> {
    // ABI encoding layout:
    // [0x00-0x04] Function selector (4 bytes)
    // [0x04-0x24] Array offset (32 bytes, points to 0x20)
    // [0x24-0x44] Array length (32 bytes)
    // [0x44-...] Root offsets (32 bytes per root)
    // [...] Root data structs:
    //   - chainId (32 bytes)
    //   - blockOrBatchNumber (32 bytes)
    //   - sides array offset (32 bytes, relative to struct start)
    //   - sides array length (32 bytes, currently always 1)
    //   - sides array data (32 bytes per side)

    // Calculate total data size needed:
    // - Function selector: 4 bytes
    // - Array offset + length: 64 bytes
    // - Root offset entries: 32 bytes per root
    // - Root struct data: 128 bytes per root (chainId + blockOrBatchNumber + sides_offset + sides_length + sides_data)
    let roots_count = interop_roots.len() as u32;

    assert!(roots_count <= MAX_ENCODABLE_AMOUNT_OF_ROOTS);

    let mut data_size = 4 + 32 + 32; // selector + array offset + array length

    // Root offset entries in the array
    data_size += roots_count * 32;

    // Root struct data: chainId + blockOrBatchNumber + sides_offset + sides_length + sides_data
    data_size += roots_count * (32 + 32 + 32 + 32 + 32);

    let mut data = alloc::vec::Vec::with_capacity_in(data_size as usize, alloc);
    data.resize(data_size as usize, 0u8);

    // Function selector for addInteropRootsInBatch(InteropRoot[] calldata interopRootsInput)
    let function_selector = [0xCC, 0xA2, 0xF7, 0xBC];
    data[0..4].copy_from_slice(&function_selector);

    // Array offset points to where array data starts (0x20 = 32 bytes after function selector)
    let array_offset = 32u64;
    data[28..36].copy_from_slice(&array_offset.to_be_bytes());

    // Array length (number of InteropRoot structs)
    data[60..68].copy_from_slice(&(roots_count as u64).to_be_bytes());

    // Position for writing root offset entries
    let mut current_pos = 68;

    // Encode each InteropRoot struct
    for (root_number, root) in interop_roots.iter().enumerate() {
        // Calculate offset to where this struct's data will be written
        let mut struct_offset = interop_roots.len() * 32 + root_number * (32 * 5);
        data[current_pos + 24..current_pos + 32]
            .copy_from_slice(&(struct_offset as u64).to_be_bytes());
        current_pos += 32;

        // Adjust offset to account for the header (selector + array offset + array length)
        struct_offset += 68;

        // Write struct fields:

        // chainId (32 bytes, right-aligned)
        data[struct_offset + 24..struct_offset + 32].copy_from_slice(&root.chain_id.to_be_bytes());
        struct_offset += 32;

        // blockOrBatchNumber (32 bytes, right-aligned)
        data[struct_offset + 24..struct_offset + 32]
            .copy_from_slice(&root.block_or_batch_number.to_be_bytes());
        struct_offset += 32;

        // Offset to sides array (relative to struct start, points to 96 bytes ahead)
        data[struct_offset + 28..struct_offset + 32].copy_from_slice(&96_u32.to_be_bytes());
        struct_offset += 32;

        // Sides array length (currently always 1 element)
        data[struct_offset + 28..struct_offset + 32].copy_from_slice(&1u32.to_be_bytes());
        struct_offset += 32;

        // Sides array data (the root hash as bytes32)
        data[struct_offset..struct_offset + 32].copy_from_slice(&root.root.as_u8_ref());
    }

    data
}

#[cfg(test)]
mod tests {
    use std::alloc::Global;

    use alloy::{primitives::U256, sol, sol_types::SolCall};

    use zk_ee::{common_structs::interop_root::InteropRoot, utils::Bytes32};

    use super::encode_interop_roots_setting_batch_call;

    // Define the Solidity types for comparison
    sol! {
        struct InteropRootSol {
            uint256 chainId;
            uint256 blockOrBatchNumber;
            bytes32[] sides;
        }

        function addInteropRootsInBatch(InteropRootSol[] calldata interopRootsInput) external;
    }

    #[test]
    fn test_check_encoding_single_root() {
        let root = InteropRoot {
            chain_id: 1,
            block_or_batch_number: 100,
            root: Bytes32::from([0x42; 32]),
        };

        let our_encoded = encode_interop_roots_setting_batch_call(&[root], Global);

        // Create equivalent Alloy structs
        let alloy_root = InteropRootSol {
            chainId: U256::from(1),
            blockOrBatchNumber: U256::from(100),
            sides: vec![[0x42; 32].into()],
        };

        // Encode with Alloy
        let alloy_encoded = addInteropRootsInBatchCall {
            interopRootsInput: vec![alloy_root],
        }
        .abi_encode();

        assert_eq!(our_encoded, alloy_encoded);
    }

    #[test]
    fn test_check_encoding_multiple_roots() {
        let roots = vec![
            InteropRoot {
                chain_id: 1,
                block_or_batch_number: 100,
                root: Bytes32::from([0x42; 32]),
            },
            InteropRoot {
                chain_id: 2,
                block_or_batch_number: 200,
                root: Bytes32::from([0x84; 32]),
            },
            InteropRoot {
                chain_id: 3,
                block_or_batch_number: 300,
                root: Bytes32::from([0x84; 32]),
            },
        ];

        let our_encoded = encode_interop_roots_setting_batch_call(&roots, Global);

        // Create equivalent Alloy structs
        let alloy_roots = vec![
            InteropRootSol {
                chainId: U256::from(1),
                blockOrBatchNumber: U256::from(100),
                sides: vec![[0x42; 32].into()],
            },
            InteropRootSol {
                chainId: U256::from(2),
                blockOrBatchNumber: U256::from(200),
                sides: vec![[0x84; 32].into()],
            },
            InteropRootSol {
                chainId: U256::from(3),
                blockOrBatchNumber: U256::from(300),
                sides: vec![[0x84; 32].into()],
            },
        ];

        let alloy_encoded = addInteropRootsInBatchCall {
            interopRootsInput: alloy_roots,
        }
        .abi_encode();

        assert_eq!(our_encoded, alloy_encoded);
    }
}
