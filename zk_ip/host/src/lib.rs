use airbender_crypto::{sha3::Keccak256, MiniDigest};

mod balance_tree;
mod logs_tree;

pub use balance_tree::BalanceTree;
pub use logs_tree::LogsTree;

pub type H256 = [u8; 32];

pub mod constants {
    use alloy_primitives::{address, hex, Address};

    pub const L2_BOOTLOADER: Address = address!("0x0000000000000000000000000000000000008001");
    pub const L2_KNOWN_CODE_STORAGE: Address =
        address!("0x0000000000000000000000000000000000008004");
    pub const L2_TO_L1_MESSENGER: Address = address!("0x0000000000000000000000000000000000008008");
    pub const L2_BASE_TOKEN: Address = address!("0x000000000000000000000000000000000000800a");
    pub const L2_COMPRESSOR: Address = address!("0x000000000000000000000000000000000000800e");

    pub const L2_ASSET_ROUTER: Address = address!("0x0000000000000000000000000000000000010003");
    pub const L2_NATIVE_TOKEN_VAULT: Address =
        address!("0x0000000000000000000000000000000000010004");
    pub const L2_INTEROP_CENTER: Address = address!("0x000000000000000000000000000000000001000d");
    pub const L2_INTEROP_HANDLER: Address = address!("0x000000000000000000000000000000000001000e");
    pub const L2_ASSET_TRACKER: Address = address!("0x000000000000000000000000000000000001000f");

    pub const L2_LOG_LENGTH: usize = 88;

    // keccak256([0; L2_LOG_LENGTH])
    pub const EMPTY_LOG_HASH: super::H256 =
        hex!("0x72abee45b59e344af8a6e520241c4744aff26ed411f4c4b00f8af09adada43ba");

    pub const FINALIZE_DEPOSIT_SELECTOR: [u8; 4] = hex!("9c884fd1");
    pub const FINALIZE_ETH_WITHDRAWAL_SELECTOR: [u8; 4] = hex!("6c0960f9");
    pub const RECEIVE_MIGRATION_ON_L1_SELECTOR: [u8; 4] = hex!("8e29043a");
}

pub struct L2Log {
    pub tx_number_in_batch: u16,
    pub sender: [u8; 20], // Address
    pub key: H256,
    pub value: H256,
}

impl L2Log {
    pub fn hash(&self) -> H256 {
        let mut buffer = [0u8; constants::L2_LOG_LENGTH];
        buffer[0] = 0; // shard_id = rollup
        buffer[1] = 1; // is_service = true
        buffer[2..4].copy_from_slice(&self.tx_number_in_batch.to_be_bytes());
        buffer[4..24].copy_from_slice(&self.sender);
        buffer[24..56].copy_from_slice(&self.key);
        buffer[56..88].copy_from_slice(&self.value);
        Keccak256::digest(&buffer)
    }
}

pub fn h256_to_u32_array(hash: H256) -> [u32; 8] {
    std::array::from_fn(|i| u32::from_be_bytes(hash[i * 4..(i + 1) * 4].try_into().unwrap()))
}

#[cfg(test)]
mod test;
