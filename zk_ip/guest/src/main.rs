#![no_main]

use std::collections::BTreeMap;

use airbender::crypto::{blake2s::Blake2s256, sha3::Keccak256, MiniDigest};
use alloy_sol_types::{SolType, private::primitives::U256, sol_data::{Address, Uint, Bytes}};

use merkle_tree::DynamicMerkleTree;
use utils::*;

mod merkle_tree;
mod utils;
mod sparse_merkle_tree;

type H256 = [u8; 32];

// struct Balance {
//     index: u32,
//     balance: U256,
//     path: Option<Vec<H256>>,
// }
//
// struct TreeNode<'a> {
//     hash: H256,
//     path: &'a [H256],
// }

struct L2Log {
    tx_number_in_batch: u16,
    sender: [u8; 20], // TODO this potentially can be just u16 since it's all system contract addresses
    key: H256,
    value: H256,
}

impl L2Log {
    fn hash(&self) -> H256 {
        let mut buffer = [0u8; 1 + 1 + 2 + 20 + 32 + 32];
        buffer[0] = 0; // shard_id = rollup
        buffer[1] = 1; // is_service = true
        buffer[2..4].copy_from_slice(&self.tx_number_in_batch.to_be_bytes());
        buffer[4..24].copy_from_slice(&self.sender);
        buffer[24..56].copy_from_slice(&self.key);
        buffer[56..88].copy_from_slice(&self.value);
        Keccak256::digest(&buffer)
    }
}

fn get_asset_id(chain_id: H256, asset_data: H256) -> H256 {
    Keccak256::digest([chain_id, L2_NATIVE_TOKEN_VAULT.into_word().0, asset_data].concat())
}

macro_rules! read {
    ($msg:literal) => {
        airbender::guest::read().expect($msg)
    };
}

