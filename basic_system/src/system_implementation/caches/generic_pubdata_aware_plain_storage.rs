//! Storage cache, backed by a history map.
use crate::system_implementation::caches::cache_element_properties::CacheElementProperties;
use crate::system_implementation::caches::storage_access_policy::StorageAccessPolicy;
use alloc::fmt::Debug;
use core::alloc::Allocator;
use ruint::aliases::B160;
use zk_ee::common_structs::cache_record::CacheRecord;
use zk_ee::common_structs::history_counter::HistoryCounterSnapshotId;
use zk_ee::common_structs::history_counter::NonEmptyHistoryCounter;
use zk_ee::common_traits::key_like_with_bounds::{KeyLikeWithBounds, TyEq};
use zk_ee::execution_environment_type::ExecutionEnvironmentType;
use zk_ee::internal_error;
use zk_ee::oracle::basic_queries::InitialStorageSlotQuery;
use zk_ee::oracle::IOOracle;
use zk_ee::system::errors::internal::InternalError;
use zk_ee::{
    memory::stack_trait::StackFactory,
    oracle::simple_oracle_query::SimpleOracleQuery,
    storage_types::StorageAddress,
    system::{errors::system::SystemError, Resources},
    types_config::{EthereumIOTypesConfig, SystemIOTypesConfig},
};

use zk_ee::common_structs::history_map::*;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct TransactionId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsWarmRead(pub bool);

type AddressItem<'a, K, V, A> =
    HistoryMapItemRefMut<'a, K, CacheRecord<V, StorageElementMetadata>, A, CacheElementProperties>;

#[derive(Default, Clone)]
pub struct StorageElementMetadata {
    /// Transaction where this account was last accessed.
    /// Considered warm if equal to Some(current_tx)
    pub last_touched_in_tx: Option<TransactionId>,
}

impl StorageElementMetadata {
    pub fn considered_warm(&self, current_tx_id: TransactionId) -> bool {
        self.last_touched_in_tx == Some(current_tx_id)
    }
}

#[derive(Debug)]
pub struct StorageSnapshotId {
    pub cache: CacheSnapshotId,
    pub evm_refunds_counter: HistoryCounterSnapshotId,
}

pub struct GenericPubdataAwarePlainStorage<
    K: KeyLikeWithBounds,
    V,
    A: Allocator + Clone, // = Global,
    SF: StackFactory<M>,
    const M: usize,
    R: Resources,
    P: StorageAccessPolicy<R, V>,
> {
    pub(crate) cache:
        HistoryMap<K, CacheRecord<V, StorageElementMetadata>, A, CacheElementProperties>,
    pub(crate) resources_policy: P,
    // Note: this doesn't need to be equal to the actual tx number in the block, it just needs to be able to differentiate between transactions.
    pub(crate) current_tx_id: TransactionId,
    pub(crate) evm_refunds_counter: NonEmptyHistoryCounter<R, SF, M, A>, // Used to keep track of EVM gas refunds
    pub(crate) alloc: A,
    pub(crate) _marker: core::marker::PhantomData<(R, SF)>,
}

