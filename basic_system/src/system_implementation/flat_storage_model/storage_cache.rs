//! Storage cache, backed by a history map.
use crate::system_implementation::caches::generic_pubdata_aware_plain_storage::{
    GenericPubdataAwarePlainStorage, StorageSnapshotId,
};
use crate::system_implementation::caches::storage_access_policy::StorageAccessPolicy;
use crate::system_implementation::flat_storage_model::address_into_special_storage_key;
use alloc::collections::BTreeSet;
use core::alloc::Allocator;
use ruint::aliases::B160;
use storage_models::common_structs::snapshottable_io::SnapshottableIo;
use storage_models::common_structs::{AccountAggregateDataHash, StorageCacheModel};
use zk_ee::execution_environment_type::ExecutionEnvironmentType;
use zk_ee::oracle::IOOracle;
use zk_ee::system::errors::internal::InternalError;
use zk_ee::{
    common_structs::{WarmStorageKey, WarmStorageValue},
    memory::stack_trait::StackFactory,
    system::{errors::system::SystemError, Resources},
    types_config::{EthereumIOTypesConfig, SystemIOTypesConfig},
    utils::Bytes32,
};

use zk_ee::common_structs::ValueDiffCompressionStrategy;

/// This storage knows concrete definitions where wer store account data hashes, etc
///
/// The address of the account which storage will be used to save mapping from account addresses to
/// partial account data(nonce, code length, etc). (key is an address, value is encoded partial
/// account data).
///
pub const ACCOUNT_PROPERTIES_STORAGE_ADDRESS: B160 = B160::from_limbs([0x8003, 0, 0]);

pub struct NewStorageWithAccountPropertiesUnderHash<
    A: Allocator + Clone,
    SF: StackFactory<M>,
    const M: usize,
    R: Resources,
    P: StorageAccessPolicy<R, Bytes32>,
>(pub GenericPubdataAwarePlainStorage<WarmStorageKey, Bytes32, A, SF, M, R, P>);

