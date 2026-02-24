#![no_main]

use airbender::crypto::{blake2s::Blake2s256, sha3::Keccak256, MiniDigest};
use alloy_sol_types::{SolType, private::primitives::U256, sol_data::{Address, Uint, Bytes}};

use utils::*;
use balance_tree::BalanceTree;
use logs_tree::LogsTree;

mod logs_tree;
mod utils;
mod balance_tree;

type H256 = [u8; 32];

struct L2Log {
    tx_number_in_batch: u16,
    sender: [u8; 20],
    key: H256,
    value: H256,
}

impl L2Log {
    fn hash(&self) -> H256 {
        let mut buffer = [0u8; L2_LOG_LENGTH];
        buffer[0] = 0; // shard_id = rollup
        buffer[1] = 1; // is_service = true
        buffer[2..4].copy_from_slice(&self.tx_number_in_batch.to_be_bytes());
        buffer[4..24].copy_from_slice(&self.sender);
        buffer[24..56].copy_from_slice(&self.key);
        buffer[56..88].copy_from_slice(&self.value);
        Keccak256::digest(&buffer)
    }
}

fn handle_asset_router_message(message: &[u8], balance_tree: &mut BalanceTree) {
    assert!(message.len() >= 68);
    let selector = &message[..4];
    let asset_id: H256 = message[36..68].try_into().unwrap();
    let transfer_data = &message[68..];

    // finalizeDeposit.selector
    assert_eq!(selector, b"\x9c\x88\x4f\xd1");

    type Tuple = (Address, Address, Address, Uint<256>, Bytes);
    let (_, _, original_token, amount, erc20_metadata) =
        Tuple::abi_decode_sequence_validate(transfer_data).expect("decoding failed");

    let token_original_chain_id = if erc20_metadata[0] == 0 {
        [0; 32]
    } else if erc20_metadata[0] == 1 {
        erc20_metadata[1..33].try_into().unwrap()
    } else {
        panic!("invalid erc20 metadata version")
    };

    let asset_data = original_token.into_word();
    assert_eq!(asset_id, get_asset_id(token_original_chain_id, asset_data.0));

    balance_tree.update_balance(asset_id, amount, false);
}

fn handle_base_token_contract_message(message: &[u8], base_token_asset_id: H256, balance_tree: &mut BalanceTree) {
    let selector = &message[..4];
    let amount: H256 = message[24..56].try_into().unwrap();
    let amount = U256::from_be_bytes(amount);

    // finalizeEthWithdrawal.selector
    assert_eq!(selector, b"\x6c\x09\x60\xf9");
    balance_tree.update_balance(base_token_asset_id, amount, false);
}

fn get_asset_id(chain_id: H256, asset_data: H256) -> H256 {
    Keccak256::digest([chain_id, L2_NATIVE_TOKEN_VAULT.into_word().0, asset_data].concat())
}

macro_rules! read {
    ($msg:literal) => {
        airbender::guest::read().expect($msg)
    };
}

#[airbender::main]
fn main() -> [u32; 8] {
    let prev_root: H256 = read!("prev root");
    let prev_tree_size: u32 = read!("prev tree size"); // assume there is < 4billion tokens and > 0
    let base_token_asset_id: H256 = read!("base token asset id"); // TODO - do we include it in public commitment?

    let mut balance_tree = BalanceTree::new(prev_tree_size);

    let n: u32 = read!("number of existing tokens"); // number of existing tokens
    for _i in 0..n {
        let asset_id = read!("asset id");
        let index = read!("index");
        let prev_balance = read!("prev balance");
        let path = read!("path");

        let hash = balance_tree.insert_token_info(asset_id, index, prev_balance, path);
        assert_eq!(hash, prev_root, "root mismatch");
    }

    let mut tree = LogsTree::new();
    let n: u32 = read!("number of logs"); // number of logs to parse
    for _i in 0..n {
        let tx_number_in_batch = read!("tx number in batch");
        let sender = read!("log sender");
        let key = read!("log key");
        let value = read!("log value");

        let log = L2Log {
            tx_number_in_batch,
            sender,
            key,
            value,
        };

        tree.push(log.hash());

        if sender == L2_TO_L1_MESSENGER {
            let message: Vec<u8> = read!("log message");
            assert_eq!(Keccak256::digest(&message), value);
            if key == L2_ASSET_ROUTER.into_word() {
                handle_asset_router_message(&message, &mut balance_tree);
            } else if key == L2_BASE_TOKEN.into_word() {
                handle_base_token_contract_message(&message, base_token_asset_id, &mut balance_tree);
            } else if key == L2_ASSET_TRACKER.into_word() {
                // IAssetTrackerDataEncoding.receiveMigrationOnL1.selector,
                assert_eq!(&message[..4], b"\x8e\x29\x04\x3a");
            } else if key == L2_COMPRESSOR.into_word() {
                // no further action
            } else if key == L2_KNOWN_CODE_STORAGE.into_word() {
                // no further action
            } else if key == L2_INTEROP_CENTER.into_word() {
                todo!();
            } else if key == L2_INTEROP_HANDLER.into_word() {
                todo!();
            } else {
                todo!();
            }
        } else if sender == L2_BOOTLOADER {
            todo!()
        }
    }

    let l2_logs_root = tree.root();

    let commitment = Blake2s256::digest([balance_tree.root(), prev_root, /*l2_logs_root*/].concat());
    std::array::from_fn(|i| u32::from_be_bytes(commitment[i * 4..(i + 1) * 4].try_into().unwrap()))
}