#[airbender::main]
fn main() -> [u32; 8] {
    let prev_root: H256 = read().expect("prev root");
    // TODO: if size is public input then we don't have to verify
    // that leafs of new tokens had hash[0, 0] previously
    let prev_tree_size: u32 = read().expect("prev tree size"); // assume there is < 4billion tokens and > 0
    let prev_tree_height = next_power_of_two_log(prev_tree_size);
    let base_token_asset_id: H256 = read().expect("base token asset id"); // TODO - do we include it in public commitment?

    let mut tree_size = prev_tree_size;

    // TODO read in zeros and sides
    let mut balance_tree = sparse_merkle_tree::BalanceTree::new(prev_tree_size, vec![], vec![]);
    // TODO: prev_root is in the sides[]

    let n: u32 = read().expect("number of existing tokens"); // number of existing tokens
    for _i in 0..n {
        let asset_id: H256 = read().expect("asset id");
        let index: u32 = read().expect("index");
        let prev_balance: H256 = read().expect("prev balance");
        let mut path = read!("path");

        balance_tree.insert_token_info(asset_id, index, prev_balance, path);

        //
        // let mut parity = index;
        // let mut hash = blake_hash_parts(asset_id, prev_balance);
        // for _j in 0..prev_tree_height {
        //     let sibling: H256 = read().expect("sibling");
        //     path.push(sibling);
        //     if parity % 2 == 0 {
        //         hash = blake_hash_parts(hash, sibling);
        //     } else {
        //         hash = blake_hash_parts(sibling, hash);
        //     }
        //     parity >>= 1;
        // }
        // assert_eq!(hash, prev_root, "prev_root mismatch");
        //
        // assert!(!balances.contains_key(&asset_id));
        // balances.insert(
        //     asset_id,
        //     Balance {
        //         index,
        //         balance: U256::from_be_bytes(prev_balance),
        //         path: Some(path),
        //     },
        // );
    }

    // let n: u32 = read().expect("number of new tokens"); // number of new tokens (that weren't in the tree)
    // tree_size += n;

    // for _i in 0..n {
    //     let asset_id: H256 = read().expect("asset id of new token");
    //     let index = tree_size;
    //     tree_size += 1;
    //     let tree_height = next_power_of_two_log(tree_size);
    //     // TODO: this can be optimized
    //     let mut path = Vec::with_capacity(tree_height);
    //     for _j in 0..tree_height {
    //         let sibling: H256 = read().expect("sibling of new token");
    //         path.push(sibling);
    //     }
    //     balances.insert(
    //         asset_id,
    //         Balance {
    //             index,
    //             balance: U256::ZERO,
    //             path,
    //         },
    //     );
    // }

    let mut tree = DynamicMerkleTree::new();
    let n: u32 = read().expect("number of logs"); // number of logs to parse
    for _i in 0..n {
        let tx_number_in_batch: u16 = read().expect("tx number in batch");
        let sender: [u8; 20] = read().expect("log sender");
        let key: H256 = read().expect("log key");
        let value: H256 = read().expect("log value");


        let log = L2Log {
            tx_number_in_batch: tx_number_in_batch as u16,
            sender,
            key,
            value,
        };

        tree.push(log.hash());

        if sender == L2_TO_L1_MESSENGER {
            let message: Vec<u8> = read().expect("log message");
            assert_eq!(Keccak256::digest(&message), value);
            if key == L2_ASSET_ROUTER.into_word() {

                assert!(message.len() >= 68);
                let selector = &message[..4];
                let asset_id: H256 = message[36..68].try_into().unwrap();
                let transfer_data = &message[68..];

                // finalizeDeposit.selector
                assert_eq!(selector, b"\x9c\x88\x4f\xd1");

                type Tuple = (Address, Address, Address, Uint<256>, Bytes);
                let (_, _, original_token, amount, erc20_metadata) =
                    Tuple::abi_decode_sequence_validate(transfer_data).expect("decoding failed");

                let token_original_chain_id = if erc20_metadata[0] == 0 {
                    [0; 32]
                } else if erc20_metadata[0] == 1 {
                    erc20_metadata[1..33].try_into().unwrap()
                } else {
                    panic!("invalid erc20 metadata version")
                };

                let asset_data = original_token.into_word();
                assert_eq!(asset_id, get_asset_id(token_original_chain_id, asset_data.0));

                balances.get_mut(&asset_id).expect("asset id not found").balance -= amount;
            } else if key == L2_BASE_TOKEN.into_word() {
                let selector = &message[..4];
                let amount: H256 = message[24..56].try_into().unwrap();
                let amount = U256::from_be_bytes(amount);

                // finalizeEthWithdrawal.selector
                assert_eq!(selector, b"\x6c\x09\x60\xf9");
                balances.get_mut(&base_token_asset_id).expect("asset id not found").balance -= amount;
            }
        } else if sender == L2_BOOTLOADER {
        }
    }

    let l2_logs_root = tree.root();

    let mut layer = BTreeMap::new();
    let mut new_layer = BTreeMap::new();
    let tree_height = next_power_of_two_log(tree_size);

    for (asset_id, balance) in balances.iter() {
        assert!(!layer.contains_key(&balance.index));
        layer.insert(
            balance.index,
            TreeNode {
                hash: blake_hash_parts(*asset_id, balance.balance.to_be_bytes()),
                path: &balance.path,
            },
        );
    }

    for i in 0..tree_height {
        for (index, node) in layer.iter() {
            let left;
            let right;
            if index % 2 == 0 {
                left = node.hash;
                right = layer
                    .get(&(index + 1))
                    .map(|n| n.hash)
                    .unwrap_or(node.path[i]);
            } else {
                if layer.contains_key(&(index - 1)) {
                    // we've already computed this node
                    continue;
                } else {
                    left = node.path[i];
                    right = node.hash;
                }
            }
            let hash = blake_hash_parts(left, right);
            new_layer.insert(
                *index >> 1,
                TreeNode {
                    hash,
                    path: node.path,
                },
            );
        }
        (layer, new_layer) = (new_layer, BTreeMap::new());
    }

    let commitment = Blake2s256::digest([layer[&0].hash, prev_root, /*l2_logs_root*/].concat());
    std::array::from_fn(|i| u32::from_be_bytes(commitment[i * 4..(i + 1) * 4].try_into().unwrap()))
}
