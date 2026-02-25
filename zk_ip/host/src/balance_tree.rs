use crate::H256;
use airbender_crypto::{blake2s::Blake2s256, MiniDigest};
use alloy_primitives::U256;
use std::collections::BTreeMap;

struct TreeNode<'a> {
    hash: H256,
    path: &'a [H256],
}

pub struct BalanceTree {
    balances: BTreeMap<H256, Balance>,
    size: u32,
    prev_size: u32,
    pub prev_root: H256,
}

struct Balance {
    index: u32,
    balance: U256,
    path: Vec<H256>,
}

impl BalanceTree {
    pub fn new(size: u32, prev_root: H256) -> Self {
        Self {
            balances: BTreeMap::new(),
            size,
            prev_size: size,
            prev_root
        }
    }

    fn height(&self) -> usize {
        if self.size == 0 {
            0
        } else {
            (u32::BITS - (self.size - 1).leading_zeros()) as usize
        }
    }

    fn hash(left: H256, right: H256) -> H256 {
        Blake2s256::digest([left, right].concat())
    }

    pub fn insert_token_info(
        &mut self,
        asset_id: H256,
        index: u32,
        balance: H256,
        path: Vec<H256>,
    ) {
        let mut hash = if index >= self.size {
            assert_eq!(index, self.size);
            assert_eq!(balance, [0; 32]);
            self.size += 1;
            // TODO replace with constant
            Self::hash([0; 32], [0; 32])
        } else {
            Self::hash(asset_id, balance)
        };

        // If this is a newly added token that fell into the newly grown part of the tree,
        // we don't need to verify its inclusion into the old tree.
        if index < self.prev_size.next_power_of_two() {
            let mut parity = index;
            for sibling in &path {
                if parity % 2 == 0 {
                    hash = Self::hash(hash, *sibling);
                } else {
                    hash = Self::hash(*sibling, hash);
                }
                parity >>= 1;
            }
            assert_eq!(hash, self.prev_root, "root mismatch on index {}", index);
        }

        assert!(!self.balances.contains_key(&asset_id));
        self.balances.insert(
            asset_id,
            Balance {
                index,
                balance: U256::from_be_bytes(balance),
                path,
            },
        );
    }

    pub fn update_balance(&mut self, asset_id: H256, amount: U256, add: bool) {
        let balance = &mut self
            .balances
            .get_mut(&asset_id)
            .expect("asset id missing")
            .balance;
        if add {
            *balance += amount;
        } else {
            assert!(*balance >= amount);
            *balance -= amount;
        }
    }

    fn leaf_layer(&self) -> BTreeMap<u32, TreeNode<'_>> {
        let mut layer = BTreeMap::new();
        for (asset_id, balance) in &self.balances {
            assert!(!layer.contains_key(&balance.index));
            layer.insert(
                balance.index,
                TreeNode {
                    hash: Self::hash(*asset_id, balance.balance.to_be_bytes()),
                    path: &balance.path,
                },
            );
        }
        layer
    }

    pub fn root(&self) -> H256 {
        let height = self.height();
        let mut layer = self.leaf_layer();
        let mut new_layer = BTreeMap::new();

        for _i in 0..height {
            for (index, node) in layer.iter() {
                let left;
                let right;
                let sibling_index = index ^ 1;
                if index % 2 == 0 {
                    left = node.hash;
                    right = layer
                        .get(&sibling_index)
                        .map(|node| node.hash)
                        .unwrap_or(node.path[0]);
                } else {
                    if layer.contains_key(&sibling_index) {
                        // Since we're iterating in ascending order,
                        // we've already computed this node
                        continue;
                    } else {
                        left = node.path[0];
                        right = node.hash;
                    }
                }
                let hash = Self::hash(left, right);
                new_layer.insert(
                    *index >> 1,
                    TreeNode {
                        hash,
                        path: &node.path[1..],
                    },
                );
            }
            (layer, new_layer) = (new_layer, BTreeMap::new());
        }

        layer[&0].hash
    }
}
