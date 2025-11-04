use crate::internal_error;
use crate::oracle::usize_serialization::{UsizeDeserializable, UsizeSerializable};
use crate::system::errors::internal::InternalError;

///
/// Rust representation of `L2DACommitmentScheme` from l1 contracts.
/// It's used to define DA commitment zksync os outputs.
///
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum DACommitmentScheme {
    /// Invalid option.
    None,
    /// Commitment will be equals to 0, used for validiums.
    EmptyNoDA,
    /// Keccak of stateDiffHash and keccak(pubdata). Can be used by custom DA solutions.
    /// Currently not supported.
    PubdataKeccak256,
    /// This commitment includes EIP-4844 blobs data. Used by default RollupL1DAValidator.
    /// With ZKsync OS it always outputs 1 0-hash blob, as separate commitment used for blobs.
    BlobsAndPubdataKeccak256,
    /// Keccak of blob versioned hashes filled with pubdata.
    BlobsZKsyncOS,
}

impl TryFrom<u8> for DACommitmentScheme {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DACommitmentScheme::None),
            1 => Ok(DACommitmentScheme::EmptyNoDA),
            2 => Ok(DACommitmentScheme::PubdataKeccak256),
            3 => Ok(DACommitmentScheme::BlobsAndPubdataKeccak256),
            4 => Ok(DACommitmentScheme::BlobsZKsyncOS),
            _ => Err(()),
        }
    }
}

impl UsizeSerializable for DACommitmentScheme {
    const USIZE_LEN: usize = <u8 as UsizeSerializable>::USIZE_LEN;

    fn iter(&self) -> impl ExactSizeIterator<Item = usize> {
        cfg_if::cfg_if!(
            if #[cfg(target_endian = "big")] {
                compile_error!("unsupported architecture: big endian arch is not supported")
            } else if #[cfg(target_pointer_width = "32")] {
                let low = *self as u8 as usize;
                let high = 0;
                return [low, high].into_iter();
            } else if #[cfg(target_pointer_width = "64")] {
                return core::iter::once(*self as usize)
            } else {
                compile_error!("unsupported architecture")
            }
        );
    }
}

impl UsizeDeserializable for DACommitmentScheme {
    const USIZE_LEN: usize = <Self as UsizeSerializable>::USIZE_LEN;

    fn from_iter(src: &mut impl ExactSizeIterator<Item = usize>) -> Result<Self, InternalError> {
        DACommitmentScheme::try_from(u8::from_iter(src)?)
            .map_err(|_| internal_error!("Failed to parse proof data: invalid da commitment value"))
    }
}
