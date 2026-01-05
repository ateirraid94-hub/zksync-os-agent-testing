use super::GAS_PER_BLOB;
use crate::bootloader::block_flow::ethereum_block_flow::oracle_queries::ETHEREUM_TARGET_HEADER_BUFFER_DATA_QUERY_ID;
use crate::bootloader::block_flow::ethereum_block_flow::oracle_queries::ETHEREUM_TARGET_HEADER_BUFFER_LEN_QUERY_ID;
use crate::bootloader::block_flow::ChainChecker;
use crate::bootloader::errors::BootloaderSubsystemError;
use crate::bootloader::ethereum::LogsBloom;
use crate::bootloader::transaction::ethereum_tx_format::Parser;
use crate::bootloader::transaction::ethereum_tx_format::RLPParsable;
use core::alloc::Allocator;
use crypto::MiniDigest;
use ruint::aliases::B160;
use ruint::aliases::U256;
use zk_ee::internal_error;
use zk_ee::metadata_markers::basic_metadata::BasicBlockMetadata;
use zk_ee::metadata_markers::block_hashes_cache::BlockHashMetadataRequest;
use zk_ee::metadata_markers::block_hashes_cache::BlockHashesCache;
use zk_ee::system::errors::internal::InternalError;
use zk_ee::system_io_oracle::IOOracle;
use zk_ee::types_config::EthereumIOTypesConfig;
use zk_ee::utils::Bytes32;

pub const MIN_BASE_FEE_PER_BLOB_GAS: u64 = 1;
pub const BLOB_BASE_FEE_UPDATE_FRACTION_PRAGUE: u64 = 5007716;
pub const BLOB_BASE_FEE_UPDATE_FRACTION: u64 = BLOB_BASE_FEE_UPDATE_FRACTION_PRAGUE;

const MAX_BLOBS_PER_BLOCK: usize = 9;
const TARGET_BLOBS_PER_BLOCK: u64 = 6;
const TARGET_BLOB_GAS_PER_BLOCK: u64 = GAS_PER_BLOB * TARGET_BLOBS_PER_BLOCK;

const PECTRA_EL_FORK_BLOCK_NUMBER: u64 = 22431084;

const EIP_1559_BASE_FEE_MAX_CHANGE_DENOMINATOR: u64 = 8;
const EIP_1559_ELASTICITY_MULTIPLIER: u64 = 2;
const EIP_1559_MIN_GAS_LIMIT: u64 = 5000;

// EIP-7825 (Fusaka)
pub const EIP_7825_SINGLE_TX_GAS_LIMIT: u64 = 2u64.pow(24);

// EIP-7934 (Fusaka)
pub const EIP_7934_MAX_RLP_BLOCK_SIZE: usize = 8usize * 1024usize * 1024usize; // 8 MiB

#[derive(Clone, Copy, Debug)]
pub struct PectraForkHeader {
    // Default fields
    pub parent_hash: Bytes32,
    pub ommers_hash: Bytes32,
    pub beneficiary: B160,
    pub state_root: Bytes32,
    pub transactions_root: Bytes32,
    pub receipts_root: Bytes32,
    pub logs_bloom: LogsBloom,
    pub difficulty: U256,
    pub number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    // 32 bytes or less, but variable lenth
    pub extra_data: (Bytes32, usize),
    pub mix_hash: Bytes32,
    // fixed length
    pub nonce: [u8; 8],

    // EIP-1559
    pub base_fee_per_gas: u64,

    pub withdrawals_root: Bytes32,

    // EIP-4844
    pub blob_gas_used: u64,
    pub excess_blob_gas: u64,

    pub parent_beacon_block_root: Bytes32,
    pub requests_hash: Bytes32,
}