impl<
        K: 'static + KeyLikeWithBounds,
        V: Default
            + Clone
            + Debug
            + PartialEq
            + From<<EthereumIOTypesConfig as SystemIOTypesConfig>::StorageValue>,
        A: Allocator + Clone,
        SF: StackFactory<M>,
        const M: usize,
        R: Resources,
        P: StorageAccessPolicy<R, V>,
    > GenericPubdataAwarePlainStorage<K, V, A, SF, M, R, P>
{
    fn query_initial_value(key: &K, oracle: &mut impl IOOracle) -> Result<(V, bool), SystemError>
    where
        StorageAddress<EthereumIOTypesConfig>: From<K>,
    {
        let query_input = (*key).into();
        let data_from_oracle = InitialStorageSlotQuery::get(oracle, &query_input)
            .map_err(|_| internal_error!("Must get initial slot value from oracle"))?;
        let value = data_from_oracle.initial_value.into();

        if data_from_oracle.is_new_storage_slot {
            assert_eq!(
                V::default(),
                value,
                "Initial value of empty slot must be trivial"
            );
        }

        Ok((value, data_from_oracle.is_new_storage_slot))
    }

    pub fn new_from_parts(allocator: A, resources_policy: P) -> Self {
        Self {
            cache: HistoryMap::new(allocator.clone()),
            current_tx_id: TransactionId(0),
            resources_policy,
            evm_refunds_counter: NonEmptyHistoryCounter::new_with_initial(
                allocator.clone(),
                R::empty(),
            ),
            alloc: allocator.clone(),
            _marker: core::marker::PhantomData,
        }
    }

    pub fn begin_new_tx(&mut self) {
        self.cache.commit();
        self.evm_refunds_counter =
            NonEmptyHistoryCounter::new_with_initial(self.alloc.clone(), R::empty());
    }

    pub fn finish_tx(&mut self) {
        self.current_tx_id.0 += 1;
    }

    #[track_caller]
    pub fn start_frame(&mut self) -> StorageSnapshotId {
        StorageSnapshotId {
            cache: self.cache.snapshot(),
            evm_refunds_counter: self.evm_refunds_counter.snapshot(),
        }
    }

    #[track_caller]
    #[must_use]
    pub fn finish_frame_impl(
        &mut self,
        rollback_handle: Option<&StorageSnapshotId>,
    ) -> Result<(), InternalError> {
        if let Some(x) = rollback_handle {
            self.evm_refunds_counter.rollback(x.evm_refunds_counter);
            self.cache.rollback(x.cache)
        } else {
            Ok(())
        }
    }

    pub fn touch_impl(&mut self, key: &K, resources: &mut R) -> Result<(), SystemError> {
        self.resources_policy
            .charge_access_list_storage_touch(resources)?;

        let mut item = self.cache.get_or_insert(key, || {
            Ok::<_, SystemError>((
                CacheRecord::new_empty_with_metadata(StorageElementMetadata {
                    last_touched_in_tx: Some(self.current_tx_id),
                }),
                CacheElementProperties::new(false, false),
            ))
        })?;

        if !item
            .current()
            .metadata()
            .considered_warm(self.current_tx_id)
        {
            if item.element_properties().is_value_observed() {
                item.update(|cache_record| {
                    cache_record.update_metadata(|metadata| {
                        metadata.last_touched_in_tx = Some(self.current_tx_id);
                        Ok(())
                    })
                })?;
            } else {
                item.mutate_current_in_place(|cache_record| {
                    cache_record.update_metadata_infallible(|metadata| {
                        metadata.last_touched_in_tx = Some(self.current_tx_id);
                    });
                });
            }
        }

        Ok(())
    }

    /// Read element and initialize it if needed
    fn materialize_element<'a>(
        cache: &'a mut HistoryMap<
            K,
            CacheRecord<V, StorageElementMetadata>,
            A,
            CacheElementProperties,
        >,
        resources_policy: &mut P,
        current_tx_id: TransactionId,
        ee_type: ExecutionEnvironmentType,
        resources: &mut R,
        key: &'a K,
        oracle: &mut impl IOOracle,
    ) -> Result<(AddressItem<'a, K, V, A>, IsWarmRead), SystemError>
    where
        StorageAddress<EthereumIOTypesConfig>: From<K>,
    {
        resources_policy.charge_warm_storage_read(ee_type, resources)?;

        let mut initialized_element = false;

        cache
            .get_or_insert(key, || {
                // Element doesn't exist in cache yet, initialize it
                initialized_element = true;
                let (value, is_new_storage_slot) = Self::query_initial_value(key, oracle)?;
                resources_policy.charge_cold_storage_read_extra(
                    ee_type,
                    resources,
                    is_new_storage_slot,
                )?;

                // Note: we initialize it as cold, should be warmed up separately
                // Since in case of revert it should become cold again and initial record can't be rolled back
                Ok((
                    CacheRecord::new(value),
                    CacheElementProperties::new(is_new_storage_slot, true),
                ))
            })
            .and_then(|mut x| {
                if !x.element_properties().is_value_observed() {
                    let (value, is_new_storage_slot) = Self::query_initial_value(key, oracle)?;
                    x.mutate_current_in_place(|cache_record| cache_record.materialize(value));
                    x.element_properties_mut().set_is_new(is_new_storage_slot);
                    x.element_properties_mut().mark_value_as_observed();
                }

                // Warm up element according to EVM rules if needed
                let is_warm_read = x.current().metadata().considered_warm(current_tx_id);
                if is_warm_read == false {
                    if initialized_element == false {
                        let is_new_storage_slot = x.element_properties().is_new_element();
                        // Element exists in cache, but wasn't touched in current tx yet
                        resources_policy.charge_cold_storage_read_extra(
                            ee_type,
                            resources,
                            is_new_storage_slot,
                        )?;
                    }

                    x.update(|cache_record| {
                        cache_record.update_metadata(|m| {
                            m.last_touched_in_tx = Some(current_tx_id);
                            Ok(())
                        })
                    })?;
                }

                Ok((x, IsWarmRead(is_warm_read)))
            })
    }

    pub fn apply_read_impl(
        &mut self,
        ee_type: ExecutionEnvironmentType,
        key: &K,
        resources: &mut R,
        oracle: &mut impl IOOracle,
    ) -> Result<V, SystemError>
    where
        StorageAddress<EthereumIOTypesConfig>: From<K>,
    {
        let (addr_data, _) = Self::materialize_element(
            &mut self.cache,
            &mut self.resources_policy,
            self.current_tx_id,
            ee_type,
            resources,
            key,
            oracle,
        )?;

        Ok(addr_data.current().materialized_value()?.clone())
    }

    pub fn apply_write_impl(
        &mut self,
        ee_type: ExecutionEnvironmentType,
        key: &K,
        new_value: &V,
        oracle: &mut impl IOOracle,
        resources: &mut R,
    ) -> Result<(V, V), SystemError>
    where
        StorageAddress<EthereumIOTypesConfig>: From<K>,
    {
        let (mut addr_data, is_warm_read) = Self::materialize_element(
            &mut self.cache,
            &mut self.resources_policy,
            self.current_tx_id,
            ee_type,
            resources,
            key,
            oracle,
        )?;

        let val_current = addr_data.current().materialized_value()?;

        // Try to get initial value at the beginning of the tx.
        let val_at_tx_start = addr_data.committed().materialized_value()?.clone();

        let is_new_slot = addr_data.element_properties().is_new_element();
        self.resources_policy.charge_storage_write_extra(
            ee_type,
            &val_at_tx_start,
            val_current,
            new_value,
            resources,
            is_warm_read.0,
            is_new_slot,
        )?;

        let old_value = addr_data.current().materialized_value()?.clone();
        addr_data.update(|cache_record| {
            cache_record.update_materialized(|x, _| {
                *x = new_value.clone();
                Ok(())
            })
        })?;

        // Add refund for storage
        let mut refund_counter_value = self.evm_refunds_counter.value().clone();
        self.resources_policy.refund_for_storage_write(
            ee_type,
            &val_at_tx_start,
            &old_value,
            new_value,
            resources,
            &mut refund_counter_value,
        )?;
        self.evm_refunds_counter.update(refund_counter_value);

        Ok((old_value, val_at_tx_start))
    }

    /// Clear state at specified address
    pub fn clear_state_impl(&mut self, address: impl AsRef<B160>) -> Result<(), SystemError>
    where
        K::Subspace: TyEq<B160>,
    {
        use core::ops::Bound::Included;
        let lower_bound = K::lower_bound(TyEq::rwi(*address.as_ref()));
        let upper_bound = K::upper_bound(TyEq::rwi(*address.as_ref()));
        self.cache
            .for_each_range((Included(&lower_bound), Included(&upper_bound)), |mut x| {
                if x.element_properties().is_value_observed() {
                    x.update(|cache_record| {
                        cache_record.update_materialized(|v, _| {
                            *v = V::default();
                            Ok(())
                        })
                    })
                } else {
                    Ok(())
                }
            })?;

        Ok(())
    }

    pub fn get_refund_counter_impl(&'_ self) -> &'_ R {
        self.evm_refunds_counter.value()
    }

    pub fn add_to_refund_counter_impl(&mut self, refund: R) -> Result<(), SystemError> {
        let mut t = self.get_refund_counter_impl().clone();
        t.add_ergs(refund.ergs());
        self.evm_refunds_counter.update(t);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{GenericPubdataAwarePlainStorage, TransactionId};
    use crate::system_implementation::caches::storage_access_policy::StorageAccessPolicy;
    use std::alloc::Global;
    use std::cell::{Cell, RefCell};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use zk_ee::common_structs::WarmStorageKey;
    use zk_ee::execution_environment_type::ExecutionEnvironmentType;
    use zk_ee::memory::stack_implementations::vec_stack::VecStackFactory;
    use zk_ee::oracle::query_ids::INITIAL_STORAGE_SLOT_VALUE_QUERY_ID;
    use zk_ee::oracle::usize_serialization::{UsizeDeserializable, UsizeSerializable};
    use zk_ee::oracle::IOOracle;
    use zk_ee::reference_implementations::{BaseResources, DecreasingNative};
    use zk_ee::storage_types::InitialStorageSlotData;
    use zk_ee::system::errors::internal::InternalError;
    use zk_ee::system::errors::system::SystemError;
    use zk_ee::system::Resource;
    use zk_ee::types_config::EthereumIOTypesConfig;
    use zk_ee::utils::Bytes32;

    type TestResources = BaseResources<DecreasingNative>;
    type TestStorage = GenericPubdataAwarePlainStorage<
        WarmStorageKey,
        Bytes32,
        Global,
        VecStackFactory,
        8,
        TestResources,
        CountingPolicy,
    >;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct WriteCharge {
        ee_type: ExecutionEnvironmentType,
        initial_value: Bytes32,
        current_value: Bytes32,
        new_value: Bytes32,
        is_warm_write: bool,
        is_new_slot: bool,
    }

    #[derive(Default)]
    struct CountingPolicy {
        access_list_touches: Cell<usize>,
        warm_reads: Cell<usize>,
        cold_reads: Cell<usize>,
        writes: RefCell<Vec<WriteCharge>>,
        refunds: Cell<usize>,
    }

    impl StorageAccessPolicy<TestResources, Bytes32> for CountingPolicy {
        fn charge_access_list_storage_touch(
            &self,
            _resources: &mut TestResources,
        ) -> Result<(), SystemError> {
            self.access_list_touches
                .set(self.access_list_touches.get() + 1);
            Ok(())
        }

        fn charge_warm_storage_read(
            &self,
            _ee_type: ExecutionEnvironmentType,
            _resources: &mut TestResources,
        ) -> Result<(), SystemError> {
            self.warm_reads.set(self.warm_reads.get() + 1);
            Ok(())
        }

        fn charge_cold_storage_read_extra(
            &self,
            _ee_type: ExecutionEnvironmentType,
            _resources: &mut TestResources,
            _is_new_slot: bool,
        ) -> Result<(), SystemError> {
            self.cold_reads.set(self.cold_reads.get() + 1);
            Ok(())
        }

        fn charge_storage_write_extra(
            &self,
            ee_type: ExecutionEnvironmentType,
            initial_value: &Bytes32,
            current_value: &Bytes32,
            new_value: &Bytes32,
            _resources: &mut TestResources,
            is_warm_write: bool,
            is_new_slot: bool,
        ) -> Result<(), SystemError> {
            self.writes.borrow_mut().push(WriteCharge {
                ee_type,
                initial_value: *initial_value,
                current_value: *current_value,
                new_value: *new_value,
                is_warm_write,
                is_new_slot,
            });
            Ok(())
        }

        fn refund_for_storage_write(
            &self,
            _ee_type: ExecutionEnvironmentType,
            _value_at_tx_start: &Bytes32,
            _current_value: &Bytes32,
            _new_value: &Bytes32,
            _resources: &mut TestResources,
            _refund_counter: &mut TestResources,
        ) -> Result<(), SystemError> {
            self.refunds.set(self.refunds.get() + 1);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestOracle {
        slot_values: Vec<(
            (ruint::aliases::B160, Bytes32),
            InitialStorageSlotData<EthereumIOTypesConfig>,
        )>,
        slot_queries: usize,
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

    struct AddressArg(ruint::aliases::B160);

    impl AsRef<ruint::aliases::B160> for AddressArg {
        fn as_ref(&self) -> &ruint::aliases::B160 {
            &self.0
        }
    }

    fn test_storage() -> TestStorage {
        GenericPubdataAwarePlainStorage::new_from_parts(Global, CountingPolicy::default())
    }

    fn test_key(slot: u8) -> WarmStorageKey {
        WarmStorageKey {
            address: ruint::aliases::B160::from_limbs([7, 0, 0]),
            key: Bytes32::from([slot; 32]),
        }
    }

    fn advance_to_next_tx(storage: &mut TestStorage) {
        storage.finish_tx();
        storage.begin_new_tx();
    }

    #[test]
    fn touch_impl_creates_unmaterialized_warm_entry_and_only_charges_access_list() {
        let mut storage = test_storage();
        let key = test_key(1);
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.touch_impl(&key, &mut resources).unwrap();

        let item = storage.cache.get(&key).unwrap();
        assert!(item.current().value().is_none());
        assert!(!item.key_properties().is_value_observed());
        assert_eq!(
            item.current().metadata().last_touched_in_tx,
            Some(TransactionId(0))
        );
        assert_eq!(storage.resources_policy.access_list_touches.get(), 1);
        assert_eq!(storage.resources_policy.warm_reads.get(), 0);
        assert_eq!(storage.resources_policy.cold_reads.get(), 0);
        assert_eq!(storage.cache.iter_altered_since_commit().count(), 0);
    }

    #[test]
    fn touch_impl_on_existing_none_entry_updates_metadata_in_place_across_txs() {
        let mut storage = test_storage();
        let key = test_key(2);
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.touch_impl(&key, &mut resources).unwrap();
        advance_to_next_tx(&mut storage);

        storage.touch_impl(&key, &mut resources).unwrap();

        let item = storage.cache.get(&key).unwrap();
        assert!(item.current().value().is_none());
        assert!(item.get_initial_and_last_values().is_none());
        assert_eq!(
            item.current().metadata().last_touched_in_tx,
            Some(TransactionId(1))
        );
        assert_eq!(
            item.initial().metadata().last_touched_in_tx,
            Some(TransactionId(1))
        );
        assert_eq!(storage.cache.iter_altered_since_commit().count(), 0);
    }

    #[test]
    fn touch_impl_on_existing_materialized_entry_creates_history_for_warmth() {
        let mut storage = test_storage();
        let key = test_key(3);
        let expected = Bytes32::from([9u8; 32]);
        let mut resources = TestResources::FORMAL_INFINITE;
        let mut oracle = TestOracle::with_slot(key, expected, false);

        let _ = storage
            .apply_read_impl(
                ExecutionEnvironmentType::NoEE,
                &key,
                &mut resources,
                &mut oracle,
            )
            .unwrap();

        advance_to_next_tx(&mut storage);

        storage.touch_impl(&key, &mut resources).unwrap();

        let item = storage.cache.get(&key).unwrap();
        assert_eq!(item.current().value(), Some(&expected));
        assert!(item.get_initial_and_last_values().is_some());
        assert_eq!(
            item.current().metadata().last_touched_in_tx,
            Some(TransactionId(1))
        );
        assert_eq!(
            item.committed().metadata().last_touched_in_tx,
            Some(TransactionId(0))
        );
        assert_eq!(storage.cache.iter_altered_since_commit().count(), 1);
    }

    #[test]
    fn materialize_element_on_untouched_key_inserts_materialized_cold_entry() {
        let mut storage = test_storage();
        let key = test_key(4);
        let expected = Bytes32::from([5u8; 32]);
        let mut resources = TestResources::FORMAL_INFINITE;
        let mut oracle = TestOracle::with_slot(key, expected, false);

        let (item, is_warm_read) = TestStorage::materialize_element(
            &mut storage.cache,
            &mut storage.resources_policy,
            storage.current_tx_id,
            ExecutionEnvironmentType::EVM,
            &mut resources,
            &key,
            &mut oracle,
        )
        .unwrap();

        assert_eq!(oracle.slot_queries, 1);
        assert!(!is_warm_read.0);
        assert_eq!(item.current().value(), Some(&expected));
        assert!(item.element_properties().is_value_observed());
        assert_eq!(storage.resources_policy.warm_reads.get(), 1);
        assert_eq!(storage.resources_policy.cold_reads.get(), 1);
    }

    #[test]
    fn materialize_element_on_touched_key_is_warm_in_same_tx() {
        let mut storage = test_storage();
        let key = test_key(5);
        let expected = Bytes32::from([6u8; 32]);
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.touch_impl(&key, &mut resources).unwrap();
        let mut oracle = TestOracle::with_slot(key, expected, false);

        let warm_before = storage.resources_policy.warm_reads.get();
        let cold_before = storage.resources_policy.cold_reads.get();

        let (item, is_warm_read) = TestStorage::materialize_element(
            &mut storage.cache,
            &mut storage.resources_policy,
            storage.current_tx_id,
            ExecutionEnvironmentType::EVM,
            &mut resources,
            &key,
            &mut oracle,
        )
        .unwrap();

        assert_eq!(oracle.slot_queries, 1);
        assert!(is_warm_read.0);
        assert_eq!(item.current().value(), Some(&expected));
        assert_eq!(storage.resources_policy.warm_reads.get(), warm_before + 1);
        assert_eq!(storage.resources_policy.cold_reads.get(), cold_before);
    }

    #[test]
    fn materialize_element_on_touched_key_is_cold_in_next_tx() {
        let mut storage = test_storage();
        let key = test_key(6);
        let expected = Bytes32::from([7u8; 32]);
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.touch_impl(&key, &mut resources).unwrap();
        advance_to_next_tx(&mut storage);

        let mut oracle = TestOracle::with_slot(key, expected, false);
        let warm_before = storage.resources_policy.warm_reads.get();
        let cold_before = storage.resources_policy.cold_reads.get();

        let (_item, is_warm_read) = TestStorage::materialize_element(
            &mut storage.cache,
            &mut storage.resources_policy,
            storage.current_tx_id,
            ExecutionEnvironmentType::EVM,
            &mut resources,
            &key,
            &mut oracle,
        )
        .unwrap();

        assert_eq!(oracle.slot_queries, 1);
        assert!(!is_warm_read.0);
        assert_eq!(storage.resources_policy.warm_reads.get(), warm_before + 1);
        assert_eq!(storage.resources_policy.cold_reads.get(), cold_before + 1);
    }

    #[test]
    fn touched_new_slot_must_materialize_to_default_value() {
        let mut storage = test_storage();
        let key = test_key(7);
        let mut resources = TestResources::FORMAL_INFINITE;
        storage.touch_impl(&key, &mut resources).unwrap();

        let mut oracle = TestOracle::with_slot(key, Bytes32::from([1u8; 32]), true);

        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = TestStorage::materialize_element(
                &mut storage.cache,
                &mut storage.resources_policy,
                storage.current_tx_id,
                ExecutionEnvironmentType::NoEE,
                &mut resources,
                &key,
                &mut oracle,
            );
        }));

        assert!(result.is_err());
    }

    #[test]
    fn apply_read_impl_after_touch_returns_materialized_oracle_value() {
        let mut storage = test_storage();
        let key = test_key(8);
        let expected = Bytes32::from([8u8; 32]);
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.touch_impl(&key, &mut resources).unwrap();

        let mut oracle = TestOracle::with_slot(key, expected, false);
        let value = storage
            .apply_read_impl(
                ExecutionEnvironmentType::EVM,
                &key,
                &mut resources,
                &mut oracle,
            )
            .unwrap();

        let item = storage.cache.get(&key).unwrap();
        assert_eq!(oracle.slot_queries, 1);
        assert_eq!(value, expected);
        assert_eq!(item.initial().value(), Some(&expected));
        assert_eq!(item.current().value(), Some(&expected));
        assert!(item.key_properties().is_value_observed());
        assert_eq!(storage.resources_policy.warm_reads.get(), 1);
        assert_eq!(storage.resources_policy.cold_reads.get(), 0);
    }

    #[test]
    fn apply_write_impl_after_touch_uses_materialized_value_for_tx_start_accounting() {
        let mut storage = test_storage();
        let key = test_key(9);
        let initial = Bytes32::from([8u8; 32]);
        let new_value = Bytes32::from([9u8; 32]);
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.touch_impl(&key, &mut resources).unwrap();

        let mut oracle = TestOracle::with_slot(key, initial, false);
        let (old_value, value_at_tx_start) = storage
            .apply_write_impl(
                ExecutionEnvironmentType::EVM,
                &key,
                &new_value,
                &mut oracle,
                &mut resources,
            )
            .unwrap();

        let item = storage.cache.get(&key).unwrap();
        assert_eq!(oracle.slot_queries, 1);
        assert_eq!(old_value, initial);
        assert_eq!(value_at_tx_start, initial);
        assert_eq!(item.committed().value(), Some(&initial));
        assert_eq!(item.current().value(), Some(&new_value));

        let writes = storage.resources_policy.writes.borrow();
        assert_eq!(writes.len(), 1);
        assert_eq!(
            writes[0],
            WriteCharge {
                ee_type: ExecutionEnvironmentType::EVM,
                initial_value: initial,
                current_value: initial,
                new_value,
                is_warm_write: true,
                is_new_slot: false,
            }
        );
        assert_eq!(storage.resources_policy.refunds.get(), 1);
    }

    #[test]
    fn clear_state_impl_skips_unmaterialized_entries_and_zeroes_materialized_ones() {
        let mut storage = test_storage();
        let touched_only = test_key(10);
        let materialized = test_key(11);
        let mut resources = TestResources::FORMAL_INFINITE;

        storage.touch_impl(&touched_only, &mut resources).unwrap();

        let materialized_value = Bytes32::from([3u8; 32]);
        let mut oracle = TestOracle::with_slot(materialized, materialized_value, false);
        let _ = storage
            .apply_read_impl(
                ExecutionEnvironmentType::NoEE,
                &materialized,
                &mut resources,
                &mut oracle,
            )
            .unwrap();

        storage
            .clear_state_impl(AddressArg(touched_only.address))
            .unwrap();

        let touched_item = storage.cache.get(&touched_only).unwrap();
        assert!(touched_item.current().value().is_none());

        let materialized_item = storage.cache.get(&materialized).unwrap();
        assert_eq!(materialized_item.current().value(), Some(&Bytes32::ZERO));
        assert_eq!(
            materialized_item.committed().value(),
            Some(&materialized_value)
        );
    }
}
