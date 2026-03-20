use crate::bootloader::block_flow::read_settlement_layer_chain_id;
use crate::bootloader::config::BasicBootloaderExecutionConfig;
use crate::bootloader::constants::{
    FREE_L1_TX_NATIVE_PER_GAS, L1_TX_INTRINSIC_L2_GAS, L1_TX_INTRINSIC_NATIVE_COST,
    L1_TX_INTRINSIC_PUBDATA, L1_TX_NATIVE_PRICE,
};
use crate::bootloader::errors::BootloaderInterfaceError;
use crate::bootloader::errors::TxError;
use crate::bootloader::runner::RunnerMemoryBuffers;
use crate::bootloader::transaction::abi_encoded::AbiEncodedTransaction;
use crate::bootloader::transaction_flow::gas_helpers::{
    check_enough_resources_for_pubdata, create_resources_for_tx,
    get_resources_to_charge_for_pubdata, L1ResourcesPolicy, ResourcesForTx,
};
use crate::bootloader::transaction_flow::refund_calculation::{compute_gas_refund, RefundInfo};
use crate::bootloader::transaction_flow::{ExecutionOutput, ExecutionResult};
use crate::bootloader::{BasicBootloader, BootloaderSubsystemError};
use crate::require_internal;
use arrayvec::ArrayVec;
use core::fmt::Write;
use ruint::aliases::{B160, U256};
use zk_ee::common_structs::system_hooks::HooksStorage;
use zk_ee::execution_environment_type::ExecutionEnvironmentType;
use zk_ee::system::errors::root_cause::GetRootCause;
use zk_ee::system::errors::root_cause::RootCause;
use zk_ee::system::errors::runtime::RuntimeError;
use zk_ee::system::errors::subsystem::SubsystemError;
use zk_ee::system::metadata::basic_metadata::{BasicMetadata, ZkSpecificPricingMetadata};
use zk_ee::system::metadata::zk_metadata::TxLevelMetadata;
use zk_ee::system::tracer::Tracer;
use zk_ee::system::validator::TxValidator;
use zk_ee::system::Resource;
use zk_ee::system::System;
use zk_ee::system::{CompletedExecution, Computational};
use zk_ee::system::{EthereumLikeTypes, Resources};
#[allow(unused_imports)]
use zk_ee::system::{IOSubsystem, IOSubsystemExt, MAX_NATIVE_COMPUTATIONAL};
use zk_ee::system_log;
use zk_ee::utils::{u256_to_b160_checked, u256_try_to_u64, Bytes32};
use zk_ee::{interface_error, internal_error, wrap_error};

use system_hooks::addresses_constants::{L2_ASSET_TRACKER_ADDRESS, L2_BASE_TOKEN_ADDRESS};

use super::validation_impl::compute_calldata_tokens;
use super::{ZkTransactionFlowOnlyEOA, ZkTxResult};

pub(crate) fn process_l1_transaction<
    'a,
    S: EthereumLikeTypes + 'a,
    Config: BasicBootloaderExecutionConfig,