impl PectraForkHeader {
    pub fn from_relection(header: PectraForkHeaderReflection<'_>) -> Self {
        let PectraForkHeaderReflection {
            parent_hash,
            ommers_hash,
            beneficiary,
            state_root,
            transactions_root,
            receipts_root,
            logs_bloom,
            difficulty,
            number,
            gas_limit,
            gas_used,
            timestamp,
            extra_data,
            mix_hash,
            nonce,
            base_fee_per_gas,
            withdrawals_root,
            blob_gas_used,
            excess_blob_gas,
            parent_beacon_block_root,
            requests_hash,
        } = header;
        let extra_data_len = header.extra_data.len();
        let extra_data = {
            let mut buffer = Bytes32::zero();
            buffer.as_u8_array_mut()[..extra_data_len].copy_from_slice(extra_data);

            buffer
        };

        Self {
            parent_hash: Bytes32::from_array(*parent_hash),
            ommers_hash: Bytes32::from_array(*ommers_hash),
            beneficiary: B160::from_be_bytes(*beneficiary),
            state_root: Bytes32::from_array(*state_root),
            transactions_root: Bytes32::from_array(*transactions_root),
            receipts_root: Bytes32::from_array(*receipts_root),
            logs_bloom: LogsBloom::from_bytes(logs_bloom),
            difficulty: U256::from_be_slice(difficulty),
            number,
            gas_limit,
            gas_used,
            timestamp,
            extra_data: (extra_data, extra_data_len),
            mix_hash: Bytes32::from_array(*mix_hash),
            nonce: *nonce,
            base_fee_per_gas,
            withdrawals_root: Bytes32::from_array(*withdrawals_root),
            blob_gas_used,
            excess_blob_gas,
            parent_beacon_block_root: Bytes32::from_array(*parent_beacon_block_root),
            requests_hash: Bytes32::from_array(*requests_hash),
        }
    }
}

pub struct HeaderAndHistory {
    pub chain_id: u64,
    pub header: PectraForkHeader,
    pub history_cache: core::cell::UnsafeCell<BlockHashesCache>,
    pub computed_header_hash: Bytes32,
    pub computed_blob_base_fee_per_gas: U256,
}

impl BasicBlockMetadata<EthereumIOTypesConfig> for HeaderAndHistory {
    fn chain_id(&self) -> u64 {
        self.chain_id
    }
    fn block_number(&self) -> u64 {
        self.header.number
    }
    fn block_historical_hash(&self, depth: u64) -> Option<Bytes32> {
        use zk_ee::metadata_markers::DynamicMetadataResponder;
        if depth < 256 {
            unsafe {
                Some(
                    self.history_cache
                        .as_mut_unchecked()
                        .get_metadata_with_bookkeeping::<BlockHashMetadataRequest>(depth as u8),
                )
            }
        } else {
            None
        }
    }
    fn block_timestamp(&self) -> u64 {
        self.header.timestamp
    }
    fn block_randomness(&self) -> Option<Bytes32> {
        Some(self.header.mix_hash)
    }
    fn coinbase(&self) -> B160 {
        self.header.beneficiary
    }
    fn block_gas_limit(&self) -> u64 {
        self.header.gas_limit
    }
    fn individual_tx_gas_limit(&self) -> u64 {
        EIP_7825_SINGLE_TX_GAS_LIMIT
    }
    fn eip1559_basefee(&self) -> U256 {
        U256::from(self.header.base_fee_per_gas)
    }
    fn max_blobs(&self) -> usize {
        MAX_BLOBS_PER_BLOCK
    }
    fn blobs_gas_limit(&self) -> u64 {
        self.max_blobs() as u64 * GAS_PER_BLOB
    }
    fn blob_base_fee_per_gas(&self) -> U256 {
        self.computed_blob_base_fee_per_gas
    }
}

impl HeaderAndHistory {
    pub fn new(
        oracle: &mut impl IOOracle,
        allocator: impl core::alloc::Allocator,
    ) -> Result<Self, InternalError> {
        let chain_id = 1u64;
        // get buffer
        let target_header_buffer = oracle.get_bytes_from_query(
            ETHEREUM_TARGET_HEADER_BUFFER_LEN_QUERY_ID,
            ETHEREUM_TARGET_HEADER_BUFFER_DATA_QUERY_ID,
            &(),
            allocator,
        )?;

        let target_header_buffer = target_header_buffer.expect("target header is not empty slice");

        if target_header_buffer.len() > EIP_7934_MAX_RLP_BLOCK_SIZE {
            return Err(internal_error!(
                "target header size exceeds maximum allowed by EIP-7934"
            ));
        }

        let target_header =
            PectraForkHeaderReflection::try_parse_slice_in_full(target_header_buffer.as_slice())
                .map_err(|_| internal_error!("must parse target header from bytes"))?;
        let computed_header_hash =
            crypto::sha3::Keccak256::digest(target_header_buffer.as_slice()).into();
        let header = PectraForkHeader::from_relection(target_header);
        let cache = BlockHashesCache::from_oracle(oracle)?;

        use crate::bootloader::block_flow::ethereum_block_flow::utils::fake_exponential;

        // TODO: consider if it's numerically stable over u64
        let blob_parameters = BlobParameters::for_block_timestamp(header.timestamp);
        let computed_blob_base_fee_per_gas = fake_exponential(
            U256::from(MIN_BASE_FEE_PER_BLOB_GAS),
            &U256::from(header.excess_blob_gas),
            &U256::from(blob_parameters.base_fee_update_fraction),
        );

        Ok(Self {
            chain_id,
            header,
            history_cache: core::cell::UnsafeCell::new(cache),
            computed_header_hash,
            computed_blob_base_fee_per_gas,
        })
    }
}

