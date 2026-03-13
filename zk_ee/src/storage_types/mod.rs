//! Serialization and deserialization helpers for keys and values for storage.

use crate::oracle::usize_serialization::{
    UsizeDeserializable, UsizeSerializable, WordDeserializable, WordSerializable, WordSink,
};

use super::system::errors::internal::InternalError;
use super::types_config::SystemIOTypesConfig;

// TODO(EVM-1167): cleanup

bitflags::bitflags! {
    /// Represents a set of flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct EmptyBitflags: u32 {}
}

pub trait ReadonlyKVMarker: 'static {
    const CAN_BE_COLD_AND_WARM_READ: bool = true;

    type Key: UsizeSerializable;
    type Value: UsizeDeserializable;
    type AccessStatsBitmask: bitflags::Flags<Bits = u32>;
}

pub trait ReadWriteKVMarker: ReadonlyKVMarker
where
    Self::Value: UsizeSerializable,
{
    const CAN_BE_COLD_AND_WARM_WRITE: bool = true;
}

// helper structs for most of the cases

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StorageAddress<IOTypes: SystemIOTypesConfig> {
    pub address: IOTypes::Address,
    pub key: IOTypes::StorageKey,
}

impl<IOTypes: SystemIOTypesConfig> WordSerializable for StorageAddress<IOTypes> {
    fn word_len(&self) -> usize {
        self.address.word_len() + self.key.word_len()
    }

    fn write_words(&self, out: &mut impl WordSink) {
        self.address.write_words(out);
        self.key.write_words(out);
    }
}

impl<IOTypes: SystemIOTypesConfig> WordDeserializable for StorageAddress<IOTypes> {
    fn read_words(src: &mut impl ExactSizeIterator<Item = usize>) -> Result<Self, InternalError> {
        let address = WordDeserializable::read_words(src)?;
        let key = WordDeserializable::read_words(src)?;

        let new = Self { address, key };

        Ok(new)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InitialStorageSlotData<IOTypes: SystemIOTypesConfig> {
    // we need to know what was a value of the storage slot,
    // and whether it existed in the state or has to be created
    // (so additional information is needed to reconstruct creation location)
    pub is_new_storage_slot: bool,
    pub initial_value: IOTypes::StorageValue,
}

impl<IOTypes: SystemIOTypesConfig> Default for InitialStorageSlotData<IOTypes> {
    fn default() -> Self {
        Self {
            is_new_storage_slot: false,
            initial_value: IOTypes::StorageValue::default(),
        }
    }
}

impl<IOTypes: SystemIOTypesConfig> WordSerializable for InitialStorageSlotData<IOTypes> {
    fn word_len(&self) -> usize {
        self.is_new_storage_slot.word_len() + self.initial_value.word_len()
    }

    fn write_words(&self, out: &mut impl WordSink) {
        self.is_new_storage_slot.write_words(out);
        self.initial_value.write_words(out);
    }
}

impl<IOTypes: SystemIOTypesConfig> WordDeserializable for InitialStorageSlotData<IOTypes> {
    fn read_words(src: &mut impl ExactSizeIterator<Item = usize>) -> Result<Self, InternalError> {
        let is_new_storage_slot = WordDeserializable::read_words(src)?;
        let initial_value = WordDeserializable::read_words(src)?;

        let new = Self {
            is_new_storage_slot,
            initial_value,
        };

        Ok(new)
    }
}

pub const MAX_EVENT_TOPICS: usize = 4;