>(
    system: &mut System<S>,
    system_functions: &mut HooksStorage<S, S::Allocator>,
    memories: RunnerMemoryBuffers<'a>,
    transaction: &AbiEncodedTransaction<S::Allocator>,
    is_priority_op: bool,
    tracer: &mut impl Tracer<S>,
    validator: &mut impl TxValidator<S>,
) -> Result<ZkTxResult<'a>, BootloaderSubsystemError>
where
    S::IO: IOSubsystemExt,
    S::Metadata: ZkSpecificPricingMetadata
        + BasicMetadata<S::IOTypes, TransactionMetadata = TxLevelMetadata<S::IOTypes>>,
{
    // The work done by the bootloader (outside of EE or EOA specific
    // computation) is charged as part of the intrinsic gas cost.
    let gas_limit = transaction.gas_limit.read();

    // The invariant that the user deposited more than the value needed
    // for the transaction must be enforced on L1, but we double-check it here
    // Note, that for now the property of block.base <= tx.maxFeePerGas does not work
    // for L1->L2 transactions. For now, these transactions are processed with the same gasPrice
    // they were provided on L1. In the future, we may apply a new logic for it.
    let gas_price = transaction.max_fee_per_gas.read();

    // For L1->L2 transactions we always use the pubdata price provided by the transaction.
    // This is needed to ensure DDoS protection. All the excess expenditure
    // will be refunded to the user.
    let gas_per_pubdata = transaction.gas_per_pubdata_limit.read();

    // Compute resource and fee information, making sure we handle
    // all possible validation errors carefully.
    // L1 transactions cannot be invalidated. Therefore, the following
    // function makes sure L1 transactions are processable even when
    // some checks that should be performed by the L1 don't hold.
    let ResourceAndFeeInfo {
        resources:
            ResourcesForTx {
                main_resources: mut resources,
                withheld: withheld_resources,
                intrinsic_computational_native_charged,
            },
        native_per_gas,
        native_per_pubdata,
        minimal_gas_used,
    } = prepare_and_check_resources::<S, Config>(
        system,
        transaction,
        is_priority_op,
        gas_limit,
        gas_price,
        gas_per_pubdata,
    )?;

    // Just used for computing native used
    let initial_resources = resources.clone();

    let tx_internal_cost = gas_price
        .checked_mul(U256::from(gas_limit))
        .ok_or(internal_error!("gp*gl"))?;
    let value = transaction.value.read();
    let total_deposited = transaction.reserved[0].read();
    require_internal!(
        total_deposited >= tx_internal_cost,
        "Deposited amount too low",
        system
    )?;

    // TODO: l1 transaction preparation (marking factory deps)

    let (tx_hash, preparation_out_of_resources): (Bytes32, bool) =
        match transaction.calculate_hash(&mut resources) {
            Ok(h) => (h.into(), false),
            Err(e) => {
                match e {
                    TxError::Internal(e) if !matches!(e.root_cause(), RootCause::Runtime(_)) => {
                        return Err(e);
                    }
                    // Only way hashing of L1 tx can fail due to Validation or Runtime is
                    // due to running out of native.
                    _ => {
                        system_log!(
                            system,
                            "Transaction preparation exhausted native resources: {e:?}\n"
                        );

                        resources.exhaust_ergs();
                        // We need to compute the hash anyways, we do with inf resources
                        let mut inf_resources = S::Resources::FORMAL_INFINITE;
                        (
                            transaction
                                .calculate_hash(&mut inf_resources)
                                .expect("must succeed")
                                .into(),
                            true,
                        )
                    }
                }
            }
        };

    // pubdata_info = (pubdata_used, to_charge_for_pubdata) can be cached
    // to used in the refund step only if the execution succeeded.
    // Otherwise, this value needs to be recomputed after reverting
    // state changes.
    let (is_success, pubdata_info, resources_before_refund, mut memories) =
        if !preparation_out_of_resources {
            // Take a snapshot in case we need to revert due to out of native.
            let rollback_handle = system.start_global_frame()?;

            // Tx execution
            let from = transaction.from.read();
            let to = transaction.to.read();
            match execute_l1_transaction_and_notify_result::<S, Config>(
                system,
                system_functions,
                memories,
                &transaction,
                from,
                to,
                value,
                native_per_pubdata,
                &mut resources,
                withheld_resources,
                tracer,
                validator,
            ) {
                Ok((outcome, returned_memories)) => {
                    let pubdata_info = if outcome.is_success {
                        system.finish_global_frame(None)?;
                        Some((outcome.pubdata_used, outcome.to_charge_for_pubdata))
                    } else {
                        system.finish_global_frame(Some(&rollback_handle))?;
                        None
                    };
                    (
                        outcome.is_success,
                        pubdata_info,
                        outcome.resources_before_refund,
                        returned_memories,
                    )
                }
                Err(e) => return Err(e),
            }
        } else {
            (false, None, S::Resources::empty(), memories)
        };

    // Compute gas to refund
    // TODO: consider operator refund
    #[allow(unused_variables)]
    let (pubdata_used, to_charge_for_pubdata) = match pubdata_info {
        Some(r) => r,
        None => get_resources_to_charge_for_pubdata(system, native_per_pubdata, None)?,
    };

    #[allow(unused_variables)]
    let RefundInfo {
        gas_used,
        evm_refund,
        native_used,
    } = compute_gas_refund(
        system,
        to_charge_for_pubdata,
        gas_limit,
        minimal_gas_used,
        native_per_gas,
        &mut resources,
    )?;

    // Transfer fee from treasury to operator
    // We already checked that total_gas_refund <= gas_limit
    let pay_to_operator = U256::from(gas_used)
        .checked_mul(U256::from(gas_price))
        .ok_or(internal_error!("gu*gp"))?;
    // Use FORMAL_INFINITE for post-execution operations (coinbase transfer,
    // asset tracker notifications, refund transfer, log emission).
    // These cannot fail due to resource exhaustion. We measure the native
    // consumed via the diff on inf_resources and include it in
    // computational_native_used so that block seal criteria remain accurate.
    let mut inf_resources = S::Resources::FORMAL_INFINITE;
    let inf_resources_initial = inf_resources.clone();

    let coinbase = system.get_coinbase();
    transfer_from_treasury::<S>(
        system,
        &pay_to_operator,
        &coinbase,
        &mut inf_resources,
        Config::SIMULATION,
    )
    .map_err(|e| match e.root_cause() {
        RootCause::Runtime(RuntimeError::OutOfErgs(_)) => {
            internal_error!("Out of ergs on infinite ergs").into()
        }
        RootCause::Runtime(RuntimeError::FatalRuntimeError(_)) => {
            internal_error!("Out of native on infinite").into()
        }
        _ => e,
    })?;

    // Notify asset tracker about the operator fee portion of the deposit.
    notify_l2_asset_tracker::<S>(
        system,
        system_functions,
        memories.reborrow(),
        pay_to_operator,
        &mut inf_resources,
        tracer,
        validator,
    )?;

    // Refund
    let to_refund_recipient = if !is_success {
        // Upgrade transactions must always succeed
        if !is_priority_op {
            return Err(internal_error!("Upgrade transaction must succeed").into());
        }
        // If the transaction reverts, then the minting of the deposit
        // reverted too. Thus, we need to refund the entire deposit minus
        // the fee (`pay_to_operator`).
        total_deposited
            .checked_sub(pay_to_operator)
            .ok_or(internal_error!("td-pto"))
    } else {
        // If the transaction succeeds, then it is assumed that the
        // mint to `from` address was transferred correctly too.
        // In this case, we just refund the unused gas that the
        // transaction paid for initially.
        let prepaid_fee = gas_price
            .checked_mul(U256::from(transaction.gas_limit.read()))
            .ok_or(internal_error!("gp*gl"))?;
        prepaid_fee
            .checked_sub(pay_to_operator)
            .ok_or(internal_error!("pf-pto"))
    }?;
    if to_refund_recipient > U256::ZERO {
        let refund_recipient = u256_to_b160_checked(transaction.reserved[1].read());
        transfer_from_treasury::<S>(
            system,
            &to_refund_recipient,
            &refund_recipient,
            &mut inf_resources,
            Config::SIMULATION,
        )
        .map_err(|e| -> BootloaderSubsystemError {
            match e.root_cause() {
                RootCause::Runtime(RuntimeError::OutOfErgs(_)) => {
                    internal_error!("Out of ergs on infinite ergs").into()
                }
                RootCause::Runtime(RuntimeError::FatalRuntimeError(_)) => {
                    internal_error!("Out of native on infinite").into()
                }
                _ => e,
            }
        })?;
    }

    // Notify asset tracker about the refund portion of the deposit.
    notify_l2_asset_tracker::<S>(
        system,
        system_functions,
        memories.reborrow(),
        to_refund_recipient,
        &mut inf_resources,
        tracer,
        validator,
    )?;

    // Emit log
    // We don't send logs for upgrade txs by protocol convention
    if is_priority_op {
        system.io.emit_l1_l2_tx_log(
            ExecutionEnvironmentType::NoEE,
            &mut inf_resources,
            tx_hash,
            is_success,
        )?;
    }

    // Native consumed by post-execution operations (coinbase transfer,
    // asset tracker notifications, refund, log emission).
    let post_execution_native_used = inf_resources.diff(inf_resources_initial).native().as_u64();

    // Add back the intrinsic native charged in get_resources_for_tx,
    // as initial_resources doesn't include them.
    let computational_native_used = resources_before_refund
        .diff(initial_resources)
        .native()
        .as_u64()
        + intrinsic_computational_native_charged
        + post_execution_native_used;

    // Construct the execution result. The returndata from the main tx
    // body has been overwritten by the asset-tracker notification calls,
    // so we use an empty slice. L1 transaction returndata is only
    // informational (it appears in receipts) and is not consumed by
    // any on-chain logic.
    let result = if is_success {
        ExecutionResult::Success {
            output: ExecutionOutput::Call(&[]),
        }
    } else {
        ExecutionResult::Revert { output: &[] }
    };

    Ok(ZkTxResult {
        result,
        tx_hash,
        is_priority_tx: is_priority_op,
        is_upgrade_tx: !is_priority_op,
        is_service_tx: false,
        gas_used,
        gas_refunded: evm_refund,
        computational_native_used,
        native_used,
        pubdata_used: pubdata_used + L1_TX_INTRINSIC_PUBDATA,
        blob_gas_used: 0,
    })
}