// and we need a simple reflection to parse block hashes chain
#[derive(Clone, Copy, Debug)]
pub struct PectraForkHeaderReflection<'a> {
    // Default fields
    pub parent_hash: &'a [u8; 32],
    pub ommers_hash: &'a [u8; 32],
    pub beneficiary: &'a [u8; 20],
    pub state_root: &'a [u8; 32],
    pub transactions_root: &'a [u8; 32],
    pub receipts_root: &'a [u8; 32],
    pub logs_bloom: &'a [u8; 256],
    pub difficulty: &'a [u8],
    pub number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    // 32 bytes or less, but variable length
    pub extra_data: &'a [u8],
    pub mix_hash: &'a [u8; 32],
    // fixed length
    pub nonce: &'a [u8; 8],

    // EIP-1559
    pub base_fee_per_gas: u64,

    pub withdrawals_root: &'a [u8; 32],

    // EIP-4844
    pub blob_gas_used: u64,
    pub excess_blob_gas: u64,

    pub parent_beacon_block_root: &'a [u8; 32],
    pub requests_hash: &'a [u8; 32],
}

impl<'a> RLPParsable<'a> for PectraForkHeaderReflection<'a> {
    fn try_parse(parser: &mut Parser<'a>) -> Result<Self, ()> {
        let mut list_parser = parser.try_make_list_subparser()?;

        let parent_hash = RLPParsable::try_parse(&mut list_parser)?;
        let ommers_hash = RLPParsable::try_parse(&mut list_parser)?;
        let beneficiary = RLPParsable::try_parse(&mut list_parser)?;
        let state_root = RLPParsable::try_parse(&mut list_parser)?;
        let transactions_root = RLPParsable::try_parse(&mut list_parser)?;
        let receipts_root = RLPParsable::try_parse(&mut list_parser)?;
        let logs_bloom = RLPParsable::try_parse(&mut list_parser)?;
        let difficulty = RLPParsable::try_parse(&mut list_parser)?;
        let number = RLPParsable::try_parse(&mut list_parser)?;
        let gas_limit = RLPParsable::try_parse(&mut list_parser)?;
        let gas_used = RLPParsable::try_parse(&mut list_parser)?;
        let timestamp = RLPParsable::try_parse(&mut list_parser)?;
        // 32 bytes or less, but variable lenth
        let extra_data: &'a [u8] = RLPParsable::try_parse(&mut list_parser)?;
        if extra_data.len() > 32 {
            return Err(());
        }
        let mix_hash = RLPParsable::try_parse(&mut list_parser)?;
        // fixed length
        let nonce = RLPParsable::try_parse(&mut list_parser)?;

        let base_fee_per_gas = RLPParsable::try_parse(&mut list_parser)?;

        let withdrawals_root = RLPParsable::try_parse(&mut list_parser)?;

        let blob_gas_used = RLPParsable::try_parse(&mut list_parser)?;
        let excess_blob_gas = RLPParsable::try_parse(&mut list_parser)?;

        let parent_beacon_block_root = RLPParsable::try_parse(&mut list_parser)?;
        let requests_hash = RLPParsable::try_parse(&mut list_parser)?;

        if list_parser.is_empty() {
            let new = Self {
                parent_hash,
                ommers_hash,
                beneficiary,
                state_root,
                transactions_root,
                receipts_root,
                logs_bloom,
                difficulty,
                number,
                gas_limit,
                gas_used,
                timestamp,
                extra_data,
                mix_hash,
                nonce,
                base_fee_per_gas,
                withdrawals_root,
                blob_gas_used,
                excess_blob_gas,
                parent_beacon_block_root,
                requests_hash,
            };

            Ok(new)
        } else {
            Err(())
        }
    }
}

