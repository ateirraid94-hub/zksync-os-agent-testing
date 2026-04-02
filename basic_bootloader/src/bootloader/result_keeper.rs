///
/// This module contains definition of the result keeper trait.
///
/// Result keeper structure that will be called during execution to save the block execution result.
/// It's needed for sequencing(to collect receipts, diffs, pubdata).
///
/// Since we will not use it during the proving, it will operate with rust types.
///
use crate::bootloader::errors::InvalidTransaction;
use ruint::aliases::B160;
use zk_ee::system::{IOResultKeeper, NopResultKeeper};
use zk_ee::types_config::SystemIOTypesConfig;

#[derive(Debug)]
pub struct TxProcessingOutput<'a> {
    pub status: bool,
    pub output: &'a [u8],
    pub contract_address: Option<B160>,
    pub gas_used: u64,
    pub gas_refunded: u64,
    pub computational_native_used: u64,
    pub native_used: u64,
    pub pubdata_used: u64,
    /// Computational native resources consumed during validation and fee payment
    /// (before execution begins). Includes intrinsic computational native.
    pub validation_computational_native_used: u64,
    /// Pubdata bytes consumed during validation (before execution begins).
    pub validation_pubdata: u64,
}

pub trait ResultKeeperExt<IOTypes: SystemIOTypesConfig>: IOResultKeeper<IOTypes> {
    type BlockHeader: 'static + Sized;

    fn tx_processed(&mut self, _tx_result: Result<TxProcessingOutput<'_>, InvalidTransaction>) {}

    fn block_sealed(&mut self, _block_header: Self::BlockHeader) {}

    fn record_sealed_block(&mut self, _header: Self::BlockHeader) {}

    fn get_gas_used(&self) -> u64 {
        0u64
    }
}

impl<T: 'static + Sized, IOTypes: SystemIOTypesConfig> ResultKeeperExt<IOTypes>
    for NopResultKeeper<T>
{
    type BlockHeader = T;
}
