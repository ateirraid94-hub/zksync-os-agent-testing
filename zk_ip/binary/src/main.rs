#![allow(incomplete_features)]
#![feature(allocator_api)]
#![feature(generic_const_exprs)]
#![feature(pointer_is_aligned_to)]
#![feature(unsafe_cell_access)]
#![feature(slice_ptr_get)]
#![feature(str_from_raw_parts)]
#![no_main]

pub mod allocator;
pub mod glue;
pub mod logger;
pub mod quasi_uart;

use riscv_common::csr_read_word;
use riscv_common::zksync_os_finish_success;
use std::collections::BTreeMap;
use crypto::{MiniDigest, blake2s::Blake2s256, sha3::Keccak256};

#[global_allocator]
static GLOBAL_ALLOC: allocator::OptionalGlobalAllocator = allocator::OptionalGlobalAllocator;

#[repr(C)]
struct Balance {
    asset_id: H256,
    index: u32,
    balance: H256,
    path: Vec<H256>,
}

#[repr(C)]
struct TreeNode {
    hash: H256,
    path: Vec<H256>,
}

#[repr(C)]
struct DynamicMerkleTree {
    next_index: u32,
    zeros: Vec<H256>,
    sides: Vec<H256>,
}

impl DynamicMerkleTree {
    fn new() -> Self {
        Self {
            next_index: 0,
            // TODO: use constant
            zeros: vec![Keccak256::digest([0; 88])],
            sides: vec![[0; 32]]
        }
    }

    fn push(&mut self, log: &L2Log) -> H256 {
        let leaf = hash_log(&log);
        let mut levels = self.zeros.len() - 1;
        let mut current_index = self.next_index;
        self.next_index += 1;
        if current_index == 1 << levels {
            let zero = self.zeros[levels];
            let new_zero = Keccak256::digest([zero, zero].concat());
            self.zeros.push(new_zero);
            self.sides.push([0; 32]);
            levels += 1;
        }
        let mut current_level_hash = leaf;
        for i in 0..levels {
            let is_left = current_index % 2 == 0;

            let (left, right) = if is_left {
                self.sides[i] = current_level_hash;
                (current_level_hash, self.zeros[i])
            } else {
                (self.sides[i], current_level_hash)
            };
            current_level_hash = Keccak256::digest([left, right].concat());
            current_index >>= 1;
        }
        self.sides[levels] = current_level_hash;
        current_level_hash
    }

    fn root(&self) -> H256 {
        self.sides[self.sides.len() - 1]
    }

    fn height(&self) -> usize {
        self.sides.len() - 1
    }
}

#[repr(C)]
struct L2Log {
    tx_number_in_batch: u16,
    sender: [u8; 20],
    key: H256,
    value: H256,
    message: Vec<u8>,
}

type H256 = [u8; 32];

fn hash_log(log: &L2Log) -> H256 {
    // TODO use constant for length
    let mut buffer = [0u8; 1+1+2+20+32+32];
    buffer[0] = 0;
    buffer[1] = 1;
    buffer[2..4].copy_from_slice(&log.tx_number_in_batch.to_be_bytes());
    buffer[4..24].copy_from_slice(&log.sender);
    buffer[24..56].copy_from_slice(log.key.as_ref());
    buffer[56..88].copy_from_slice(log.value.as_ref());
    Keccak256::digest(&buffer)
}

fn u32_array_to_u8_array<const N: usize>(input: [u32; N]) -> [u8; N * 4] {
    std::array::from_fn(|i| input[i / 4].to_be_bytes()[i % 4])
}

fn u8_array_to_u32_array(input: H256) -> [u32; 8] {
    std::array::from_fn(|i| {
        u32::from_be_bytes([
            input[i * 4],
            input[i * 4 + 1],
            input[i * 4 + 2],
            input[i * 4 + 3],
        ])
    })
}

fn blake_hash_parts(left: H256, right: H256) -> H256 {
    Blake2s256::digest([left, right].concat())
}

fn read_h256() -> H256 {
    u32_array_to_u8_array::<8>(core::array::from_fn(|i| csr_read_word()))
}

fn read_h160() -> [u8; 20] {
    u32_array_to_u8_array::<5>(core::array::from_fn(|i| csr_read_word()))
}

fn tree_height(tree_size: u32) -> usize {
    (u32::BITS - (tree_size - 1).leading_zeros()) as usize
}

