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
    prev_size: u32,
    size: u32,
    zeros: Vec<H256>,
    sides: Vec<H256>,
    first_new_token_path: Vec<H256>,
}

pub struct Balance {
    index: u32,
    balance: U256,
    path: Option<Vec<H256>>,
}

impl BalanceTree {
    pub fn new(size: u32, zeros: Vec<H256>, sides: Vec<H256>) -> Self {
        Self {
            balances: BTreeMap::new(),
            prev_size: size,
            size,
            zeros,
            sides,
            first_new_token_path: vec![],
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
        let mut hash = Self::hash(asset_id, balance);
        let mut parity = index;

        // TODO assert path length
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
                path: Some(path),
            },
        );

        hash
    }

    pub fn update_balance(&mut self, asset_id: H256, amount: U256, add: bool) {
        self.balances
            .entry(asset_id)
            .and_modify(|e| {
                if add {
                    e.balance += amount;
                } else {
                    assert!(e.balance >= amount);
                    e.balance -= amount;
                }
            })
            .or_insert_with(|| {
                let index = self.size;
                self.size += 1;
                assert!(add);
                Balance {
                    index,
                    balance: amount,
                    path: None,
                }
            });
    }

    fn leaf_layer(&mut self) -> BTreeMap<u32, TreeNode<'_>> {
        if self.size > self.prev_size {
            // new tokens were inserted - construct the path for the first new token
            let mut parity = self.prev_size;
            let height = self.height();
            for i in 0..height {
                let sibling = if parity % 2 == 0 {
                    self.zeros[i]
                } else {
                    self.sides[i]
                };
                self.first_new_token_path.push(sibling);
                parity >>= 1;
            }
        }

        let mut layer = BTreeMap::new();
        for (asset_id, balance) in &self.balances {
            assert!(!layer.contains_key(&balance.index));

            layer.insert(
                balance.index,
                TreeNode {
                    hash: Self::hash(*asset_id, balance.balance.to_be_bytes()),
                    path: match balance.path.as_ref() {
                        Some(path) => path,
                        None => {
                            if balance.index == self.prev_size {
                                // path has been constructed for the first new token
                                &self.first_new_token_path
                            } else {
                                // since new tokens' indices are consecutive,
                                // they can have only zeros[]
                                // since they will never access path when index is odd
                                &self.zeros
                            }
                        }
                    },
                },
            );
        }

        layer
    }

    fn root(&mut self) -> H256 {
        let height = self.height();
        let mut layer = self.leaf_layer();
        let mut new_layer = BTreeMap::new();

        for _i in 0..height {
            for (index, node) in layer.iter() {
                let left;
                let right;
                if index % 2 == 0 {
                    left = node.hash;
                    right = layer
                        .get(&(index + 1))
                        .map(|n| n.hash)
                        .unwrap_or(node.path[0]);
                } else {
                    if layer.contains_key(&(index - 1)) {
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
