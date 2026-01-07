use super::*;
use crate::bootloader::block_flow::post_tx_loop_op::PostTxLoopOp;
use basic_system::system_implementation::cache_structs::storage_values::StorageAccessPolicy;
use basic_system::system_implementation::system::BasicStorageModel;
use zk_ee::common_structs::WarmStorageKey;
use zk_ee::memory::stack_trait::StackCtor;
use zk_ee::system_io_oracle::IOOracle;
use zk_ee::types_config::*;

// TODO: move to fork params
pub const MAX_BLOBS_PER_BLOCK: usize = 21; // TODO: should not be static; should be hardfork-dependent
pub const VERSIONED_HASH_VERSION_KZG: u8 = 0x01;
pub const GAS_PER_BLOB: u64 = 1 << 17;

pub(crate) const SSZ_BYTES_PER_LENGTH_OFFSET: u32 = 4;

pub struct EthereumMetadataOp;
pub struct EthereumPostInitOp;
pub struct EthereumPreOp;
pub struct EthereumPostOp<const PROOF_ENV: bool>;
pub struct EthereumLoopOp;

mod block_data;
mod block_header;
pub mod eip_2935_historical_block_hash;
pub mod eip_4788_historical_beacon_root;
pub mod eip_6110_deposit_events_parser;
pub mod eip_7002_withdrawal_contract;
pub mod eip_7251_consolidation_contract;
mod loop_op;
mod metadata_op;
pub mod oracle_queries;
mod post_init_op;
mod post_tx_op_proving;
mod post_tx_op_sequencing;
mod pre_tx_loop;
pub(crate) mod precompiles;
mod rlp_encodings;
mod utils;
pub(crate) mod withdrawals;

pub use self::block_data::*;
pub use self::block_header::PectraForkHeader;
pub use self::metadata_op::EthereumBlockMetadata;

pub use eip_2935_historical_block_hash::HISTORY_STORAGE_ADDRESS;
pub use eip_4788_historical_beacon_root::BEACON_ROOTS_ADDRESS;
pub use eip_6110_deposit_events_parser::DEPOSIT_CONTRACT_ADDRESS;
pub use eip_7002_withdrawal_contract::WITHDRAWAL_REQUEST_PREDEPLOY_ADDRESS;

pub(crate) fn rlp_ordering_and_key_for_index(index: u32) -> (u32, ([u8; 4], usize)) {
    if index == 0 {
        (0x80, ([0x80, 0x00, 0x00, 0x00], 1usize))
    } else if index < 0x80 {
        (index, ([index as u8, 0x00, 0x00, 0x00], 1))
    } else {
        let ordering_key = 0x80 + index;
        if index < 1 << 8 {
            (ordering_key, ([0x80 + 1, index as u8, 0x00, 0x00], 2))
        } else if index < 1 << 16 {
            (
                ordering_key,
                ([0x80 + 2, (index >> 8) as u8, index as u8, 0x00], 3),
            )
        } else {
            unreachable!()
        }
    }
}
