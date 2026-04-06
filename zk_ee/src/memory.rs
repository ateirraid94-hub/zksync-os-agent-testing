use std::ops::Range;

#[derive(Debug)]
pub struct MemorySlice {
    data: Vec<u8>,
}

impl MemorySlice {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
    
    pub fn len(&self) -> usize {
        self.data.len()
    }
}

#[derive(Debug)]
pub enum MemoryError {
    OutOfBounds,
    InvalidRange,
}

pub trait ReadMemory {
    fn read_range(&self, range: Range<usize>, buffer: &mut [u8]) -> Result<(), MemoryError>;
}

pub trait WriteMemory {
    fn write_range(&mut self, range: Range<usize>, data: &[u8]) -> Result<(), MemoryError>;
}

impl ReadMemory for MemorySlice {
    fn read_range(&self, range: Range<usize>, buffer: &mut [u8]) -> Result<(), MemoryError> {
        if range.end > self.data.len() || range.start >= range.end {
            return Err(MemoryError::OutOfBounds);
        }
        
        if buffer.len() != range.len() {
            return Err(MemoryError::InvalidRange);
        }
        
        buffer.copy_from_slice(&self.data[range]);
        Ok(())
    }
}

impl WriteMemory for MemorySlice {
    fn write_range(&mut self, range: Range<usize>, data: &[u8]) -> Result<(), MemoryError> {
        if range.end > self.data.len() || range.start >= range.end {
            return Err(MemoryError::OutOfBounds);
        }
        
        if data.len() != range.len() {
            return Err(MemoryError::InvalidRange);
        }
        
        self.data[range].copy_from_slice(data);
        Ok(())
    }
}
