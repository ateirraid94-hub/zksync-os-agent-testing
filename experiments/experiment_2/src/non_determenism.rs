use risc_v_simulator::abstractions::memory::MemorySource;
use risc_v_simulator::abstractions::non_determinism::NonDeterminismCSRSource;

use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, Default)]
pub struct DummyStorageNonDetermenismSource {
    buffer: VecDeque<u32>,
    storage: HashMap<u32, u32>,
}

impl DummyStorageNonDetermenismSource {
    pub fn set(&mut self, key: u32, value: u32) {
        self.storage.insert(key, value);
    }
}

impl<M: MemorySource> NonDeterminismCSRSource<M> for DummyStorageNonDetermenismSource {
    fn read(&mut self) -> u32 {
        let res = self.buffer.pop_front().unwrap(); // panic on unexpected read

        println!("`NonDeterminismCSRSource` sent 0x{:08x}", res);
        res
    }

    /// In general NonDeterminismSource is allowed to peek into memory (readonly)
    fn write_with_memory_access(&mut self, _memory: &M, value: u32) {
        println!("`NonDeterminismCSRSource` received 0x{:08x}", value);

        let index = value;
        // Program requests storage value at `index`.

        // Get value. Use 0 (default value) if it doesn't exist
        let value = self.storage.get(&index).copied().unwrap_or_default();

        // Push it to the buffer.
        self.buffer.push_back(value);
    }
}
