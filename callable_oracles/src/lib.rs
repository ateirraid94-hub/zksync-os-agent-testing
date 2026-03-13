#![cfg_attr(all(not(feature = "evaluate"), not(test)), no_std)]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![feature(iter_array_chunks)]
#![allow(clippy::new_without_default)]
#![allow(clippy::needless_lifetimes)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_borrows_for_generic_args)]
#![allow(clippy::bool_comparison)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::result_unit_err)]
#![allow(clippy::double_must_use)]
#![allow(clippy::explicit_auto_deref)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::borrow_deref_ref)]
#![allow(clippy::op_ref)]
#![allow(clippy::precedence)]

pub mod arithmetic;
pub mod blob_kzg_commitment;
pub mod utils;

use zk_ee::{
    oracle::usize_serialization::{WordDeserializable, WordSerializable, WordSink},
    system::errors::internal::InternalError,
};

pub mod hash_to_prime;

#[derive(Clone, Copy, Debug)]
pub struct MemoryRegionDescriptionParams {
    pub offset: u32,
    pub len: u32,
}

impl WordSerializable for MemoryRegionDescriptionParams {
    fn word_len(&self) -> usize {
        <u32 as WordSerializable>::word_len(&self.offset)
            + <u32 as WordSerializable>::word_len(&self.len)
    }

    fn write_words(&self, out: &mut impl WordSink) {
        <u32 as WordSerializable>::write_words(&self.offset, out);
        <u32 as WordSerializable>::write_words(&self.len, out);
    }
}

impl WordDeserializable for MemoryRegionDescriptionParams {
    fn read_words(src: &mut impl ExactSizeIterator<Item = usize>) -> Result<Self, InternalError> {
        let offset = <u32 as WordDeserializable>::read_words(src)?;
        let len = <u32 as WordDeserializable>::read_words(src)?;

        let new = Self { offset, len };

        Ok(new)
    }
}
