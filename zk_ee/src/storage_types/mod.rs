//! Serialization and deserialization helpers for keys and values for storage.

use crate::oracle::usize_serialization::{UsizeDeserializable, UsizeSerializable};
use crate::utils::exact_size_chain::ExactSizeChain;

use super::system::errors::internal::InternalError;
use super::types_config::SystemIOTypesConfig;

// helper structs for most of the cases

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StorageAddress<IOTypes: SystemIOTypesConfig> {
    pub address: IOTypes::Address,
    pub key: IOTypes::StorageKey,
}

impl<IOTypes: SystemIOTypesConfig> UsizeSerializable for StorageAddress<IOTypes> {
    const USIZE_LEN: usize = <IOTypes::Address as UsizeSerializable>::USIZE_LEN
        + <IOTypes::StorageKey as UsizeSerializable>::USIZE_LEN;

    fn iter(&self) -> impl ExactSizeIterator<Item = usize> {
        ExactSizeChain::new(
            UsizeSerializable::iter(&self.address),
            UsizeSerializable::iter(&self.key),
        )
    }
}

impl<IOTypes: SystemIOTypesConfig> UsizeDeserializable for StorageAddress<IOTypes> {
    const USIZE_LEN: usize = <Self as UsizeSerializable>::USIZE_LEN;

    fn from_iter(src: &mut impl ExactSizeIterator<Item = usize>) -> Result<Self, InternalError> {
        let address = UsizeDeserializable::from_iter(src)?;
        let key = UsizeDeserializable::from_iter(src)?;

        Ok(Self { address, key })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InitialStorageSlotData<IOTypes: SystemIOTypesConfig> {
    // we need to know what was a value of the storage slot,
    // and whether it existed in the state or has to be created
    // (so additional information is needed to reconstruct creation location)
    pub is_new_storage_slot: bool,
    pub initial_value: IOTypes::StorageValue,
}

impl<IOTypes: SystemIOTypesConfig> UsizeSerializable for InitialStorageSlotData<IOTypes> {
    const USIZE_LEN: usize = <bool as UsizeSerializable>::USIZE_LEN
        + <IOTypes::StorageValue as UsizeSerializable>::USIZE_LEN;
    fn iter(&self) -> impl ExactSizeIterator<Item = usize> {
        ExactSizeChain::new(
            UsizeSerializable::iter(&self.is_new_storage_slot),
            UsizeSerializable::iter(&self.initial_value),
        )
    }
}

impl<IOTypes: SystemIOTypesConfig> UsizeDeserializable for InitialStorageSlotData<IOTypes> {
    const USIZE_LEN: usize = <Self as UsizeSerializable>::USIZE_LEN;

    fn from_iter(src: &mut impl ExactSizeIterator<Item = usize>) -> Result<Self, InternalError> {
        let is_new_storage_slot = UsizeDeserializable::from_iter(src)?;
        let initial_value = UsizeDeserializable::from_iter(src)?;

        Ok(Self {
            is_new_storage_slot,
            initial_value,
        })
    }
}

pub const MAX_EVENT_TOPICS: usize = 4;
