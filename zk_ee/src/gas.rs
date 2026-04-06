#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gas {
    value: u64,
}

impl Gas {
    pub const fn new(value: u64) -> Self {
        Self { value }
    }
    
    pub fn value(&self) -> u64 {
        self.value
    }
}
