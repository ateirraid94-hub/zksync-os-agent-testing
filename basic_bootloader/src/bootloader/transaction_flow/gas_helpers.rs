use crate::require;
use evm_interpreter::ERGS_PER_GAS;
use zk_ee::internal_error;
use zk_ee::system::errors::internal::InternalError;
use zk_ee::system::Resource;
use zk_ee::system::{Computational, Resources};
use zk_ee::utils::u256_to_u64_saturated;

use super::super::*;

pub struct ResourcesForTx<S: EthereumLikeTypes> {
    // Resources to run the transaction.
    // These will be capped to MAX_NATIVE_COMPUTATIONAL, to prevent
    // transaction from using too many native computational resources.
    pub main_resources: S::Resources,
    /// Resources in excess of MAX_NATIVE_COMPUTATIONAL.
    /// These resources can only be used for paying for pubdata.
    pub withheld: S::Resources,
    /// Computational native charged for as intrinsic
    pub intrinsic_computational_native_charged: u64,
}

impl<S: EthereumLikeTypes> core::fmt::Debug for ResourcesForTx<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ResourcesForTx")
            .field("gas", &(self.main_resources.ergs().0 / ERGS_PER_GAS))
            .field("main_resources", &self.main_resources)
            .field("withheld", &self.withheld)
            .field(
                "intrinsic_computational_native_charged",
                &self.intrinsic_computational_native_charged,
            )
            .finish()
    }
}

///
/// Get current pubdata spent and ergs to be charged for it.
/// If base_pubdata is Some, it's discounted from the current
/// pubdata counter.
/// Note: if base_pubdata is greater than the current counter, this function
/// returns 0.
///
pub fn get_resources_to_charge_for_pubdata<S: EthereumLikeTypes>(
    system: &mut System<S>,
    native_per_pubdata: U256,
    base_pubdata: Option<u64>,
) -> Result<(u64, S::Resources), InternalError> {
    let current_pubdata_spent = system
        .net_pubdata_used()?
        .saturating_sub(base_pubdata.unwrap_or(0));
    let native_per_pubdata = u256_to_u64_saturated(&native_per_pubdata);
    let native = current_pubdata_spent
        .checked_mul(native_per_pubdata)
        .ok_or(internal_error!("cps*epp"))?;
    let native = <S::Resources as zk_ee::system::Resources>::Native::from_computational(native);
    Ok((current_pubdata_spent, S::Resources::from_native(native)))
}

///
/// Checks if the remaining resources are sufficient to pay for the
/// spent pubdata.
/// If base_pubdata is Some, it's discounted from the current
/// pubdata counter.
/// Returns if the check succeeded, the resources to charge
/// for pubdata and the net pubdata used.
///
pub fn check_enough_resources_for_pubdata<S: EthereumLikeTypes>(
    system: &mut System<S>,
    native_per_pubdata: U256,
    resources: &S::Resources,
    base_pubdata: Option<u64>,
) -> Result<(bool, S::Resources, u64), InternalError> {
    let (pubdata_used, resources_for_pubdata) =
        get_resources_to_charge_for_pubdata(system, native_per_pubdata, base_pubdata)?;
    let _ = system.get_logger().write_fmt(format_args!(
        "Checking gas for pubdata, resources_for_pubdata: {resources_for_pubdata:?}, resources: {resources:?}\n"
    ));
    let enough = resources.has_enough(&resources_for_pubdata);
    Ok((enough, resources_for_pubdata, pubdata_used))
}

pub(crate) fn get_gas_price<S: EthereumLikeTypes>(
    system: &mut System<S>,
    max_fee_per_gas: &U256,
    max_priority_fee_per_gas: Option<&U256>,
) -> Result<U256, TxError> {
    let base_fee = system.get_eip1559_basefee();
    let max_priority_fee_per_gas = max_priority_fee_per_gas.unwrap_or(max_fee_per_gas);
    require!(
        max_priority_fee_per_gas <= max_fee_per_gas,
        TxError::Validation(InvalidTransaction::PriorityFeeGreaterThanMaxFee,),
        system
    )?;
    require!(
        &base_fee <= max_fee_per_gas,
        TxError::Validation(InvalidTransaction::BaseFeeGreaterThanMaxFee,),
        system
    )?;
    let priority_fee_per_gas = (*max_priority_fee_per_gas).min(max_fee_per_gas - base_fee);
    Ok(base_fee + priority_fee_per_gas)
}
