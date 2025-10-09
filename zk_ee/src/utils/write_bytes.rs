use crypto::MiniDigest;

///
/// A minimal `no_std`-friendly trait for writing contiguous byte slices to a destination.
///
/// This is useful when a function needs to write its output into *different kinds of destinations*.
/// For example `pubdata` needs to be written to hasher or another accumulator depending on the commitment.
///
pub trait WriteBytes {
    fn write(&mut self, buf: impl AsRef<[u8]>);
}

impl<T: MiniDigest> WriteBytes for T {
    fn write(&mut self, buf: impl AsRef<[u8]>) {
        self.update(buf);
    }
}