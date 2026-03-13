//! Serialization and deserialization helpers for keys and values for storage.

use crate::oracle::word_serialization::{WordDeserializable, WordSerializable};
use super::types_config::SystemIOTypesConfig;

// TODO(EVM-1167): cleanup

bitflags::bitflags! {
    /// Represents a set of flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct EmptyBitflags: u32 {}
}

pub trait ReadonlyKVMarker: 'static {
    const CAN_BE_COLD_AND_WARM_READ: bool = true;

    type Key: WordSerializable;
    type Value: WordDeserializable;
    type AccessStatsBitmask: bitflags::Flags<Bits = u32>;
}

pub trait ReadWriteKVMarker: ReadonlyKVMarker
where
    Self::Value: WordSerializable,
{
    const CAN_BE_COLD_AND_WARM_WRITE: bool = true;
}

// helper structs for most of the cases

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, WordSerializable, WordDeserializable)]
pub struct StorageAddress<IOTypes: SystemIOTypesConfig> {
    pub address: IOTypes::Address,
    pub key: IOTypes::StorageKey,
}

#[derive(Clone, Copy, Debug, WordSerializable, WordDeserializable)]
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

pub const MAX_EVENT_TOPICS: usize = 4;
