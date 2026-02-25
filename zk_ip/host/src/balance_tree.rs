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
            prev_root,
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
                        .unwrap_or_else(|| node.path[0]);
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
                        path: &node.path.get(1..).unwrap_or(&[]),
                    },
                );
            }
            (layer, new_layer) = (new_layer, BTreeMap::new());
        }

        layer[&0].hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(left: H256, right: H256) -> H256 {
        BalanceTree::hash(left, right)
    }

    #[test]
    fn test_new_tree() {
        let prev_root = [1u8; 32];
        let tree = BalanceTree::new(4, prev_root);

        assert_eq!(tree.size, 4);
        assert_eq!(tree.prev_size, 4);
        assert_eq!(tree.prev_root, prev_root);
        assert!(tree.balances.is_empty());
    }

    #[test]
    fn test_height() {
        assert_eq!(BalanceTree::new(0, [0; 32]).height(), 0);
        assert_eq!(BalanceTree::new(1, [0; 32]).height(), 0);
        assert_eq!(BalanceTree::new(2, [0; 32]).height(), 1);
        assert_eq!(BalanceTree::new(3, [0; 32]).height(), 2);
        assert_eq!(BalanceTree::new(4, [0; 32]).height(), 2);
        assert_eq!(BalanceTree::new(5, [0; 32]).height(), 3);
        assert_eq!(BalanceTree::new(8, [0; 32]).height(), 3);
        assert_eq!(BalanceTree::new(9, [0; 32]).height(), 4);
        assert_eq!(BalanceTree::new(16, [0; 32]).height(), 4);
    }

    #[test]
    fn test_hash_deterministic() {
        let left = [1u8; 32];
        let right = [2u8; 32];

        let hash1 = hash(left, right);
        let hash2 = hash(left, right);

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, [0u8; 32]);
    }

    #[test]
    fn test_hash_order_matters() {
        let left = [1u8; 32];
        let right = [2u8; 32];

        let hash1 = hash(left, right);
        let hash2 = hash(right, left);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_insert_and_root_single_leaf() {
        let asset_id = [1u8; 32];
        let balance = [0u8; 32];

        let leaf_hash = hash(asset_id, balance);
        let root = leaf_hash;

        let mut tree = BalanceTree::new(1, root);
        tree.insert_token_info(asset_id, 0, balance, vec![]);

        assert_eq!(tree.root(), root);
    }

    #[test]
    fn test_insert_and_root_two_leaves() {
        let asset_id_0 = [1u8; 32];
        let balance_0 = [0u8; 32];
        let asset_id_1 = [2u8; 32];
        let balance_1 = [0u8; 32];

        let leaf_0 = hash(asset_id_0, balance_0);
        let leaf_1 = hash(asset_id_1, balance_1);
        let root = hash(leaf_0, leaf_1);

        let mut tree = BalanceTree::new(2, root);
        tree.insert_token_info(asset_id_0, 0, balance_0, vec![leaf_1]);
        tree.insert_token_info(asset_id_1, 1, balance_1, vec![leaf_0]);

        assert_eq!(tree.root(), root);
    }

    #[test]
    fn test_insert_and_root_four_leaves() {
        let asset_ids: [H256; 4] = [[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]];
        let balances: [H256; 4] = [[10u8; 32], [20u8; 32], [30u8; 32], [40u8; 32]];

        let leaves: Vec<H256> = asset_ids
            .iter()
            .zip(balances.iter())
            .map(|(a, b)| hash(*a, *b))
            .collect();

        let left_subtree = hash(leaves[0], leaves[1]);
        let right_subtree = hash(leaves[2], leaves[3]);
        let root = hash(left_subtree, right_subtree);

        let mut tree = BalanceTree::new(4, root);

        tree.insert_token_info(asset_ids[0], 0, balances[0], vec![leaves[1], right_subtree]);
        tree.insert_token_info(asset_ids[1], 1, balances[1], vec![leaves[0], right_subtree]);
        tree.insert_token_info(asset_ids[2], 2, balances[2], vec![leaves[3], left_subtree]);
        tree.insert_token_info(asset_ids[3], 3, balances[3], vec![leaves[2], left_subtree]);

        assert_eq!(tree.root(), root);
    }

    #[test]
    fn test_update_balance_add() {
        let asset_id = [1u8; 32];
        let balance = [0u8; 32];

        let leaf_hash = hash(asset_id, balance);

        let mut tree = BalanceTree::new(1, leaf_hash);
        tree.insert_token_info(asset_id, 0, balance, vec![]);

        tree.update_balance(asset_id, U256::from(100), true);

        let new_balance = U256::from(100).to_be_bytes();
        let expected_root = hash(asset_id, new_balance);
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_update_balance_subtract() {
        let asset_id = [1u8; 32];
        let initial_balance = U256::from(100).to_be_bytes();

        let leaf_hash = hash(asset_id, initial_balance);

        let mut tree = BalanceTree::new(1, leaf_hash);
        tree.insert_token_info(asset_id, 0, initial_balance, vec![]);

        tree.update_balance(asset_id, U256::from(30), false);

        let new_balance = U256::from(70).to_be_bytes();
        let expected_root = hash(asset_id, new_balance);
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_update_balance_multiple_operations() {
        let asset_id = [1u8; 32];
        let balance = [0u8; 32];

        let leaf_hash = hash(asset_id, balance);

        let mut tree = BalanceTree::new(1, leaf_hash);
        tree.insert_token_info(asset_id, 0, balance, vec![]);

        tree.update_balance(asset_id, U256::from(100), true);
        tree.update_balance(asset_id, U256::from(50), true);
        tree.update_balance(asset_id, U256::from(30), false);

        let expected_balance = U256::from(120).to_be_bytes();
        let expected_root = hash(asset_id, expected_balance);
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    #[should_panic(expected = "asset id missing")]
    fn test_update_balance_missing_asset() {
        let mut tree = BalanceTree::new(0, [0; 32]);
        tree.update_balance([1u8; 32], U256::from(100), true);
    }

    #[test]
    #[should_panic]
    fn test_update_balance_underflow() {
        let asset_id = [1u8; 32];
        let initial_balance = U256::from(50).to_be_bytes();

        let leaf_hash = hash(asset_id, initial_balance);

        let mut tree = BalanceTree::new(1, leaf_hash);
        tree.insert_token_info(asset_id, 0, initial_balance, vec![]);

        tree.update_balance(asset_id, U256::from(100), false);
    }

    #[test]
    #[should_panic]
    fn test_insert_duplicate_asset() {
        let asset_id = [1u8; 32];
        let balance = [0u8; 32];

        let leaf_hash = hash(asset_id, balance);
        let sibling = hash([2u8; 32], [0u8; 32]);
        let root = hash(leaf_hash, sibling);

        let mut tree = BalanceTree::new(2, root);
        tree.insert_token_info(asset_id, 0, balance, vec![sibling]);
        tree.insert_token_info(asset_id, 0, balance, vec![sibling]);
    }

    #[test]
    #[should_panic(expected = "root mismatch")]
    fn test_insert_invalid_proof() {
        let asset_id = [1u8; 32];
        let balance = [0u8; 32];

        let root = [99u8; 32];

        let mut tree = BalanceTree::new(1, root);
        tree.insert_token_info(asset_id, 0, balance, vec![]);
    }

    #[test]
    fn test_insert_new_token_grows_tree() {
        // Start with a tree of size 1
        let asset_id_0 = [1u8; 32];
        let balance_0 = [10u8; 32];
        let leaf_0 = hash(asset_id_0, balance_0);

        let mut tree = BalanceTree::new(1, leaf_0);
        tree.insert_token_info(asset_id_0, 0, balance_0, vec![]);

        assert_eq!(tree.size, 1);

        // Now add a new token at index 1 (grows tree to size 2)
        let new_asset_id = [2u8; 32];
        tree.insert_token_info(new_asset_id, 1, [0; 32], vec![leaf_0]);

        assert_eq!(tree.size, 2);
    }

    #[test]
    fn test_insert_new_token_at_boundary() {
        let asset_id_0 = [1u8; 32];
        let balance_0 = [10u8; 32];

        let leaf_0 = hash(asset_id_0, balance_0);

        let mut tree = BalanceTree::new(1, leaf_0);
        tree.insert_token_info(asset_id_0, 0, balance_0, vec![]);

        let new_asset_id = [2u8; 32];
        tree.insert_token_info(new_asset_id, 1, [0; 32], vec![leaf_0]);

        assert_eq!(tree.size, 2);

        let new_leaf = hash(new_asset_id, [0; 32]);
        let expected_root = hash(leaf_0, new_leaf);
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_root_with_partial_tree() {
        let asset_id_0 = [1u8; 32];
        let balance_0 = [10u8; 32];
        let asset_id_2 = [3u8; 32];
        let balance_2 = [30u8; 32];

        let leaf_0 = hash(asset_id_0, balance_0);
        let leaf_1 = hash([99u8; 32], [99u8; 32]);
        let leaf_2 = hash(asset_id_2, balance_2);
        let leaf_3 = hash([98u8; 32], [98u8; 32]);

        let left_subtree = hash(leaf_0, leaf_1);
        let right_subtree = hash(leaf_2, leaf_3);
        let root = hash(left_subtree, right_subtree);

        let mut tree = BalanceTree::new(4, root);

        tree.insert_token_info(asset_id_0, 0, balance_0, vec![leaf_1, right_subtree]);
        tree.insert_token_info(asset_id_2, 2, balance_2, vec![leaf_3, left_subtree]);

        assert_eq!(tree.root(), root);
    }

    #[test]
    fn test_root_changes_after_balance_update() {
        let asset_id = [1u8; 32];
        let balance = [0u8; 32];

        let leaf_hash = hash(asset_id, balance);

        let mut tree = BalanceTree::new(1, leaf_hash);
        tree.insert_token_info(asset_id, 0, balance, vec![]);

        let root_before = tree.root();

        tree.update_balance(asset_id, U256::from(100), true);

        let root_after = tree.root();

        assert_ne!(root_before, root_after);
    }
}
