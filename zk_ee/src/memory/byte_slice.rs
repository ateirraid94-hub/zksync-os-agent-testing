use ruint::aliases::U256;

use crate::common_traits::TryExtend;

pub trait MinimalByteAddressableSlice {
    fn len(&self) -> usize;
    fn iter<'a>(&'a self) -> impl Iterator<Item = &'a u8> + 'a
    where
        Self: 'a;
}

impl MinimalByteAddressableSlice for [u8] {
    fn len(&self) -> usize {
        Self::len(self)
    }

    fn iter<'a>(&'a self) -> impl Iterator<Item = &'a u8> + 'a
    where
        Self: 'a,
    {
        Self::iter(self)
    }
}

pub struct ArrayBuilder<const N: usize> {
    bytes: [u8; N],
    offset: usize,
}

impl<const N: usize> Default for ArrayBuilder<N> {
    fn default() -> Self {
        Self {
            bytes: [0u8; N],
            offset: Default::default()
        }
    }
}

impl<const N: usize> ArrayBuilder<N> {
    pub fn build(self) -> [u8; N] {
        assert!(self.offset == N);
        self.bytes
    }

    pub fn is_empty(&self) -> bool {
        self.offset == 0
    }
}

impl<const N: usize> TryExtend<u8> for ArrayBuilder<N> {
    type Error = ();

    fn try_extend<I>(&mut self, iter: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = u8>,
    {
        for byte in iter {
            if self.offset == self.bytes.len() {
                // ran out of space
                return Err(());
            }
            self.bytes[self.offset] = byte;
            self.offset += 1;
        }

        Ok(())
    }
}

pub struct U256Builder {
    bytes: [u8; 32],
    previously_written: usize,
}

impl Default for U256Builder {
    fn default() -> Self {
        Self {
            bytes: [0; 32],
            previously_written: 32,
        }
    }
}

impl U256Builder {
    pub fn build(self) -> U256 {
        assert!(self.previously_written == 0);
        U256::from_le_bytes(self.bytes)
    }
}

impl TryExtend<u8> for U256Builder {
    type Error = ();

    fn try_extend<I>(&mut self, iter: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = u8>,
    {
        for byte in iter {
            // Fail if input is larger than 32 bytes.
            self.previously_written = self.previously_written.checked_sub(1).ok_or(())?;
            self.bytes[self.previously_written] = byte;
        }
        Ok(())
    }
}