struct ResourceAndFeeInfo<S: EthereumLikeTypes> {
    resources: ResourcesForTx<S>,
    native_per_pubdata: u64,
    native_per_gas: u64,
    minimal_gas_used: u64,
}

///
/// Compute and perform some checks on fee/resource parameters.
/// This function handles cases that for L2 transactions would be
/// validation errors, as "invalidating" an L1 transaction can halt
/// the chain (due to the priority queue).
/// Note that the "validation errors" are practically unreachable, as
/// gas_limit, gas_price and gas_per_pubdata are either checked or set
/// by the L1 contracts. We decide to handle these cases as a fallback in
/// case the L1 contracts aren't properly updated to reflect a change in
/// ZKsync OS.
/// The approach is to use saturating arithmetic and emit a system
/// log if this situation ever happens.
///
fn prepare_and_check_resources<
    'a,
    S: EthereumLikeTypes + 'a,
    Config: BasicBootloaderExecutionConfig,
>(
    system: &mut System<S>,
    transaction: &AbiEncodedTransaction<S::Allocator>,
    is_priority_op: bool,
    gas_limit: u64,
    gas_price: U256,
    gas_per_pubdata: u32,
) -> Result<ResourceAndFeeInfo<S>, BootloaderSubsystemError>
where
    S::IO: IOSubsystemExt,
    S::Metadata: ZkSpecificPricingMetadata
        + BasicMetadata<S::IOTypes, TransactionMetadata = TxLevelMetadata<S::IOTypes>>,
{
    // For L1->L2 txs, we use a constant native price to avoid censorship.
    let native_price = L1_TX_NATIVE_PRICE;
    let native_per_gas = if is_priority_op {
        if gas_price.is_zero() {
            if Config::SIMULATION {
                u256_try_to_u64(&system.get_eip1559_basefee().div_ceil(native_price))
                    .unwrap_or_else(|| {
                        system_log!(
                            system,
                            "Native per gas calculation for L1 tx overflows, using saturated arithmetic instead");
                        u64::MAX
                    })
            } else {
                FREE_L1_TX_NATIVE_PER_GAS
            }
        } else {
            u256_try_to_u64(&gas_price.div_ceil(native_price)).unwrap_or_else(|| {
                system_log!(
                    system,
                    "Native per gas calculation for L1 tx overflows, using saturated arithmetic instead");
                u64::MAX
            })
        }
    } else {
        // Upgrade txs are paid by the protocol, so we use a fixed native per gas
        FREE_L1_TX_NATIVE_PER_GAS
    };

    let native_per_pubdata = (gas_per_pubdata as u64)
        .checked_mul(native_per_gas)
        .unwrap_or_else(|| {
            system_log!(
                system,
                "Native per pubdata calculation for L1 tx overflows, using saturated arithmetic instead");
                u64::MAX
        });

    let native_prepaid_from_gas = native_per_gas.checked_mul(gas_limit)
        .unwrap_or_else(|| {
            system_log!(
                system,
                "Native prepaid from gas calculation for L1 tx overflows, using saturated arithmetic instead");
                u64::MAX
        });

    let (calldata_tokens, minimal_gas_used) =
        compute_calldata_tokens(system, transaction.calldata(), true);

    // With L1ResourcesPolicy, this returns Result<ResourcesForTx<S>, BootloaderSubsystemError>
    // Validation errors are type-safe impossible - they're logged and saturated instead
    let resources = create_resources_for_tx::<S, L1ResourcesPolicy>(
        system,
        gas_limit,
        native_per_gas == 0,
        native_prepaid_from_gas,
        native_per_pubdata,
        false, // is_deployment
        transaction.calldata().len() as u64,
        calldata_tokens,
        L1_TX_INTRINSIC_L2_GAS,
        L1_TX_INTRINSIC_PUBDATA,
        L1_TX_INTRINSIC_NATIVE_COST,
    )?;

    // L1 transactions might have a gas limit < minimal_gas_used. This should be
    // prevented by L1 validation, but we log and saturate if it happens.
    if gas_limit < minimal_gas_used {
        system_log!(
            system,
            "L1 tx gas limit below intrinsic cost, using saturated arithmetic instead"
        );
    }
    // Pick the min to keep processing L1 txs even if the L1 validation is wrong.
    let minimal_gas_used = minimal_gas_used.min(gas_limit);

    Ok(ResourceAndFeeInfo {
        resources,
        native_per_pubdata,
        native_per_gas,
        minimal_gas_used,
    })
}

