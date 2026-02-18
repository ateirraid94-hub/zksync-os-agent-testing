use crate::bootloader::errors::BootloaderInterfaceError;
use crate::bootloader::runner::{run_till_completion, RunnerMemoryBuffers};
use crypto::sha3::{Digest, Keccak256};
use errors::BootloaderSubsystemError;
use system_hooks::addresses_constants::{
    L2_ASSET_TRACKER_ADDRESS, L2_BASE_TOKEN_HOLDER_ADDRESS, L2_CHAIN_ASSET_HANDLER_ADDRESS,
};
use zk_ee::common_structs::system_hooks::HooksStorage;
use zk_ee::system::errors::subsystem::SubsystemError;
use zk_ee::system::errors::{runtime::RuntimeError, system::SystemError};
use zk_ee::system::CallModifier;
use zk_ee::system::{EthereumLikeTypes, System};
use zk_ee::utils::Bytes32;
use zk_ee::{interface_error, internal_error, wrap_error};

use super::*;

impl<S: EthereumLikeTypes, F: BasicTransactionFlow<S>> BasicBootloader<S, F>
where
    S::IO: IOSubsystemExt,
{
    ///
    /// Mints [value] to address [to].
    ///
    pub fn mint_token(
        system: &mut System<S>,
        nominal_token_value: &U256,
        to: &B160,
        resources: &mut S::Resources,
    ) -> Result<(), BootloaderSubsystemError>
    where
        S::IO: IOSubsystemExt,
    {
        // TODO: debug implementation for ruint types uses global alloc, which panics in ZKsync OS
        #[cfg(not(target_arch = "riscv32"))]
        let _ = system.get_logger().write_fmt(format_args!(
            "Minting {nominal_token_value:?} tokens to {to:?}\n"
        ));

        let _old_balance = system
            .io
            .update_account_nominal_token_balance(
                ExecutionEnvironmentType::EVM,
                resources,
                to,
                nominal_token_value,
                false,
            )
            .map_err(|e| -> BootloaderSubsystemError {
                match e {
                    SubsystemError::LeafUsage(balance_error) => {
                        let _ = system
                            .get_logger()
                            .write_fmt(format_args!("Error while minting: {balance_error:?}"));
                        interface_error!(BootloaderInterfaceError::MintingBalanceOverflow)
                    }
                    _ => wrap_error!(e),
                }
            })?;

        Self::update_saved_total_supply(system, resources)?;

        Ok(())
    }

    /// Computes keccak256(abi.encode(key, base_slot)) for Solidity mapping slot derivation.
    fn solidity_mapping_slot(key: &[u8; 32], base_slot: u64) -> Bytes32 {
        let mut hasher = Keccak256::new();
        hasher.update(key);
        hasher.update(&U256::from(base_slot).to_be_bytes::<32>());
        Bytes32::from_array(hasher.finalize().into())
    }

    /// Replicates the L2AssetTracker's `_getOrSaveTotalSupply` logic for the base token.
    ///
    /// On each mint, checks whether `savedTotalSupply[migrationNumber][baseTokenAssetId]`
    /// has been recorded in the L2AssetTracker contract. If not, computes the current
    /// total supply from the L2BaseTokenZKOS contract and writes it.
    ///
    /// This is needed because zksync-os mints tokens natively (bypassing the Solidity
    /// L2BaseToken.mint() which would normally call handleFinalizeBaseTokenBridgingOnL2
    /// on the L2AssetTracker).
    fn update_saved_total_supply(
        system: &mut System<S>,
        resources: &mut S::Resources,
    ) -> Result<(), BootloaderSubsystemError>
    where
        S::IO: IOSubsystemExt,
    {
        let ee = ExecutionEnvironmentType::EVM;

        // 1. Read BASE_TOKEN_ASSET_ID from L2AssetTracker (slot 155)
        let base_token_asset_id = system.io.storage_read::<false>(
            ee,
            resources,
            &L2_ASSET_TRACKER_ADDRESS,
            &Bytes32::from_u256_be(&U256::from(155)),
        )?;
        if base_token_asset_id.is_zero() {
            // Not initialized yet (before genesis upgrade), skip
            return Ok(());
        }

        // 2. Read migrationNumber[chainId] from L2ChainAssetHandler (base slot 207)
        let chain_id = system.get_chain_id();
        let chain_id_bytes = U256::from(chain_id).to_be_bytes::<32>();
        let migration_number_slot =
            Self::solidity_mapping_slot(&chain_id_bytes, 207);
        let migration_number = system.io.storage_read::<false>(
            ee,
            resources,
            &L2_CHAIN_ASSET_HANDLER_ADDRESS,
            &migration_number_slot,
        )?;

        // 3. Compute savedTotalSupply[migrationNumber][baseTokenAssetId] slot
        //    level1 = keccak256(abi.encode(migrationNumber, 156))
        //    level2 = keccak256(abi.encode(baseTokenAssetId, level1))
        let migration_number_bytes = migration_number.as_u8_array();
        let level1_slot =
            Self::solidity_mapping_slot(&migration_number_bytes, 156);
        let level1_bytes = level1_slot.as_u8_array();
        let asset_id_bytes = base_token_asset_id.as_u8_array();
        let mut hasher = Keccak256::new();
        hasher.update(&asset_id_bytes);
        hasher.update(&level1_bytes);
        let struct_base_slot = Bytes32::from_array(hasher.finalize().into());

        // 4. Read isSaved (bool at struct_base_slot + 0)
        let is_saved_value = system.io.storage_read::<false>(
            ee,
            resources,
            &L2_ASSET_TRACKER_ADDRESS,
            &struct_base_slot,
        )?;
        if !is_saved_value.is_zero() {
            // Already saved, nothing to do
            return Ok(());
        }

        // 5. Compute totalSupply = _zkosPreV31TotalSupply + (INITIAL_BASE_TOKEN_HOLDER_BALANCE - holderBalance)
        //    _zkosPreV31TotalSupply is at slot 2 of L2BaseToken (0x800a)
        let pre_v31_supply_bytes = system.io.storage_read::<false>(
            ee,
            resources,
            &system_hooks::addresses_constants::L2_BASE_TOKEN_ADDRESS,
            &Bytes32::from_u256_be(&U256::from(2)),
        )?;
        let pre_v31_supply = pre_v31_supply_bytes.into_u256_be();

        // INITIAL_BASE_TOKEN_HOLDER_BALANCE = 2^127 - 1
        let initial_holder_balance: U256 = (U256::from(1) << 127) - U256::from(1);

        // Read L2_BASE_TOKEN_HOLDER native balance
        let holder_balance_u256 = system
            .io
            .get_nominal_token_balance(ee, resources, &L2_BASE_TOKEN_HOLDER_ADDRESS)?;

        let total_supply = pre_v31_supply
            .checked_add(
                initial_holder_balance
                    .checked_sub(holder_balance_u256)
                    .unwrap_or(U256::ZERO),
            )
            .unwrap_or(U256::ZERO);

        // 6. Write savedTotalSupply[migrationNumber][assetId] = {isSaved: true, amount: totalSupply}
        //    Struct slot 0: isSaved (bool, value = 1)
        //    Struct slot 1: amount (uint256)
        system.io.storage_write::<false>(
            ee,
            resources,
            &L2_ASSET_TRACKER_ADDRESS,
            &struct_base_slot,
            &Bytes32::from_u256_be(&U256::from(1)), // isSaved = true
        )?;

        // Compute struct_base_slot + 1 for the amount field
        let amount_slot_u256 = struct_base_slot.into_u256_be() + U256::from(1);
        let amount_slot = Bytes32::from_u256_be(&amount_slot_u256);
        system.io.storage_write::<false>(
            ee,
            resources,
            &L2_ASSET_TRACKER_ADDRESS,
            &amount_slot,
            &Bytes32::from_u256_be(&total_supply),
        )?;

        #[cfg(not(target_arch = "riscv32"))]
        let _ = system.get_logger().write_fmt(format_args!(
            "Saved total supply {total_supply:?} for base token in L2AssetTracker\n"
        ));

        Ok(())
    }

    ///
    /// Pre-condition: if [nominal_token_value] is not 0, this function
    /// assumes the caller's balance has been validated. It returns an
    /// internal error in case of balance underflow.
    ///
    pub fn run_single_interaction<'a>(
        system: &mut System<S>,
        system_functions: &mut HooksStorage<S, S::Allocator>,
        memories: RunnerMemoryBuffers<'a>,
        calldata: &[u8],
        caller: &B160,
        callee: &B160,
        mut resources: S::Resources,
        nominal_token_value: &U256,
        should_make_frame: bool,
        tracer: &mut impl Tracer<S>,
    ) -> Result<CompletedExecution<'a, S>, BootloaderSubsystemError>
    where
        S::IO: IOSubsystemExt,
    {
        if DEBUG_OUTPUT {
            let _ = system
                .get_logger()
                .write_fmt(format_args!("`caller` = {caller:?}\n"));
            let _ = system
                .get_logger()
                .write_fmt(format_args!("`callee` = {callee:?}\n"));
        }

        let ee_version = {
            resources
                .with_infinite_ergs(|inf_resources| {
                    system.io.read_account_properties(
                        ExecutionEnvironmentType::NoEE,
                        inf_resources,
                        caller,
                        AccountDataRequest::empty().with_ee_version(),
                    )
                })
                .map_err(|e| -> BootloaderSubsystemError {
                    match e {
                        SystemError::LeafRuntime(RuntimeError::OutOfErgs(_)) => {
                            unreachable!("OOG on infinite resources")
                        }
                        e @ SystemError::LeafRuntime(RuntimeError::FatalRuntimeError(_)) => {
                            e.into()
                        }
                        SystemError::LeafDefect(e) => e.into(),
                    }
                })?
                .ee_version
                .0
        };

        // start execution
        let rollback_handle = should_make_frame
            .then(|| {
                system
                    .start_global_frame()
                    .map_err(|_| internal_error!("must start a frame before execution"))
            })
            .transpose()?;

        let ee_type = ExecutionEnvironmentType::parse_ee_version_byte(ee_version)?;

        let initial_request = ExternalCallRequest {
            available_resources: resources.clone(),
            ergs_to_pass: resources.ergs(),
            callers_caller: B160::ZERO, // Fine to use placeholder
            caller: *caller,
            callee: *callee,
            modifier: CallModifier::NoModifier,
            input: calldata,
            call_scratch_space: None,
            nominal_token_value: *nominal_token_value,
        };

        let final_state = run_till_completion(
            memories,
            system,
            system_functions,
            ee_type,
            initial_request,
            tracer,
        )?;

        let CompletedExecution {
            resources_returned,
            result,
        } = final_state;

        if let Some(ref rollback_handle) = rollback_handle {
            system
                .finish_global_frame(result.failed().then_some(rollback_handle))
                .map_err(|_| internal_error!("must finish execution frame"))?;
        }
        Ok(CompletedExecution {
            resources_returned,
            result,
        })
    }
}