impl<
        A: Allocator + Clone,
        SF: StackFactory<M>,
        const M: usize,
        R: Resources,
        P: StorageAccessPolicy<R, Bytes32>,
    > StorageCacheModel for NewStorageWithAccountPropertiesUnderHash<A, SF, M, R, P>
{
    type IOTypes = EthereumIOTypesConfig;
    type Resources = R;

    fn read(
        &mut self,
        ee_type: ExecutionEnvironmentType,
        resources: &mut Self::Resources,
        address: &<Self::IOTypes as SystemIOTypesConfig>::Address,
        key: &<Self::IOTypes as SystemIOTypesConfig>::StorageKey,
        oracle: &mut impl IOOracle,
    ) -> Result<<Self::IOTypes as SystemIOTypesConfig>::StorageKey, SystemError> {
        let key = WarmStorageKey {
            address: *address,
            key: *key,
        };

        self.0.apply_read_impl(ee_type, &key, resources, oracle)
    }

    fn touch(
        &mut self,
        _ee_type: ExecutionEnvironmentType,
        resources: &mut Self::Resources,
        address: &<Self::IOTypes as SystemIOTypesConfig>::Address,
        key: &<Self::IOTypes as SystemIOTypesConfig>::StorageKey,
    ) -> Result<(), SystemError> {
        let key = WarmStorageKey {
            address: *address,
            key: *key,
        };

        self.0.touch_impl(&key, resources)
    }

    fn write(
        &mut self,
        ee_type: ExecutionEnvironmentType,
        resources: &mut Self::Resources,
        address: &<Self::IOTypes as SystemIOTypesConfig>::Address,
        key: &<Self::IOTypes as SystemIOTypesConfig>::StorageKey,
        new_value: &<Self::IOTypes as SystemIOTypesConfig>::StorageValue,
        oracle: &mut impl IOOracle,
    ) -> Result<<Self::IOTypes as SystemIOTypesConfig>::StorageValue, SystemError> {
        let key = WarmStorageKey {
            address: *address,
            key: *key,
        };

        #[allow(unused_variables)]
        let (old_value, val_at_tx_start) = self
            .0
            .apply_write_impl(ee_type, &key, new_value, oracle, resources)?;

        Ok(old_value)
    }

    fn read_special_account_property<T: storage_models::common_structs::SpecialAccountProperty>(
        &mut self,
        ee_type: ExecutionEnvironmentType,
        resources: &mut Self::Resources,
        address: &<Self::IOTypes as SystemIOTypesConfig>::Address,
        oracle: &mut impl IOOracle,
    ) -> Result<T::Value, SystemError> {
        if core::any::TypeId::of::<T>() != core::any::TypeId::of::<AccountAggregateDataHash>() {
            panic!("unsupported property type in this model");
        }
        // this is the only tricky part, and the only special account property that we support is a hash
        // of the total account properties

        let key = address_into_special_storage_key(address);

        // we just need to create a proper access function

        let key = WarmStorageKey {
            address: ACCOUNT_PROPERTIES_STORAGE_ADDRESS,
            key,
        };

        let raw_value = self.0.apply_read_impl(ee_type, &key, resources, oracle)?;

        let value = unsafe {
            // we checked TypeId above, so we reinterpret. No drop/forget needed
            core::ptr::read((&raw_value as *const Bytes32).cast::<T::Value>())
        };

        Ok(value)
    }

    fn write_special_account_property<T: storage_models::common_structs::SpecialAccountProperty>(
        &mut self,
        ee_type: ExecutionEnvironmentType,
        resources: &mut Self::Resources,
        address: &<Self::IOTypes as SystemIOTypesConfig>::Address,
        new_value: &T::Value,
        oracle: &mut impl IOOracle,
    ) -> Result<T::Value, SystemError> {
        if core::any::TypeId::of::<T>() != core::any::TypeId::of::<AccountAggregateDataHash>() {
            panic!("unsupported property type in this model");
        }
        // this is the only tricky part, and the only special account property that we support is a hash
        // of the total account properties

        let key = address_into_special_storage_key(address);

        let key = WarmStorageKey {
            address: ACCOUNT_PROPERTIES_STORAGE_ADDRESS,
            key,
        };

        let new_value = unsafe {
            // we checked TypeId above, so we reinterpret. No drop/forget needed
            core::ptr::read((new_value as *const T::Value).cast::<Bytes32>())
        };

        let (old_value, _) = self
            .0
            .apply_write_impl(ee_type, &key, &new_value, oracle, resources)?;

        let old_value = unsafe {
            // we checked TypeId above, so we reinterpret. No drop/forget needed
            core::ptr::read((&old_value as *const Bytes32).cast::<T::Value>())
        };

        Ok(old_value)
    }
}

impl<
        A: Allocator + Clone,
        SF: StackFactory<M>,
        const M: usize,
        R: Resources,
        P: StorageAccessPolicy<R, Bytes32>,
    > SnapshottableIo for NewStorageWithAccountPropertiesUnderHash<A, SF, M, R, P>
{
    type StateSnapshot = StorageSnapshotId;

    fn begin_new_tx(&mut self) {
        self.0.begin_new_tx();
    }

    fn finish_tx(&mut self) -> Result<(), InternalError> {
        self.0.finish_tx();
        Ok(())
    }

    fn start_frame(&mut self) -> Self::StateSnapshot {
        self.0.start_frame()
    }

    fn finish_frame(
        &mut self,
        rollback_handle: Option<&Self::StateSnapshot>,
    ) -> Result<(), InternalError> {
        self.0.finish_frame_impl(rollback_handle)
    }
}