/// Outcome of executing the L1 transaction body.
///
/// This deliberately does NOT carry `ExecutionResult<'a>` (which borrows
/// returndata from the runner memory buffers). Keeping the buffers
/// un-borrowed lets `process_l1_transaction` reborrow them for the
/// post-execution asset-tracker notification calls.
struct L1ExecutionOutcome<S: EthereumLikeTypes> {
    is_success: bool,
    pubdata_used: u64,
    to_charge_for_pubdata: S::Resources,
    resources_before_refund: S::Resources,
}

// Returns (outcome, memories) — memories is returned so the caller can
// reborrow it for subsequent `run_single_interaction` calls.
fn execute_l1_transaction_and_notify_result<
    'a,
    S: EthereumLikeTypes + 'a,
    Config: BasicBootloaderExecutionConfig,
>(
    system: &mut System<S>,
    system_functions: &mut HooksStorage<S, S::Allocator>,
    mut memories: RunnerMemoryBuffers<'a>,
    transaction: &AbiEncodedTransaction<S::Allocator>,
    from: B160,
    to: B160,
    value: U256,
    native_per_pubdata: u64,
    resources: &mut S::Resources,
    withheld_resources: S::Resources,
    tracer: &mut impl Tracer<S>,
    validator: &mut impl TxValidator<S>,
) -> Result<(L1ExecutionOutcome<S>, RunnerMemoryBuffers<'a>), BootloaderSubsystemError>
where
    S::IO: IOSubsystemExt,
    S::Metadata: ZkSpecificPricingMetadata
        + BasicMetadata<S::IOTypes, TransactionMetadata = TxLevelMetadata<S::IOTypes>>,
{
    system_log!(system, "Executing L1 transaction\n");

    let gas_price = U256::from(transaction.max_fee_per_gas.read());
    system.set_tx_context(TxLevelMetadata {
        tx_gas_price: gas_price,
        tx_origin: from,
        blobs: ArrayVec::new(),
    });

    // Start a frame, to revert minting of value if execution fails
    let rollback_handle = system.start_global_frame()?;

    // Fee payment is done in two steps.
    // The first step is here, where the max fee (gas limit * gas price)
    // is committed to.
    // This fee is deducted from the deposit to be minted (transferred from
    // the treasury).
    // After the execution of the transaction, the actual fee
    // (gas used * gas price) is paid to the operator, while the
    // rest of the max fee is refunded.
    let max_fee_commitment = gas_price
        .checked_mul(U256::from(transaction.gas_limit.read()))
        .ok_or(internal_error!("gp*gl"))?;
    let total_deposited = transaction.reserved[0].read();
    let to_transfer = total_deposited
        .checked_sub(max_fee_commitment)
        .ok_or(internal_error!("mfc+tic"))?;

    // Transfer value from treasury to sender (the deposit minus max fee).
    // We want to ensure that the simulation of a transaction
    // never underestimates gas/pubdata compared to the actual execution
    // of said transaction.
    // During simulation the gas price is typically set to 0. So we need
    // to be conservative about operations that incur in gas/pubdata depending
    // on the value of the fee. For that reason, we always perform the
    // following transfer on simulation, and avoid compressing the pubdata
    // for the balance changes resulting from it.
    if to_transfer > U256::ZERO || Config::SIMULATION {
        resources
            .with_infinite_ergs(|inf_resources| {
                transfer_from_treasury::<S>(
                    system,
                    &to_transfer,
                    &from,
                    inf_resources,
                    Config::SIMULATION,
                )
            })
            .map_err(|e| match e.root_cause() {
                RootCause::Runtime(RuntimeError::OutOfErgs(_)) => {
                    system_log!(
                        system,
                        "Out of ergs on infinite ergs: inner error was {e:?}"
                    );
                    BootloaderSubsystemError::LeafDefect(internal_error!(
                        "Out of ergs on infinite ergs"
                    ))
                }
                _ => e,
            })?;
    }

    // Notify L2AssetTracker about the value mint portion of the deposit.
    // This is inside the execution frame, so it gets rolled back if the
    // main tx body reverts — matching the treasury transfer above.
    notify_l2_asset_tracker::<S>(
        system,
        system_functions,
        memories.reborrow(),
        to_transfer,
        resources,
        tracer,
        validator,
    )?;

    let resources_for_tx = resources.clone();

    // transaction is in managed region, so we can recast it back
    let calldata = transaction.calldata();

    // TODO: add support for deployment transactions,
    // probably unify with execution logic for EOA

    let reverted = match BasicBootloader::<S, ZkTransactionFlowOnlyEOA<S>>::run_single_interaction(
        system,
        system_functions,
        memories.reborrow(),
        calldata,
        &from,
        &to,
        resources_for_tx,
        &value,
        false,
        tracer,
        validator,
    ) {
        Ok(CompletedExecution {
            resources_returned,
            result,
        }) => {
            let reverted = result.failed();
            *resources = resources_returned;
            system.finish_global_frame(reverted.then_some(&rollback_handle))?;
            reverted
        }
        // Handle out-of-native / out-of-memory as a top-level revert so that
        // `memories` is always returned to the caller for post-execution
        // asset-tracker notifications.
        Err(e) => match e.root_cause() {
            RootCause::Runtime(RuntimeError::FatalRuntimeError(re)) => {
                system_log!(
                    system,
                    "L1 transaction ran out of native resources or memory {re:?}\n"
                );
                resources.exhaust_ergs();
                system.finish_global_frame(Some(&rollback_handle))?;
                true
            }
            _ => return Err(e),
        },
    };

    system_log!(system, "Main TX body successful = {}\n", !reverted);

    // Just used for computing native used
    // Needs to use the resources before we reclaim withheld
    let resources_before_refund = resources.clone();

    // After the transaction is executed, we reclaim the withheld resources.
    // This is needed to ensure correct "gas_used" calculation, also these
    // resources could be spent for pubdata.
    resources.reclaim_withheld(withheld_resources);

    let (enough, to_charge_for_pubdata, pubdata_used) =
        check_enough_resources_for_pubdata(system, native_per_pubdata, resources, None)?;
    let is_success = !reverted && enough;
    if !enough {
        system_log!(system, "Not enough gas for pubdata after execution\n");
    }

    Ok((
        L1ExecutionOutcome {
            is_success,
            pubdata_used,
            to_charge_for_pubdata,
            resources_before_refund,
        },
        memories,
    ))
}

/// Transfers [value] from the treasury account to address [to].
///
/// Returns `TreasuryTransferFailed` if:
/// - Treasury has insufficient balance
/// - Balance overflow occurs
pub fn transfer_from_treasury<'a, S: EthereumLikeTypes + 'a>(
    system: &mut System<S>,
    nominal_token_value: &U256,
    to: &B160,
    resources: &mut S::Resources,
    fee_payment_in_simulation: bool,
) -> Result<(), BootloaderSubsystemError>
where
    S::IO: IOSubsystemExt,
{
    system_log!(
        system,
        "Transferring {nominal_token_value:?} tokens from treasury to {to:?}\n"
    );

    let treasury_address = &system_hooks::addresses_constants::BASE_TOKEN_HOLDER_ADDRESS;

    let _ = system
        .io
        .update_account_nominal_token_balance(
            zk_ee::execution_environment_type::ExecutionEnvironmentType::EVM,
            resources,
            treasury_address,
            nominal_token_value,
            true, // true = subtract from balance
            fee_payment_in_simulation,
        )
        .map_err(|e| -> BootloaderSubsystemError {
            match e {
                SubsystemError::LeafUsage(balance_error) => {
                    system_log!(system, "Treasury transfer failed: {balance_error:?}");
                    interface_error!(BootloaderInterfaceError::TreasuryTransferFailed)
                }
                _ => wrap_error!(e),
            }
        })?;

    let _ = system
        .io
        .update_account_nominal_token_balance(
            zk_ee::execution_environment_type::ExecutionEnvironmentType::EVM,
            resources,
            to,
            nominal_token_value,
            false, // false = add to balance
            fee_payment_in_simulation,
        )
        .map_err(|e| -> BootloaderSubsystemError {
            match e {
                SubsystemError::LeafUsage(balance_error) => {
                    system_log!(system, "Error while minting: {balance_error:?}");
                    interface_error!(BootloaderInterfaceError::MintingBalanceOverflow)
                }
                _ => wrap_error!(e),
            }
        })?;

    Ok(())
}

/// Notify L2AssetTracker about base token bridging from L1.
///
/// Calls handleFinalizeBaseTokenBridgingOnL2(uint256 _fromChainId, uint256 _amount)
/// as L2_BASE_TOKEN_ADDRESS (0x800a) to pass the onlyBaseTokenHolderOrL2BaseToken modifier.
///
/// This is called separately for each token movement (value mint, operator
/// payment, refund) so that the asset tracker's accounting stays correct even
/// if the main transaction body reverts.
///
/// Uses infinite EVM gas but tracks native resource consumption via the
/// caller's resource object, so the block seal criteria remain accurate.
///
/// Failure is logged but does not halt block processing — L1 transactions
/// must always be processable to avoid stalling the priority queue.
fn notify_l2_asset_tracker<'a, S: EthereumLikeTypes + 'a>(
    system: &mut System<S>,
    system_functions: &mut HooksStorage<S, S::Allocator>,
    memories: RunnerMemoryBuffers<'a>,
    amount: U256,
    resources: &mut S::Resources,
    tracer: &mut impl Tracer<S>,
    validator: &mut impl TxValidator<S>,
) -> Result<(), BootloaderSubsystemError>
where
    S::IO: IOSubsystemExt,
    S::Metadata: ZkSpecificPricingMetadata
        + BasicMetadata<S::IOTypes, TransactionMetadata = TxLevelMetadata<S::IOTypes>>,
{
    if amount > U256::ZERO {
        // Read settlement layer chain ID from SystemContext storage slot 0
        let sl_chain_id = read_settlement_layer_chain_id(&mut system.io);

        // Encode calldata for handleFinalizeBaseTokenBridgingOnL2(uint256,uint256):
        // selector 0x03117c8c + abi-encoded (fromChainId, amount)
        let mut calldata = [0u8; 68];
        calldata[0..4].copy_from_slice(&[0x03, 0x11, 0x7c, 0x8c]);
        calldata[4..36].copy_from_slice(&sl_chain_id.to_be_bytes::<32>());
        calldata[36..68].copy_from_slice(&amount.to_be_bytes::<32>());

        let failed = resources.with_infinite_ergs(|inf_ergs| {
            let CompletedExecution {
                resources_returned,
                result: asset_tracker_result,
            } = BasicBootloader::<S, ZkTransactionFlowOnlyEOA<S>>::run_single_interaction(
                system,
                system_functions,
                memories,
                &calldata,
                &L2_BASE_TOKEN_ADDRESS,
                &L2_ASSET_TRACKER_ADDRESS,
                inf_ergs.clone(),
                &U256::ZERO,
                true, // should_make_frame - isolate state changes
                tracer,
                validator,
            )?;
            // Overwrite resources inside the closure so that
            // with_infinite_ergs correctly restores ergs afterwards,
            // while the native consumption from the call is kept.
            *inf_ergs = resources_returned;
            Ok::<bool, BootloaderSubsystemError>(asset_tracker_result.failed())
        })?;

        if failed {
            system_log!(
                system,
                "L2AssetTracker.handleFinalizeBaseTokenBridgingOnL2 failed for amount {amount:?}\n"
            );
        }
    }
    Ok(())
}
