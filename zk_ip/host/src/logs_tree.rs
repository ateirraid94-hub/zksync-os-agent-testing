use crate::H256;
use airbender_crypto::{sha3::Keccak256, MiniDigest};

/// A dynamic incremental Merkle tree.
/// This tree uses Keccak256 hashing and grows dynamically as leaves are added.
/// The tree maintains zero hashes for each level and
/// rightmost left nodes (sides) for each level.
pub struct LogsTree {
    next_index: u32,
    zeros: Vec<H256>,
    sides: Vec<H256>,
}

impl LogsTree {
    pub fn new() -> Self {
        Self {
            next_index: 0,
            zeros: vec![crate::constants::EMPTY_LOG_HASH],
            sides: vec![[0; 32]],
        }
    }

    /// Adds a leaf at the next available index and recalculates the Merkle root.
    /// The tree will automatically grow to accommodate more
    /// leaves when necessary (when the index reaches a power of 2).
    pub fn push(&mut self, leaf: H256) -> H256 {
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

    /// Returns the current Merkle root of the tree.
    pub fn root(&self) -> H256 {
        self.sides[self.sides.len() - 1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tree() {
        let tree = LogsTree::new();
        assert_eq!(tree.next_index, 0);
        assert_eq!(tree.zeros.len(), 1);
        assert_eq!(tree.sides.len(), 1);
        assert_eq!(tree.root(), [0; 32]);
    }

    #[test]
    fn test_push_single_leaf() {
        let mut tree = LogsTree::new();
        let leaf = [1u8; 32];

        let root = tree.push(leaf);
        assert_eq!(tree.next_index, 1);
        assert_eq!(tree.root(), root);
        assert_eq!(tree.root(), leaf);
        assert_eq!(tree.zeros.len(), 1);
    }

    #[test]
    fn test_push_multiple_leaves() {
        let mut tree = LogsTree::new();
        let leaf1 = [1u8; 32];
        let leaf2 = [2u8; 32];
        let leaf3 = [3u8; 32];

        let root1 = tree.push(leaf1);
        assert_eq!(tree.next_index, 1);

        let root2 = tree.push(leaf2);
        assert_eq!(tree.next_index, 2);
        assert_eq!(tree.root(), root2);
        assert_ne!(root1, root2);

        let expected_root = Keccak256::digest([leaf1, leaf2].concat());
        assert_eq!(root2, expected_root);

        let root3 = tree.push(leaf3);
        assert_eq!(tree.next_index, 3);
        assert_eq!(tree.root(), root3);
        assert_ne!(root2, root3);

        let expected_root = {
            let subroot = Keccak256::digest([leaf3, crate::constants::EMPTY_LOG_HASH].concat());
            Keccak256::digest([expected_root, subroot].concat())
        };
        assert_eq!(root3, expected_root);
    }

    #[test]
    fn test_tree_growth() {
        let mut tree = LogsTree::new();
        let initial_levels = tree.zeros.len();

        // Push leaves until the tree needs to grow
        // Tree grows when next_index reaches 2^levels
        let leaf = [1u8; 32];
        tree.push(leaf);
        assert_eq!(tree.zeros.len(), initial_levels);

        tree.push(leaf);

        // After 2 pushes, tree should have grown
        assert_eq!(tree.zeros.len(), initial_levels + 1);
        assert_eq!(tree.sides.len(), initial_levels + 1);

        // One more push
        tree.push(leaf);
        assert_eq!(tree.zeros.len(), initial_levels + 2);
        assert_eq!(tree.sides.len(), initial_levels + 2);
    }

    #[test]
    fn test_tree_growth_multiple_levels() {
        let mut tree = LogsTree::new();

        // Push enough leaves to trigger multiple level expansions
        for i in 0..5 {
            tree.push([i; 32]);
        }

        assert_eq!(tree.next_index, 5);
        // Should have grown to accommodate 5 leaves
        assert_eq!(tree.zeros.len(), 4); // ceil(log2(5)) + 1
    }

    #[test]
    fn test_side_updates() {
        let mut tree = LogsTree::new();
        let leaf = [1u8; 32];

        tree.push(leaf);
        tree.push(leaf);
        tree.push(leaf);
        let sides1 = tree.sides.clone();

        tree.push(leaf);
        let sides2 = tree.sides.clone();

        // Exactly one side should be different
        assert_eq!(1, sides1.iter().zip(&sides2).filter(|(x, y)| x != y).count());
    }
}