impl<
        A: Allocator + Clone,
        SF: StackFactory<M>,
        const M: usize,
        R: Resources,
        P: StorageAccessPolicy<R, Bytes32>,
    > NewStorageWithAccountPropertiesUnderHash<A, SF, M, R, P>
{
    pub fn iter_as_storage_types(
        &self,
    ) -> impl Iterator<Item = (WarmStorageKey, WarmStorageValue)> + Clone + use<'_, A, SF, M, R, P>
    {
        self.0.cache.iter().filter_map(|item| {
            if !item.key_properties().is_value_observed() {
                return None;
            }

            let is_new_storage_slot = item.key_properties().is_new_element();
            let initial_value_used = item.key_properties().is_value_observed();
            let current_record = item.current();
            let initial_record = item.initial();
            let current_value = current_record.value()?;
            let initial_value = initial_record.value()?;

            Some((
                *item.key(),
                // Using the WarmStorageValue temporarily till it's outed from the codebase. We're
                // not actually 'using' it.
                WarmStorageValue {
                    current_value: *current_value,
                    is_new_storage_slot,
                    initial_value: *initial_value,
                    initial_value_used,
                    ..Default::default()
                },
            ))
        })
    }
    ///
    /// Returns all the accessed storage slots.
    ///
    /// This one should be used for merkle proof validation, includes initial reads.
    ///
    pub fn net_accesses_iter(
        &self,
    ) -> impl Iterator<Item = (WarmStorageKey, WarmStorageValue)> + Clone + use<'_, A, SF, M, R, P>
    {
        self.iter_as_storage_types()
    }

    ///
    /// Returns slots that were changed during execution.
    ///
    pub fn net_diffs_iter(
        &self,
    ) -> impl Iterator<Item = (WarmStorageKey, WarmStorageValue)> + use<'_, A, SF, M, R, P> {
        self.iter_as_storage_types()
            .filter(|(_, v)| v.current_value != v.initial_value)
    }

    pub fn calculate_pubdata_used_by_tx(&self) -> u32 {
        let mut visited_elements = BTreeSet::new_in(self.0.alloc.clone());

        let mut pubdata_used = 0u32;
        for element_history in self.0.cache.iter_altered_since_commit() {
            // Elements are sorted chronologically

            let element_key = element_history.key();

            // we publish preimages for account details, so no need to publish hash
            if element_key.address == ACCOUNT_PROPERTIES_STORAGE_ADDRESS {
                continue;
            }

            // Skip if already calculated pubdata for this element
            if visited_elements.contains(element_key) {
                continue;
            }
            visited_elements.insert(element_key);

            let Some(current_value) = element_history.current().value() else {
                continue;
            };
            let Some(initial_value) = element_history.initial().value() else {
                continue;
            };
            let Some(at_tx_start_value) = element_history.committed().value() else {
                continue;
            };

            // If the current value is resetting to the initial one,
            // we don't consider this diff in the pubdata charging.
            // This change will be optimized away, so it's actually reducing
            // pubdata.
            if current_value == initial_value {
                continue;
            }

            if at_tx_start_value != current_value {
                // TODO(EVM-1074): use tree index instead of key for repeated writes
                pubdata_used += 32; // key
                pubdata_used += ValueDiffCompressionStrategy::optimal_compression_length(
                    at_tx_start_value,
                    current_value,
                ) as u32;
            }
        }

        pubdata_used
    }
}

#[cfg(test)]
mod tests {
    use super::{NewStorageWithAccountPropertiesUnderHash, ACCOUNT_PROPERTIES_STORAGE_ADDRESS};
    use crate::system_implementation::caches::generic_pubdata_aware_plain_storage::GenericPubdataAwarePlainStorage;
    use crate::system_implementation::system::EthereumLikeStorageAccessCostModel;
    use std::alloc::Global;
    use zk_ee::common_structs::WarmStorageKey;
    use zk_ee::execution_environment_type::ExecutionEnvironmentType;
    use zk_ee::memory::stack_implementations::vec_stack::VecStackFactory;
    use zk_ee::oracle::query_ids::INITIAL_STORAGE_SLOT_VALUE_QUERY_ID;
    use zk_ee::oracle::usize_serialization::{UsizeDeserializable, UsizeSerializable};
    use zk_ee::oracle::IOOracle;
    use zk_ee::reference_implementations::{BaseResources, DecreasingNative};
    use zk_ee::storage_types::InitialStorageSlotData;
    use zk_ee::system::errors::internal::InternalError;
    use zk_ee::system::Resource;
    use zk_ee::types_config::EthereumIOTypesConfig;
    use zk_ee::utils::Bytes32;

    type TestResources = BaseResources<DecreasingNative>;
    type TestStorage = NewStorageWithAccountPropertiesUnderHash<
        Global,
        VecStackFactory,
        8,
        TestResources,
        EthereumLikeStorageAccessCostModel,
    >;

    #[derive(Default)]
    struct TestOracle {
        slot_values: Vec<(
            (ruint::aliases::B160, Bytes32),
            InitialStorageSlotData<EthereumIOTypesConfig>,
        )>,
        slot_queries: usize,
    }

    struct AddressArg(ruint::aliases::B160);

    impl AsRef<ruint::aliases::B160> for AddressArg {
        fn as_ref(&self) -> &ruint::aliases::B160 {
            &self.0
        }
    }

