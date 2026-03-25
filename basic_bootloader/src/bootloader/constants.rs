use evm_interpreter::ERGS_PER_GAS;
use ruint::aliases::{B160, U256};

pub const SPECIAL_ADDRESS_SPACE_BOUND: u64 = 0x010000;
pub const SPECIAL_ADDRESS_TO_WASM_DEPLOY: B160 = B160::from_limbs([0x9000, 0, 0]);

/// Bootloader's formal address for system-level operations
pub const BOOTLOADER_FORMAL_ADDRESS: B160 = B160::from_limbs([0x8001, 0, 0]);

pub const MAX_TX_LEN_BYTES: usize = 1 << 23;
pub const MAX_TX_LEN_WORDS: usize = MAX_TX_LEN_BYTES / core::mem::size_of::<u32>();

const _: () = const {
    assert!(MAX_TX_LEN_BYTES.is_multiple_of(core::mem::size_of::<usize>()));
};

// 1024 for EVM equivalence
// We actually use 1025 one more because we fail when pushing to the stack,
// while geth checks if the stack depth limit was passed later on in
// the execution.
pub const MAX_CALLSTACK_DEPTH: usize = 1025;

/// Offset for the beginning of the tx data as passed in calldata.
/// The value (96) is the sum of 32 bytes for the tx_hash,
/// 32 for the suggested_signed_hash and 32 for the offset itself.
pub const TX_CALLDATA_OFFSET: usize = 0x60;

/// Maximum value of gas that can be represented as ergs in an u64.
pub const MAX_BLOCK_GAS_LIMIT: u64 = u64::MAX / ERGS_PER_GAS;

// Just for EVM compatibility.
pub const L1_TX_INTRINSIC_L2_GAS: u64 = 21_000;

// Covers intrinsic L1 tx work not charged as tx-body computation:
//  - storing and hashing the L1 tx log
//  - hashing tx hash into the rolling hash and linear hasher
//  - post-execution coinbase transfer and refund transfer
//  - post-execution L2AssetTracker notifications for operator payment and refund
//
// The sender-side transfer/accounting is charged separately. For the refund path
// we assume the recipient is, in most cases, a cold existing account.
pub const L1_TX_INTRINSIC_NATIVE_COST: u64 = 300_000;

// Pubdata needed for the diff in balance as a result of
// the fee payment to the coinbase.
// We take a worst-case value of 32 byte for the key and 34 for
// the uncompressed update.
const COINBASE_BALANCE_INTRINSIC_PUBDATA: u64 = 32 + 34;

// Pubdata produced by the L2AssetTracker.handleFinalizeBaseTokenBridgingOnL2
// call that the bootloader makes inside the L1 tx execution frame (value-mint
// notification). In the steady-state case (base token already registered,
// settled on L1), the contract performs a single SSTORE:
//   interopInfo[assetId].totalSuccessfulDepositsFromL1 += _amount
// Each storage diff is encoded as 32 bytes (derived key) + compressed value
// diff. The worst-case compressed value using the Add strategy with a
// 256-bit amount falls back to Nothing encoding = 33 bytes.
const ASSET_TRACKER_INTRINSIC_PUBDATA: u64 = 32 + 33;

// Needed to publish the l1 tx log, coinbase balance, and asset tracker state diff.
pub const L1_TX_INTRINSIC_PUBDATA: u64 =
    88 + COINBASE_BALANCE_INTRINSIC_PUBDATA + ASSET_TRACKER_INTRINSIC_PUBDATA;

pub const L2_TX_INTRINSIC_GAS: u64 = 21_000;

/// Extra cost for deployment transactions.
pub const DEPLOYMENT_TX_EXTRA_INTRINSIC_GAS: u64 = 32_000;

/// Value taken from system-contracts, to adjust.
pub const L2_TX_INTRINSIC_PUBDATA: u64 = COINBASE_BALANCE_INTRINSIC_PUBDATA;

// Includes:
//  - Transferring fee to coinbase.
//  - Transferring the gas refund.
//  - Hashing of tx hash into rolling hash.
pub const L2_TX_INTRINSIC_NATIVE_COST: u64 = 30_000;

/// Cost to convert zero byte of calldata into "token"
pub const CALLDATA_ZERO_BYTE_TOKEN_FACTOR: u64 = 1;

/// Cost to convert non-zero byte of calldata into "token"
pub const CALLDATA_NON_ZERO_BYTE_TOKEN_FACTOR: u64 = 4;

/// Cost in gas per "token" of calldata
pub const CALLDATA_TOKEN_GAS_COST: u64 = 4;

/// EIP-7623 minimal "token" cost
pub const TOTAL_COST_FLOOR_PER_TOKEN: u64 = 10;

/// Computational cost of 7702 auth
pub const PER_AUTH_NATIVE_COST: u64 = 2000;

/// Computational cost of 2930 access list per address
pub const PER_ADDRESS_ACCESS_LIST_NATIVE_COST: u64 = 2000;

/// Computational cost of 2930 access list per slot
pub const PER_SLOT_ACCESS_LIST_NATIVE_COST: u64 = 2000;

/// EVM tester requires a high native_per_gas, but it hard-codes
/// low gas prices. We need to bypass the usual way to compute this
/// value. The value is so high because of modexp tests.
pub const TESTER_NATIVE_PER_GAS: u64 = 25_000;

// Default native price for L1->L2 transactions.
// TODO (EVM-1157): find a reasonable value for it.
pub const L1_TX_NATIVE_PRICE: U256 = U256::from_limbs([10, 0, 0, 0]);

// Upgrade, service and gateway mailbox transactions are expected to have ~72 million gas. We will use enough
// gas to ensure that multiplied by the 72 million they exceed the native computational limit.
pub const FREE_L1_TX_NATIVE_PER_GAS: u64 = 10000;
