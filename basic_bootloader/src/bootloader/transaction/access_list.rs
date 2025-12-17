use super::Transaction;
use crate::bootloader::errors::TxError;
use zk_ee::system::metadata::basic_metadata::ZkSpecificPricingMetadata;
use zk_ee::{
    execution_environment_type::ExecutionEnvironmentType,
    system::{EthereumLikeTypes, IOSubsystemExt, System},
    utils::Bytes32,
};

/// Parse and warm up accounts and storage slots from the access list.
///
/// Touches all accounts and storage keys in the access list so they are hot
/// before execution.
///
/// Returns Ok on success, or `TxError` if an IO operation fails.
pub fn parse_and_warm_up_access_list<S: EthereumLikeTypes>(
    system: &mut System<S>,
    resources: &mut S::Resources,
    transaction: &Transaction<S::Allocator>,
) -> Result<(), TxError>
where
    S::IO: IOSubsystemExt,
    S::Metadata: ZkSpecificPricingMetadata,
{
    use crate::bootloader::transaction::rlp_encoded::AccessListForAddress;
    if let Some(iter) = transaction.access_list_iter() {
        for AccessListForAddress {
            address,
            slots_list,
        } in iter
        {
            system
                .io
                .touch_account(ExecutionEnvironmentType::NoEE, resources, &address, true)?;
            for key in slots_list.iter() {
                let key = key?;
                system.io.storage_touch(
                    ExecutionEnvironmentType::NoEE,
                    resources,
                    &address,
                    &Bytes32::from_array(*key),
                    true,
                )?;
            }
        }
    }

    Ok(())
}
