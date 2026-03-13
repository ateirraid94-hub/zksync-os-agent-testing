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
    oracle::word_serialization::{WordDeserializable, WordSerializable},
};

pub mod hash_to_prime;

#[derive(Clone, Copy, Debug, WordSerializable, WordDeserializable)]
pub struct MemoryRegionDescriptionParams {
    pub offset: u32,
    pub len: u32,
}