impl ChainChecker for PectraForkHeader {
    type ExtraData = BlockHashesCache;
    type Output = Bytes32;

    fn verify_chain<A: Allocator + Clone>(
        &self,
        current_block_number: u64,
        verification_depth: usize,
        oracle: &mut impl IOOracle,
        extra_data: &Self::ExtraData,
        allocator: A,
    ) -> Result<Self::Output, BootloaderSubsystemError> {
        use crate::bootloader::block_flow::ethereum_block_flow::oracle_queries::ETHEREUM_HISTORICAL_HEADER_BUFFER_DATA_QUERY_ID;
        use crate::bootloader::block_flow::ethereum_block_flow::oracle_queries::ETHEREUM_HISTORICAL_HEADER_BUFFER_LEN_QUERY_ID;

        let history_cache = extra_data;
        assert!(verification_depth > 0);
        assert_eq!(self.number, current_block_number);
        assert!(current_block_number >= verification_depth as u64); // Do not want underflows anywhere beloe

        let mut block_headers_hasher = <crypto::sha3::Keccak256 as crypto::MiniDigest>::new();
        let mut initial_state_commitment = Bytes32::ZERO;
        let mut parent_to_expect = self.parent_hash;

        for depth in 0..verification_depth {
            let block_number = current_block_number - 1 - (depth as u64);
            // we do not expect to have any practical implementation that go across header formats,
            // so we assert here
            assert!(block_number >= PECTRA_EL_FORK_BLOCK_NUMBER);

            use crate::bootloader::transaction::ethereum_tx_format::RLPParsable;

            let buffer = oracle
                .get_bytes_from_query(
                    ETHEREUM_HISTORICAL_HEADER_BUFFER_LEN_QUERY_ID,
                    ETHEREUM_HISTORICAL_HEADER_BUFFER_DATA_QUERY_ID,
                    &(depth as u32),
                    allocator.clone(),
                )
                .expect("must get buffer for historical header")
                .expect("buffer for historical header is not empty");
            let historical_header =
                PectraForkHeaderReflection::try_parse_slice_in_full(buffer.as_slice())
                    .expect("must parse historical header");
            crypto::MiniDigest::update(&mut block_headers_hasher, buffer.as_slice());
            let computed_header_hash: Bytes32 =
                crypto::MiniDigest::finalize_reset(&mut block_headers_hasher).into();
            assert_eq!(history_cache.cache_entry(depth), &computed_header_hash,);
            assert_eq!(historical_header.number, block_number);
            assert_eq!(
                &parent_to_expect,
                &computed_header_hash,
                "parent header malformed for history depth {}",
                depth + 1
            );

            if depth == 0 {
                initial_state_commitment = Bytes32::from_array(*historical_header.state_root);

                // we should check cross-blocks pricing invariants, so EIP-1559 for gas price,
                // and check excess blob gas

                // EIP-1559
                {
                    assert_eq!(EIP_1559_ELASTICITY_MULTIPLIER, 2);
                    let parent_gas_limit = historical_header.gas_limit;
                    let parent_gas_target = parent_gas_limit >> 1; // EIP_1559_ELASTICITY_MULTIPLIER == 2

                    let parent_base_fee_per_gas = historical_header.base_fee_per_gas;
                    let parent_gas_used = historical_header.gas_used;

                    let slack = parent_gas_limit >> 10;
                    assert!(self.gas_limit < parent_gas_limit + slack);
                    assert!(self.gas_limit > parent_gas_limit - slack);

                    assert!(self.gas_limit >= EIP_1559_MIN_GAS_LIMIT);

                    let expected_base_fee_per_gas = if parent_gas_used == parent_gas_target {
                        parent_base_fee_per_gas
                    } else if parent_gas_used > parent_gas_target {
                        let gas_used_delta = parent_gas_used - parent_gas_target;
                        let base_fee_per_gas_delta = core::cmp::max(
                            parent_base_fee_per_gas * gas_used_delta
                                / parent_gas_target
                                / EIP_1559_BASE_FEE_MAX_CHANGE_DENOMINATOR,
                            1,
                        );
                        parent_base_fee_per_gas + base_fee_per_gas_delta
                    } else {
                        let gas_used_delta = parent_gas_target - parent_gas_used;
                        let base_fee_per_gas_delta = parent_base_fee_per_gas * gas_used_delta
                            / parent_gas_target
                            / EIP_1559_BASE_FEE_MAX_CHANGE_DENOMINATOR;
                        parent_base_fee_per_gas - base_fee_per_gas_delta
                    };
                    assert_eq!(expected_base_fee_per_gas, self.base_fee_per_gas);
                }

                // EIP-4844
                {
                    // EIP-7918 logic
                    let blob_parameters = BlobParameters::for_block_timestamp(self.timestamp);
                    let target_blob_gas = blob_parameters.target * GAS_PER_BLOB;

                    let computed_blob_base_fee_per_gas =
                        crate::bootloader::block_flow::ethereum_block_flow::utils::fake_exponential(
                            U256::from(MIN_BASE_FEE_PER_BLOB_GAS),
                            &U256::from(historical_header.excess_blob_gas),
                            &U256::from(blob_parameters.base_fee_update_fraction),
                        );

                    let excess_blob_gas = if historical_header.excess_blob_gas
                        + historical_header.blob_gas_used
                        < target_blob_gas
                    {
                        0
                    } else if U256::from(BLOB_BASE_COST)
                        * U256::from(historical_header.base_fee_per_gas)
                        > U256::from(GAS_PER_BLOB) * computed_blob_base_fee_per_gas
                    {
                        historical_header.excess_blob_gas
                            + historical_header.blob_gas_used
                                * (blob_parameters.max - blob_parameters.target)
                                / blob_parameters.max
                    } else {
                        historical_header.excess_blob_gas + historical_header.blob_gas_used
                            - target_blob_gas
                    };

                    assert_eq!(self.excess_blob_gas, excess_blob_gas);
                }
            }
            parent_to_expect = Bytes32::from_array(*historical_header.parent_hash);
        }

        Ok(initial_state_commitment)
    }
}

