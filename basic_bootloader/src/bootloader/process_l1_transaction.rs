use super::gas_helpers::get_resources_for_tx;
use super::transaction::abi_encoded::AbiEncodedTransaction;
use super::*;
use crate::bootloader::config::BasicBootloaderExecutionConfig;
use crate::bootloader::constants::UPGRADE_TX_NATIVE_PER_GAS;
use crate::bootloader::errors::TxError;
use crate::bootloader::process_transaction::RefundInfo;
use crate::bootloader::runner::RunnerMemoryBuffers;
use crate::bootloader::transaction_flow::ExecutionResult;
use crate::require_internal;
use constants::L1_TX_INTRINSIC_NATIVE_COST;
use constants::L1_TX_NATIVE_PRICE;
use constants::SIMULATION_NATIVE_PER_GAS;
use constants::{L1_TX_INTRINSIC_L2_GAS, L1_TX_INTRINSIC_PUBDATA};
use errors::BootloaderSubsystemError;
use gas_helpers::check_enough_resources_for_pubdata;
use gas_helpers::get_resources_to_charge_for_pubdata;
use gas_helpers::ResourcesForTx;
use metadata::zk_metadata::TxLevelMetadata;
use system_hooks::HooksStorage;
use zk_ee::internal_error;
use zk_ee::system::errors::root_cause::GetRootCause;
use zk_ee::system::errors::root_cause::RootCause;
use zk_ee::system::errors::runtime::RuntimeError;
use zk_ee::system::metadata::basic_metadata::ZkSpecificPricingMetadata;
use zk_ee::system::{EthereumLikeTypes, Resources};

impl<
        S: EthereumLikeTypes<Metadata = zk_ee::system::metadata::zk_metadata::ZkMetadata>,
        F: BasicTransactionFlow<S>,
    > BasicBootloader<S, F>
