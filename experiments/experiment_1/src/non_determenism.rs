use risc_v_simulator::abstractions::memory::MemorySource;
use risc_v_simulator::abstractions::non_determinism::NonDeterminismCSRSource;

use std::collections::VecDeque;

#[derive(Clone, Debug, Default)]
pub struct VecDequeNonDetermenismSource {
    pub oracle: VecDeque<u32>,
}

impl<M: MemorySource> NonDeterminismCSRSource<M> for VecDequeNonDetermenismSource {
    fn read(&mut self) -> u32 {
        self.oracle.pop_front().unwrap_or_default()
    }

    /// In general NonDeterminismSource is allowed to peek into memory (readonly)
    fn write_with_memory_access(&mut self, _memory: &M, _value: u32) {
        //
    }
}