unsafe fn workload() -> ! {
    crate::allocator::init_allocator(
        riscv_common::boot_sequence::heap_start(),
        riscv_common::boot_sequence::heap_end(),
    );

    let prev_root = read_h256();
    let prev_tree_size = csr_read_word(); // assume there is < 4billion tokens and > 0
    let prev_tree_height = tree_height(prev_tree_size);

    let mut tree_size = prev_tree_size;

    let mut tree_layer = BTreeMap::new(); // sparse tree layer: index -> hash

    let mut balances = BTreeMap::new(); // index -> balance diff

    let n = csr_read_word(); // number of existing tokens
    for _i in 0..n {
        let asset_id = read_h256();
        let index = csr_read_word();
        let prev_balance = read_h256();

        let mut parity = index;
        let mut hash = blake_hash_parts(asset_id, prev_balance);
        let mut path = Vec::with_capacity(prev_tree_height);
        for _j in 0..prev_tree_height {
            let sibling = read_h256();
            path.push(sibling);
            if parity % 2 == 0 {
                hash = blake_hash_parts(hash, sibling);
            } else {
                hash = blake_hash_parts(sibling, hash);
            }
            parity >>= 1;
        }
        assert_eq!(hash, prev_root, "prev_root mismatch");

        assert!(!balances.contains_key(&index));
        // TODO: assert asset_id is unique
        balances.insert(
            index,
            Balance {
                asset_id,
                index,
                balance: prev_balance,
                path,
            },
        );
    }

    let n = csr_read_word(); // number of new tokens (that weren't in the tree)
    for _i in 0..n {
        let asset_id = read_h256();
        let index = tree_size;
        tree_size += 1;
        let tree_height = tree_height(tree_size);
        // TODO: this can be optimized
        let mut path = Vec::with_capacity(tree_height);
        for _j in 0..tree_height {
            path.push(read_h256());
        }
        balances.insert(
            index,
            Balance {
                asset_id,
                index,
                balance: [0; 32],
                path,
            },
        );
    }


    let mut tree = DynamicMerkleTree::new();
    let n = csr_read_word(); // number of logs to parse
    for _i in 0..n {
        let tx_number_in_batch = csr_read_word();
        let sender = read_h160();
        let key = read_h256();
        let value = read_h256();
        // TODO do this better
        // TODO not every log has a message
        let message_len = csr_read_word() as usize;
        let mut message = Vec::with_capacity(message_len);
        message.resize_with(message_len, || csr_read_word() as u8);

        assert!(tx_number_in_batch < u16::MAX as u32);
        assert_eq!(Keccak256::digest(&message), value);

        let log = L2Log {
            tx_number_in_batch: tx_number_in_batch as u16,
            sender,
            key,
            value,
            message
        };

        tree.push(&log);

        // TODO: parse logs and update balance for each token
    }

    let l2_logs_root = tree.root();

    for (index, balance) in balances.into_iter() {
        tree_layer.insert(
            index,
            TreeNode {
                hash: blake_hash_parts(balance.asset_id, balance.balance),
                path: balance.path,
            },
        );
    }

    let tree_height = tree_height(tree_size);
    let mut new_layer = BTreeMap::new();
    for i in 0..tree_height {
        for (index, node) in tree_layer.iter() {
            let left;
            let right;
            if index % 2 == 0 {
                left = node.hash;
                right = tree_layer
                    .get(&(index + 1))
                    .map(|n| n.hash)
                    .unwrap_or(node.path[i]);
            } else {
                if tree_layer.contains_key(&(index - 1)) {
                    // we've already computed this node
                    continue;
                } else {
                    left = node.path[i];
                    right = node.hash;
                }
            }
            let hash = blake_hash_parts(left, right);
            // TODO: cloning path is suboptimal
            new_layer.insert(
                *index << 1,
                TreeNode {
                    hash,
                    path: node.path.clone(),
                },
            );
        }
        (tree_layer, new_layer) = (new_layer, BTreeMap::new());
    }

    let final_hash =
        Blake2s256::digest([tree_layer[&0].hash, prev_root, l2_logs_root].concat());
    zksync_os_finish_success(&u8_array_to_u32_array(final_hash));
}

#[inline(never)]
fn main() -> ! {
    riscv_common::boot_sequence::init();

    unsafe { workload() }
}

#[link_section = ".init.rust"]
#[export_name = "_start_rust"]
unsafe extern "C" fn start_rust() -> ! {
    main()
}