where
    S::IO: IOSubsystemExt,
    S::Metadata: ZkSpecificPricingMetadata,
{
    pub(crate) fn process_l1_transaction<'a, Config: BasicBootloaderExecutionConfig>(
        system: &mut System<S>,
        system_functions: &mut HooksStorage<S, S::Allocator>,
        memories: RunnerMemoryBuffers<'a>,
        transaction: &AbiEncodedTransaction<S::Allocator>,
        is_priority_op: bool,
        tracer: &mut impl Tracer<S>,
    ) -> Result<TxProcessingResult<'a>, TxError> {
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

        // For L1->L2 txs, we use a constant native price to avoid censorship.
        let native_price = L1_TX_NATIVE_PRICE;
        let native_per_gas = if is_priority_op {
            if Config::SIMULATION {
                SIMULATION_NATIVE_PER_GAS
            } else {
                gas_price.div_ceil(native_price)
            }
        } else {
            UPGRADE_TX_NATIVE_PER_GAS
        };
        let native_per_pubdata = U256::from(gas_per_pubdata)
            .checked_mul(native_per_gas)
            .ok_or(internal_error!("gpp*npg"))?;

        let ResourcesForTx {
            main_resources: mut resources,
            withheld: withheld_resources,
            intrinsic_computational_native_charged,
        } = get_resources_for_tx::<S>(
            gas_limit,
            native_per_pubdata,
            native_per_gas,
            transaction.calldata(),
            L1_TX_INTRINSIC_L2_GAS,
            L1_TX_INTRINSIC_PUBDATA,
            L1_TX_INTRINSIC_NATIVE_COST,
            true,
        )?;
        // Just used for computing native used
        let initial_resources = resources.clone();

        let tx_internal_cost = gas_price
            .checked_mul(U256::from(gas_limit))
            .ok_or(internal_error!("gp*gl"))?;
        let value = transaction.value.read();
        let total_deposited = transaction.reserved[0].read();
        let needed_amount = value
            .checked_add(U256::from(tx_internal_cost))
            .ok_or(internal_error!("v+tic"))?;
        require_internal!(
            total_deposited >= needed_amount,
            "Deposited amount too low",
            system
        )?;

        // TODO: l1 transaction preparation (marking factory deps)

        let (tx_hash, preparation_out_of_resources): (Bytes32, bool) = match transaction
            .calculate_hash(&mut resources)
        {
            Ok(h) => (h.into(), false),
            Err(e) => {
                match e {
                    TxError::Internal(e) if !matches!(e.root_cause(), RootCause::Runtime(_)) => {
                        return Err(e.into());
                    }
                    // Only way hashing of L1 tx can fail due to Validation or Runtime is
                    // due to running out of native.
                    _ => {
                        let _ = system.get_logger().write_fmt(format_args!(
                            "Transaction preparation exhausted native resources: {e:?}\n"
                        ));

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
        let (result, pubdata_info, resources_before_refund) = if !preparation_out_of_resources {
            // Take a snapshot in case we need to revert due to out of native.
            let rollback_handle = system.start_global_frame()?;

            // Tx execution
            let from = transaction.from.read();
            let to = transaction.to.read();
            match Self::execute_l1_transaction_and_notify_result(
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
            ) {
                Ok((r, pubdata_used, to_charge_for_pubdata, resources_before_refund)) => {
                    let pubdata_info = match r {
                        ExecutionResult::Success { .. } => {
                            system.finish_global_frame(None)?;
                            Some((pubdata_used, to_charge_for_pubdata))
                        }
                        ExecutionResult::Revert { .. } => {
                            system.finish_global_frame(Some(&rollback_handle))?;
                            None
                        }
                    };
                    (r, pubdata_info, resources_before_refund)
                }
                Err(e) => {
                    match e.root_cause() {
                        // Out of native is converted to a top-level revert and
                        // gas is exhausted.
                        RootCause::Runtime(e @ RuntimeError::FatalRuntimeError(_)) => {
                            let _ = system.get_logger().write_fmt(format_args!(
                                "L1 transaction ran out of native resources or memory {e:?}\n"
                            ));
                            resources.exhaust_ergs();
                            system.finish_global_frame(Some(&rollback_handle))?;
                            (
                                ExecutionResult::Revert { output: &[] },
                                None,
                                S::Resources::empty(),
                            )
                        }
                        _ => return Err(e.into()),
                    }
                }
            }
        } else {
            (
                ExecutionResult::Revert { output: &[] },
                None,
                S::Resources::empty(),
            )
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
            gas_refund: _,
            gas_used,
            evm_refund,
            native_used,
        } = Self::compute_gas_refund(
            system,
            to_charge_for_pubdata,
            gas_limit,
            native_per_gas,
            &mut resources,
        )?;

        // Mint fee to bootloader
        // We already checked that total_gas_refund <= gas_limit
        let pay_to_operator = U256::from(gas_used)
            .checked_mul(U256::from(gas_price))
            .ok_or(internal_error!("gu*gp"))?;
        let mut inf_resources = S::Resources::FORMAL_INFINITE;

        let coinbase = system.get_coinbase();
        Self::mint_token(system, &pay_to_operator, &coinbase, &mut inf_resources).map_err(|e| {
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

        // Refund
        let to_refund_recipient = match result {
            ExecutionResult::Revert { .. } => {
                // Upgrade transactions must always succeed
                if !is_priority_op {
                    return Err(internal_error!("Upgrade transaction must succeed").into());
                }
                // If the transaction reverts, then minting the msg.value to the
                // user has been reverted as well, so we can simply mint everything
                // that the user has deposited to the refund recipient
                total_deposited
                    .checked_sub(pay_to_operator)
                    .ok_or(internal_error!("td-pto"))
            }
            ExecutionResult::Success { .. } => {
                // If the transaction succeeds, then it is assumed that msg.value
                // was transferred correctly.
                // However, the remaining value deposited will be given to
                // the refund recipient.
                let value_plus_fee = value
                    .checked_add(pay_to_operator)
                    .ok_or(internal_error!("v+pto"))?;
                total_deposited
                    .checked_sub(value_plus_fee)
                    .ok_or(internal_error!("td-vpf"))
            }
        }?;
        if to_refund_recipient > U256::ZERO {
            let refund_recipient = u256_to_b160_checked(transaction.reserved[1].read());
            Self::mint_token(
                system,
                &to_refund_recipient,
                &refund_recipient,
                &mut inf_resources,
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

        // Emit log
        let success = matches!(result, ExecutionResult::Success { .. });
        let mut inf_resources = S::Resources::FORMAL_INFINITE;
        system.io.emit_l1_l2_tx_log(
            ExecutionEnvironmentType::NoEE,
            &mut inf_resources,
            tx_hash,
            success,
            is_priority_op,
        )?;

        // Add back the intrinsic native charged in get_resources_for_tx,
        // as initial_resources doesn't include them.
        let computational_native_used = resources_before_refund
            .diff(initial_resources)
            .native()
            .as_u64()
            + intrinsic_computational_native_charged;

        Ok(TxProcessingResult {
            result,
            tx_hash,
            is_l1_tx: is_priority_op,
            is_upgrade_tx: !is_priority_op,
            gas_used,
            gas_refunded: evm_refund,
            computational_native_used,
            native_used,
            pubdata_used: pubdata_used + L1_TX_INTRINSIC_PUBDATA,
        })
    }

    // Returns (execution_result, pubdata_used, to_charge_for_pubdata, resources_before_refund)
    fn execute_l1_transaction_and_notify_result<'a>(
        system: &mut System<S>,
        system_functions: &mut HooksStorage<S, S::Allocator>,
        memories: RunnerMemoryBuffers<'a>,
        transaction: &AbiEncodedTransaction<S::Allocator>,
        from: B160,
        to: B160,
        value: U256,
        native_per_pubdata: U256,
        resources: &mut S::Resources,
        withheld_resources: S::Resources,
        tracer: &mut impl Tracer<S>,
    ) -> Result<(ExecutionResult<'a>, u64, S::Resources, S::Resources), BootloaderSubsystemError>
    {
        let _ = system
            .get_logger()
            .write_fmt(format_args!("Executing L1 transaction\n"));

        let gas_price = U256::from(transaction.max_fee_per_gas.read());
        system.set_tx_context(TxLevelMetadata {
            tx_gas_price: gas_price,
            tx_origin: from,
        });

        // Start a frame, to revert minting of value if execution fails
        let rollback_handle = system.start_global_frame()?;

        // First we mint value
        if value > U256::ZERO {
            resources
                .with_infinite_ergs(|inf_resources| {
                    Self::mint_token(system, &value, &from, inf_resources)
                })
                .map_err(|e| match e.root_cause() {
                    RootCause::Runtime(RuntimeError::OutOfErgs(_)) => {
                        let _ = system.get_logger().write_fmt(format_args!(
                            "Out of ergs on infinite ergs: inner error was {e:?}"
                        ));
                        BootloaderSubsystemError::LeafDefect(internal_error!(
                            "Out of ergs on infinite ergs"
                        ))
                    }
                    _ => e,
                })?;
        }

        let resources_for_tx = resources.clone();

        // transaction is in managed region, so we can recast it back
        let calldata = transaction.calldata();

        // TODO: add support for deployment transactions,
        // probably unify with execution logic for EOA

        let CompletedExecution {
            resources_returned,
            result,
        } = Self::run_single_interaction(
            system,
            system_functions,
            memories,
            calldata,
            &from,
            &to,
            resources_for_tx,
            &value,
            false,
            tracer,
        )?;
        let reverted = result.failed();
        let return_values = result.return_values();

        *resources = resources_returned;
        system.finish_global_frame(reverted.then_some(&rollback_handle))?;

        let _ = system
            .get_logger()
            .write_fmt(format_args!("Main TX body successful = {}\n", !reverted));

        let returndata_region = return_values.returndata;

        let execution_result = if reverted {
            ExecutionResult::Revert {
                output: returndata_region,
            }
        } else {
            ExecutionResult::Success {
                output: ExecutionOutput::Call(returndata_region),
            }
        };

        // Just used for computing native used
        // Needs to use the resources before we reclaim withheld
        let resources_before_refund = resources.clone();

        // After the transaction is executed, we reclaim the withheld resources.
        // This is needed to ensure correct "gas_used" calculation, also these
        // resources could be spent for pubdata.
        resources.reclaim_withheld(withheld_resources);

        let (enough, to_charge_for_pubdata, pubdata_used) =
            check_enough_resources_for_pubdata(system, native_per_pubdata, resources, None)?;
        let execution_result = if !enough {
            let _ = system
                .get_logger()
                .write_fmt(format_args!("Not enough gas for pubdata after execution\n"));
            execution_result.reverted()
        } else {
            execution_result
        };

        Ok((
            execution_result,
            pubdata_used,
            to_charge_for_pubdata,
            resources_before_refund,
        ))
    }
}
