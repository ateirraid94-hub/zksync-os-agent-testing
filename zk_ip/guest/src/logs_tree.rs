use airbender::crypto::{blake2s::Blake2s256, sha3::Keccak256, MiniDigest};
use crate::H256;

pub struct LogsTree {
    next_index: u32,
    zeros: Vec<H256>,
    sides: Vec<H256>,
}

impl LogsTree {
    pub fn new() -> Self {
        Self {
            next_index: 0,
            // TODO: use constant
            zeros: vec![Keccak256::digest([0; 88])],
            sides: vec![[0; 32]],
        }
    }

    pub fn push(&mut self, leaf: H256) -> H256 {
        // let leaf = hash_log(&log);
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

    pub fn root(&self) -> H256 {
        self.sides[self.sides.len() - 1]
    }

    pub fn height(&self) -> usize {
        self.sides.len() - 1
    }
}
