use crate::{
    oracle::usize_serialization::{UsizeDeserializable, UsizeSerializable},
    system::errors::internal::InternalError,
    utils::{exact_size_chain::ExactSizeChain, Bytes32},
};

/// Represents a cross-chain interoperability root that enables
/// communication and state verification between different blockchain networks.
#[cfg_attr(feature = "testing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct InteropRoot {
    /// The merkle root hash (cannot be zero for valid roots)
    pub root: Bytes32,
    /// Block or batch number from the source chain
    pub block_or_batch_number: u64,
    /// Source chain identifier (must be non-zero)
    pub chain_id: u64,
}

impl UsizeSerializable for InteropRoot {
    const USIZE_LEN: usize = <Bytes32 as UsizeSerializable>::USIZE_LEN
        + <u64 as UsizeSerializable>::USIZE_LEN
        + <u64 as UsizeSerializable>::USIZE_LEN;

    fn iter(&self) -> impl ExactSizeIterator<Item = usize> {
        ExactSizeChain::new(
            ExactSizeChain::new(
                UsizeSerializable::iter(&self.root),
                UsizeSerializable::iter(&self.block_or_batch_number),
            ),
            UsizeSerializable::iter(&self.chain_id),
        )
    }
}

impl UsizeDeserializable for InteropRoot {
    const USIZE_LEN: usize = <Bytes32 as UsizeSerializable>::USIZE_LEN
        + <u64 as UsizeSerializable>::USIZE_LEN
        + <u64 as UsizeSerializable>::USIZE_LEN;

    fn from_iter(src: &mut impl ExactSizeIterator<Item = usize>) -> Result<Self, InternalError> {
        let root = <Bytes32 as UsizeDeserializable>::from_iter(src)?;
        let block_number = <u64 as UsizeDeserializable>::from_iter(src)?;
        let chain_id = <u64 as UsizeDeserializable>::from_iter(src)?;

        let new = Self {
            root,
            block_or_batch_number: block_number,
            chain_id,
        };

        Ok(new)
    }
}
