extern crate alloc;
use alloc::collections::BTreeMap;
use riscv_common::{csr_read_word, csr_write_word};

#[derive(Default)]
pub struct StorageLayer {
    cache: BTreeMap<u32, u32>,
}

impl StorageLayer {
    pub fn new_in() -> Self {
        Self {
            cache: BTreeMap::default()
        }
    }

    pub fn get(&mut self, index: u32) -> u32 {
        self.cache
            .get(&index)
            .copied()
            .or_else(|| {
                let value = self.request_storage_slot_from_oracle(index);
                self.cache.insert(index, value);

                Some(value)
            })
            .unwrap()
    }

    pub fn set(&mut self, index: u32, value: u32) {
        self.cache.insert(index, value);
    }

    pub fn commit(self) -> u32 {
        let mut commitment = 0;
        for value in self.cache.values() {
            commitment += value;
        }

        commitment
    }

    fn request_storage_slot_from_oracle(&self, index: u32) -> u32 {
        csr_write_word(index as usize);
        csr_read_word()
    }
}
