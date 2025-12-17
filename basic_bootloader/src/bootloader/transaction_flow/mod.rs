use crate::bootloader::BasicBootloaderExecutionConfig;
use crate::bootloader::BootloaderSubsystemError;
use crate::bootloader::RunnerMemoryBuffers;
use crate::bootloader::TxError;
use crate::bootloader::TxProcessingOutput;
use zk_ee::common_structs::system_hooks::HooksStorage;
use zk_ee::system::errors::internal::InternalError;
use zk_ee::system::tracer::Tracer;
use zk_ee::system::IOSubsystemExt;
use zk_ee::system::ReturnValues;
use zk_ee::system::System;
use zk_ee::system::SystemTypes;
use zk_ee::types_config::SystemIOTypesConfig;
use zk_ee::utils::Bytes32;

use super::transaction::abi_encoded::AbiEncodedTransaction;
use super::transaction::Transaction;

pub(crate) mod gas_helpers;
pub mod process_transaction;
pub(crate) mod refund_calculation;
pub mod zk;

// Address deployed, or reason for the lack thereof.
pub enum DeployedAddress<IOTypes: SystemIOTypesConfig> {
    CallNoAddress,
    RevertedNoAddress,
    Address(IOTypes::Address),
}

pub struct TxExecutionResult<'a, S: SystemTypes> {
    pub return_values: ReturnValues<'a, S>,
    pub reverted: bool,
    pub deployed_address: DeployedAddress<S::IOTypes>,
}

pub trait MinimalTransactionOutput<'a> {
    fn is_success(&self) -> bool;
    fn returndata(&self) -> &[u8];
    fn transaction_hash(&self) -> Bytes32;
    fn into_bookkeeper_output(self) -> TxProcessingOutput<'a>;
}

/// The execution step output
#[derive(Debug)]
pub enum ExecutionOutput<'a, IOTypes: SystemIOTypesConfig> {
    /// return data
    Call(&'a [u8]),
    /// return data, deployed contract address
    Create(&'a [u8], IOTypes::Address),
}

/// The execution step result
#[derive(Debug)]
pub enum ExecutionResult<'a, IOTypes: SystemIOTypesConfig> {
    /// Transaction executed successfully
    Success {
        output: ExecutionOutput<'a, IOTypes>,
    },
    /// Transaction reverted
    Revert { output: &'a [u8] },
}

impl<'a, IOTypes: SystemIOTypesConfig> ExecutionResult<'a, IOTypes> {
    pub fn reverted(self) -> Self {
        match self {
            Self::Success {
                output: ExecutionOutput::Call(r),
            }
            | Self::Success {
                output: ExecutionOutput::Create(r, _),
            } => Self::Revert { output: r },
            a => a,
        }
    }
}

/// Note - even though function here may use IO internally, one should not make such assumptions and open frames
/// at caller side if needed
pub trait BasicTransactionFlow<S: SystemTypes>: Sized
where
    S::IO: IOSubsystemExt,
{
    type TransactionContext: core::fmt::Debug;
    type ExecutionBodyExtraData: core::fmt::Debug;
    type ExecutionResult<'a>: MinimalTransactionOutput<'a>;

    // We identity few steps that are somewhat universal (it's named "basic"),
    // and will try to adhere to them to easier compose the execution flow for transactions that are "intrinsic" and not "enforced upon".

    fn before_validation(
        system: &mut System<S>,
        transaction: &Transaction<S::Allocator>,
        tracer: &mut impl Tracer<S>,
    ) -> Result<(), TxError>;

    fn validate_and_prepare_context<Config: BasicBootloaderExecutionConfig>(
        system: &mut System<S>,
        transaction: &mut Transaction<S::Allocator>,
        tracer: &mut impl Tracer<S>,
    ) -> Result<Self::TransactionContext, TxError>;

    fn before_fee_collection(
        system: &mut System<S>,
        transaction: &Transaction<S::Allocator>,
        context: &Self::TransactionContext,
        tracer: &mut impl Tracer<S>,
    ) -> Result<(), TxError>;

    fn precharge_fee<Config: BasicBootloaderExecutionConfig>(
        system: &mut System<S>,
        transaction: &Transaction<S::Allocator>,
        context: &mut Self::TransactionContext,
        tracer: &mut impl Tracer<S>,
    ) -> Result<(), TxError>;

    fn before_execute_transaction_payload(
        system: &mut System<S>,
        transaction: &Transaction<S::Allocator>,
        context: &mut Self::TransactionContext,
        tracer: &mut impl Tracer<S>,
    ) -> Result<(), TxError>;

    fn create_frame_and_execute_transaction_payload<'a, Config: BasicBootloaderExecutionConfig>(
        system: &mut System<S>,
        system_functions: &mut HooksStorage<S, S::Allocator>,
        memories: RunnerMemoryBuffers<'a>,
        transaction: &Transaction<S::Allocator>,
        context: &mut Self::TransactionContext,
        tracer: &mut impl Tracer<S>,
    ) -> Result<
        (
            ExecutionResult<'a, S::IOTypes>,
            Self::ExecutionBodyExtraData,
        ),
        BootloaderSubsystemError,
    >
    where
        S: 'a;

    fn before_refund<'a, Config: BasicBootloaderExecutionConfig>(
        system: &mut System<S>,
        transaction: &Transaction<S::Allocator>,
        context: &mut Self::TransactionContext,
        result: &ExecutionResult<'a, S::IOTypes>,
        extra_data: Self::ExecutionBodyExtraData,
        tracer: &mut impl Tracer<S>,
    ) -> Result<(), InternalError>;

    fn refund_and_commit_fee<Config: BasicBootloaderExecutionConfig>(
        system: &mut System<S>,
        transaction: &Transaction<S::Allocator>,
        context: &mut Self::TransactionContext,
        tracer: &mut impl Tracer<S>,
    ) -> Result<(), BootloaderSubsystemError>;

    fn after_execution<'a, Config: BasicBootloaderExecutionConfig>(
        system: &mut System<S>,
        transaction: &Transaction<S::Allocator>,
        context: Self::TransactionContext,
        result: ExecutionResult<'a, S::IOTypes>,
        tracer: &mut impl Tracer<S>,
    ) -> Self::ExecutionResult<'a>;

    /// Special method to run an L1 transaction, as they don't necessarily follow the same flow as L2 transactions
    fn process_l1_transaction<'a, Config: BasicBootloaderExecutionConfig>(
        system: &mut System<S>,
        system_functions: &mut HooksStorage<S, S::Allocator>,
        memories: RunnerMemoryBuffers<'a>,
        transaction: &AbiEncodedTransaction<S::Allocator>,
        is_priority_op: bool,
        tracer: &mut impl Tracer<S>,
    ) -> Result<Self::ExecutionResult<'a>, TxError>
    where
        S: 'a;
}