// TODO: Move to a separate file

pub const BLOB_BASE_COST: u64 = 2u64.pow(13);

#[derive(Debug, Clone, Copy)]
pub struct BlobParameters {
    pub target: u64,
    pub max: u64,
    pub base_fee_update_fraction: u64,
}

impl BlobParameters {
    const CANCUN_TIME: u64 = 0;
    const PRAGUE_TIME: u64 = 0;
    const OSAKA_TIME: u64 = 1747387400;
    const BPO1_TIME: u64 = 1757387400;
    const BPO2_TIME: u64 = 1767747671; // Value in EIP is wrong, correct value is here: https://notes.ethereum.org/@bbusa/fusaka-bpo-timeline

    pub fn for_block_timestamp(block_timestamp: u64) -> Self {
        if block_timestamp >= Self::BPO2_TIME {
            BPO2_BLOB_PARAMETERS
        } else if block_timestamp >= Self::BPO1_TIME {
            BPO1_BLOB_PARAMETERS
        } else if block_timestamp >= Self::OSAKA_TIME {
            OSAKA_BLOB_PARAMETERS
        } else {
            PRAGUE_BLOB_PARAMETERS
        }
    }
}

const CANCUN_BLOB_PARAMETERS: BlobParameters = BlobParameters {
    target: 3,
    max: 6,
    base_fee_update_fraction: 3338477,
};

const PRAGUE_BLOB_PARAMETERS: BlobParameters = BlobParameters {
    target: 6,
    max: 9,
    base_fee_update_fraction: 5007716,
};

const OSAKA_BLOB_PARAMETERS: BlobParameters = BlobParameters {
    target: 6,
    max: 9,
    base_fee_update_fraction: 5007716,
};

const BPO1_BLOB_PARAMETERS: BlobParameters = BlobParameters {
    target: 10,
    max: 15,
    base_fee_update_fraction: 8346193,
};

const BPO2_BLOB_PARAMETERS: BlobParameters = BlobParameters {
    target: 14,
    max: 21,
    base_fee_update_fraction: 11684671,
};
