use crate::H256;
use alloy_sol_types::private::U256;
use std::collections::BTreeMap;
use airbender::crypto::{MiniDigest, blake2s::Blake2s256};

struct TreeNode<'a> {
    hash: H256,
    path: &'a [H256],
}

pub struct BalanceTree {
    balances: BTreeMap<H256, Balance>,
    size: u32,
}

pub struct Balance {
    index: u32,
    balance: U256,
    path: Vec<H256>,
}

impl BalanceTree {
    pub fn new(size: u32) -> Self {
        Self {
            balances: BTreeMap::new(),
            size,
        }
    }

    fn height(&self) -> usize {
        (u32::BITS - (self.size - 1).leading_zeros()) as usize
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
    ) -> H256 {
        let mut hash = if index >= self.size {
            assert_eq!(index, self.size);
            assert_eq!(balance, [0; 32]);
            self.size += 1;
            Self::hash([0; 32], [0; 32])
        } else {
            Self::hash(asset_id, balance)
        };
        let mut parity = index;

        // TODO assert path length? should be always prev_height?
        // no need, if it's incorrect then there will be root mismatch
        for sibling in &path {
            if parity % 2 == 0 {
                hash = Self::hash(hash, *sibling);
            } else {
                hash = Self::hash(*sibling, hash);
            }
            parity >>= 1;
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

        hash
    }

    pub fn update_balance(&mut self, asset_id: H256, amount: U256, add: bool) {
        let balance = &mut self.balances
            .get_mut(&asset_id).expect("asset id missing").balance;
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