    impl TestOracle {
        fn with_slot(key: WarmStorageKey, value: Bytes32, is_new_storage_slot: bool) -> Self {
            let slot_values = vec![(
                (key.address, key.key),
                InitialStorageSlotData {
                    is_new_storage_slot,
                    initial_value: value,
                },
            )];
            Self {
                slot_values,
                slot_queries: 0,
            }
        }
    }

    impl IOOracle for TestOracle {
        type RawIterator<'a> = std::vec::IntoIter<usize>;

        fn raw_query<'a, I: UsizeSerializable + UsizeDeserializable>(
            &'a mut self,
            query_type: u32,
            input: &I,
        ) -> Result<Self::RawIterator<'a>, InternalError> {
            match query_type {
                INITIAL_STORAGE_SLOT_VALUE_QUERY_ID => {
                    self.slot_queries += 1;
                    let mut input_iter = input.iter();
                    let key =
                        zk_ee::storage_types::StorageAddress::<EthereumIOTypesConfig>::from_iter(
                            &mut input_iter,
                        )?;
                    assert!(input_iter.next().is_none());
                    let value = self
                        .slot_values
                        .iter()
                        .find_map(|((address, storage_key), value)| {
                            (*address == key.address && *storage_key == key.key).then_some(*value)
                        })
                        .unwrap_or_default();
                    Ok(value.iter().collect::<Vec<_>>().into_iter())
                }
                _ => panic!("unexpected query id {query_type}"),
            }
        }
    }

    fn test_storage() -> TestStorage {
        NewStorageWithAccountPropertiesUnderHash(GenericPubdataAwarePlainStorage::new_from_parts(
            Global,
            EthereumLikeStorageAccessCostModel,
        ))
    }

    fn test_key() -> WarmStorageKey {
        WarmStorageKey {
            address: ruint::aliases::B160::from_limbs([7, 0, 0]),
            key: Bytes32::from([3u8; 32]),
        }
    }

    #[test]
    fn touched_only_slots_are_hidden_from_iterators_and_deconstruction_clear() {
        let mut storage = test_storage();
        let key = test_key();
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.0.touch_impl(&key, &mut resources).unwrap();

        let item = storage.0.cache.get(&key).unwrap();
        assert!(item.current().value().is_none());
        assert!(!item.key_properties().is_value_observed());
        assert_eq!(storage.iter_as_storage_types().count(), 0);

        storage.0.clear_state_impl(AddressArg(key.address)).unwrap();

        let item = storage.0.cache.get(&key).unwrap();
        assert!(item.current().value().is_none());
        assert_eq!(storage.iter_as_storage_types().count(), 0);
    }

    #[test]
    fn touched_slot_materializes_on_read_and_becomes_iterable() {
        let mut storage = test_storage();
        let key = test_key();
        let expected_value = Bytes32::from([9u8; 32]);
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.0.touch_impl(&key, &mut resources).unwrap();

        let mut oracle = TestOracle::with_slot(key, expected_value, false);
        let value = storage
            .0
            .apply_read_impl(
                ExecutionEnvironmentType::NoEE,
                &key,
                &mut resources,
                &mut oracle,
            )
            .unwrap();

        assert_eq!(oracle.slot_queries, 1);
        assert_eq!(value, expected_value);

        let item = storage.0.cache.get(&key).unwrap();
        assert_eq!(item.current().value(), Some(&expected_value));
        assert!(item.key_properties().is_value_observed());

        let accesses = storage.iter_as_storage_types().collect::<Vec<_>>();
        assert_eq!(accesses.len(), 1);
        assert_eq!(accesses[0].0, key);
        assert_eq!(accesses[0].1.current_value, expected_value);
        assert_eq!(accesses[0].1.initial_value, expected_value);
        assert!(accesses[0].1.initial_value_used);
        assert!(!accesses[0].1.is_new_storage_slot);
    }

    #[test]
    fn account_property_subspace_is_not_special_cased_by_storage_iterator() {
        let mut storage = test_storage();
        let key = WarmStorageKey {
            address: ACCOUNT_PROPERTIES_STORAGE_ADDRESS,
            key: Bytes32::from([1u8; 32]),
        };
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.0.touch_impl(&key, &mut resources).unwrap();

        assert_eq!(storage.iter_as_storage_types().count(), 0);
    }
}
