//! Storage cache, backed by a history map.
use core::alloc::Allocator;
use storage_models::common_structs::snapshottable_io::SnapshottableIo;
use storage_models::common_structs::StorageCacheModel;
use zk_ee::common_structs::StorageInitialAppearance;
use zk_ee::execution_environment_type::ExecutionEnvironmentType;
use zk_ee::system::errors::internal::InternalError;
use zk_ee::{
    common_structs::{WarmStorageKey, WarmStorageValue},
    memory::stack_trait::StackCtor,
    system::{errors::system::SystemError, Resources},
    system_io_oracle::IOOracle,
    types_config::{EthereumIOTypesConfig, SystemIOTypesConfig},
    utils::Bytes32,
};

use crate::system_implementation::cache_structs::storage_values::*;

pub struct EthereumStorageCache<
    A: Allocator + Clone,
    SC: StackCtor<N>,
    const N: usize,
    R: Resources,
    P: StorageAccessPolicy<R, Bytes32>,
> {
    pub(crate) slot_values:
        GenericPubdataAwareStorageValuesCache<WarmStorageKey, Bytes32, A, SC, N, R, P>,
}

impl<
        A: Allocator + Clone,
        SC: StackCtor<N>,
        const N: usize,
        R: Resources,
        P: StorageAccessPolicy<R, Bytes32>,
    > StorageCacheModel for EthereumStorageCache<A, SC, N, R, P>
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

        self.slot_values
            .apply_read_impl(ee_type, &key, resources, oracle, false)
    }

    fn touch(
        &mut self,
        ee_type: ExecutionEnvironmentType,
        resources: &mut Self::Resources,
        address: &<Self::IOTypes as SystemIOTypesConfig>::Address,
        key: &<Self::IOTypes as SystemIOTypesConfig>::StorageKey,
        oracle: &mut impl IOOracle,
        is_access_list: bool,
    ) -> Result<(), SystemError> {
        // TODO(EVM-1076): use a different low-level function to avoid creating pubdata
        // and merkle proof obligations until we actually read the value

        let key = WarmStorageKey {
            address: *address,
            key: *key,
        };

        if is_access_list {
            self.slot_values
                .mark_access_list_slot(ee_type, resources, &key)?;
        } else {
            self.slot_values
                .apply_read_impl(ee_type, &key, resources, oracle, false)?;
        }

        Ok(())
    }

    fn write(
        &mut self,
        ee_type: ExecutionEnvironmentType,
        resources: &mut Self::Resources,
        address: &<Self::IOTypes as SystemIOTypesConfig>::Address,
        key: &<Self::IOTypes as SystemIOTypesConfig>::StorageKey,
        new_value: &<Self::IOTypes as SystemIOTypesConfig>::StorageValue,
        oracle: &mut impl IOOracle,
    ) -> Result<<Self::IOTypes as SystemIOTypesConfig>::StorageKey, SystemError> {
        let key = WarmStorageKey {
            address: *address,
            key: *key,
        };

        let old_value = self
            .slot_values
            .apply_write_impl(ee_type, &key, new_value, oracle, resources)?;

        Ok(old_value)
    }

    fn read_special_account_property<T: storage_models::common_structs::SpecialAccountProperty>(
        &mut self,
        _ee_type: ExecutionEnvironmentType,
        _resources: &mut Self::Resources,
        _address: &<Self::IOTypes as SystemIOTypesConfig>::Address,
        _oracle: &mut impl IOOracle,
    ) -> Result<T::Value, SystemError> {
        panic!("unreachable for such cache");
    }

    fn write_special_account_property<T: storage_models::common_structs::SpecialAccountProperty>(
        &mut self,
        _ee_type: ExecutionEnvironmentType,
        _resources: &mut Self::Resources,
        _address: &<Self::IOTypes as SystemIOTypesConfig>::Address,
        _new_value: &T::Value,
        _oracle: &mut impl IOOracle,
    ) -> Result<T::Value, SystemError> {
        panic!("unreachable for such cache");
    }
}

impl<
        A: Allocator + Clone,
        SC: StackCtor<N>,
        const N: usize,
        R: Resources,
        P: StorageAccessPolicy<R, Bytes32>,
    > SnapshottableIo for EthereumStorageCache<A, SC, N, R, P>
{
    type StateSnapshot = StorageSnapshotId;

    fn begin_new_tx(&mut self) {
        self.slot_values.begin_new_tx();
    }

    fn start_frame(&mut self) -> Self::StateSnapshot {
        self.slot_values.start_frame()
    }

    fn finish_frame(
        &mut self,
        rollback_handle: Option<&Self::StateSnapshot>,
    ) -> Result<(), InternalError> {
        self.slot_values.finish_frame_impl(rollback_handle)
    }
}

impl<
        A: Allocator + Clone,
        SC: StackCtor<N>,
        const N: usize,
        R: Resources,
        P: StorageAccessPolicy<R, Bytes32>,
    > EthereumStorageCache<A, SC, N, R, P>
{
    pub(crate) fn iter_as_storage_types(
        &self,
    ) -> impl Iterator<Item = (WarmStorageKey, WarmStorageValue)> + Clone + use<'_, A, SC, N, R, P>
    {
        self.slot_values.cache.iter().map(|item| {
            use zk_ee::common_structs::StorageCurrentAppearance;

            let initial_appearance = item.key_properties().initial_appearance();
            let current_record = item.current();
            let initial_record = item.initial();
            let initial_value_used = matches!(
                item.key_properties().current_appearance(),
                StorageCurrentAppearance::Observed
                    | StorageCurrentAppearance::Updated
                    | StorageCurrentAppearance::Deleted
            );
            (
                *item.key(),
                // Using the WarmStorageValue temporarily till it's outed from the codebase. We're
                // not actually 'using' it.
                WarmStorageValue {
                    current_value: *current_record.value(),
                    is_new_storage_slot: initial_appearance == StorageInitialAppearance::Empty,
                    initial_value: *initial_record.value(),
                    initial_value_used,
                    ..Default::default()
                },
            )
        })
    }
    ///
    /// Returns all the accessed storage slots.
    ///
    /// This one should be used for merkle proof validation, includes initial reads.
    ///
    pub fn net_accesses_iter(
        &self,
    ) -> impl Iterator<Item = (WarmStorageKey, WarmStorageValue)> + Clone + use<'_, A, SC, N, R, P>
    {
        self.iter_as_storage_types()
    }

    ///
    /// Returns slots that were changed during execution.
    ///
    pub fn net_diffs_iter(
        &self,
    ) -> impl Iterator<Item = (WarmStorageKey, WarmStorageValue)> + use<'_, A, SC, N, R, P> {
        self.iter_as_storage_types()
            .filter(|(_, v)| v.current_value != v.initial_value)
    }

    pub fn calculate_pubdata_used_by_tx(&self) -> u32 {
        0
    }
}
