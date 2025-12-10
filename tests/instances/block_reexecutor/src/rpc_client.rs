use alloy::primitives::{Address, Bytes, U256};
use anyhow::{anyhow, Result};
use log::{debug, warn};
use rig::{utils::encode_alloy_rpc_tx, zksync_os_interface::traits::EncodedTx};
use ruint::aliases::B160;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io::Read, str::FromStr};
use ureq::json;

/// Simple RPC client with the basic methods we need
pub struct RpcClient {
    endpoint: String,
}

/// Converts u64 to hex string with "0x" prefix.
fn to_hex(n: u64) -> String {
    format!("0x{n:x}")
}

impl RpcClient {
    pub fn new(endpoint: String) -> Self {
        Self { endpoint }
    }

    /// Converts u64 to hex string with "0x" prefix
    fn to_hex(n: u64) -> String {
        format!("0x{n:x}")
    }

    /// Send JSON-RPC request
    fn send(&self, body: Value) -> Result<String> {
        let response = ureq::post(&self.endpoint)
            .set("Content-Type", "application/json")
            .send_json(body)?;

        let mut out = String::new();
        response.into_reader().read_to_string(&mut out)?;
        Ok(out)
    }

    /// Fetches the full block data with transactions.
    pub fn get_block(&self, block_number: u64) -> Result<Block> {
        debug!("RPC: get_block({block_number})");
        let body = json!({
            "method": "eth_getBlockByNumber",
            "params": [to_hex(block_number), true],
            "id": 1,
            "jsonrpc": "2.0"
        });
        let res = self.send(body)?;
        let block = serde_json::from_str(&res)?;
        Ok(block)
    }

    pub fn get_chain_id(&self) -> Result<u64> {
        debug!("RPC: get_chain_id()");
        let body = json!({
            "method": "eth_chainId",
            "params": [],
            "id": 1,
            "jsonrpc": "2.0"
        });
        let res = self.send(body)?;
        let response: Value = serde_json::from_str(&res)?;
        let chain_id_hex = response["result"]
            .as_str()
            .ok_or_else(|| anyhow!("No chain ID found in response"))?;
        let chain_id = u64::from_str_radix(chain_id_hex.trim_start_matches("0x"), 16)?;
        Ok(chain_id)
    }

    /// Fetch account balance
    pub fn get_balance(&self, address: Address, block_number: u64) -> Result<U256> {
        debug!("RPC: get_balance({address}, {block_number})");
        let body = json!({
            "method": "eth_getBalance",
            "params": [format!("0x{:x}", address), Self::to_hex(block_number)],
            "id": 1,
            "jsonrpc": "2.0"
        });
        let res = self.send(body)?;
        let response: Value = serde_json::from_str(&res)?;
        let balance_hex = response["result"]
            .as_str()
            .ok_or_else(|| anyhow!("No balance found in response"))?;
        let balance = U256::from_str(balance_hex)?;
        Ok(balance)
    }

    /// Fetch contract code
    pub fn get_code(&self, address: Address, block_number: u64) -> Result<Bytes> {
        debug!("RPC: get_code({address}, {block_number})");
        let body = json!({
            "method": "eth_getCode",
            "params": [format!("0x{:x}", address), Self::to_hex(block_number)],
            "id": 1,
            "jsonrpc": "2.0"
        });
        let res = self.send(body)?;
        let response: Value = serde_json::from_str(&res)?;
        let code_hex = response["result"]
            .as_str()
            .ok_or_else(|| anyhow!("No code found in response"))?;
        let code = Bytes::from_str(code_hex)?;
        Ok(code)
    }

    /// Fetch transaction count (nonce)
    pub fn get_transaction_count(&self, address: Address, block_number: u64) -> Result<u64> {
        debug!("RPC: get_transaction_count({address}, {block_number})");
        let body = json!({
            "method": "eth_getTransactionCount",
            "params": [format!("0x{:x}", address), Self::to_hex(block_number)],
            "id": 1,
            "jsonrpc": "2.0"
        });
        let res = self.send(body)?;
        let response: Value = serde_json::from_str(&res)?;
        let nonce_hex = response["result"]
            .as_str()
            .ok_or_else(|| anyhow!("No transaction count found in response"))?;
        let nonce = u64::from_str_radix(nonce_hex.trim_start_matches("0x"), 16)?;
        Ok(nonce)
    }

    /// Fetch storage value
    pub fn get_storage_at(&self, address: Address, slot: U256, block_number: u64) -> Result<U256> {
        debug!("RPC: get_storage_at({address}, {slot}, {block_number})");
        let body = json!({
            "method": "eth_getStorageAt",
            "params": [format!("0x{:x}", address), format!("0x{:x}", slot), Self::to_hex(block_number)],
            "id": 1,
            "jsonrpc": "2.0"
        });
        let res = self.send(body)?;
        let response: Value = serde_json::from_str(&res)?;
        let storage_hex = response["result"]
            .as_str()
            .ok_or_else(|| anyhow!("No storage value found in response"))?;
        let storage = U256::from_str(storage_hex)?;
        Ok(storage)
    }
}

use alloy::eips::Typed2718;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Block {
    pub result: alloy::rpc::types::Block<alloy::rpc::types::Transaction, alloy::rpc::types::Header>,
}

impl Block {
    pub fn get_block_context(&self) -> rig::BlockContext {
        let base_fee = U256::from(self.result.header.base_fee_per_gas.unwrap_or(1000));
        rig::BlockContext {
            timestamp: self.result.header.timestamp,
            eip1559_basefee: base_fee,
            pubdata_price: U256::ZERO,
            native_price: base_fee / U256::from(100),
            coinbase: B160::from_be_bytes(self.result.header.beneficiary.0 .0),
            gas_limit: self.result.header.gas_limit,
            pubdata_limit: u64::MAX,
            mix_hash: U256::from_be_bytes(self.result.header.mix_hash.0),
        }
    }

    pub fn get_transactions(self) -> Vec<EncodedTx> {
            self.result
                .transactions
                .into_transactions()
                .enumerate()
                .filter_map(|(i, tx)| {
                    let transaction_type = tx.ty();
                    let supported_tx_type = transaction_type <= 2;
                    if supported_tx_type {
                        Some(encode_alloy_rpc_tx(tx))
                    } else {
                        warn!("Skipping unsupported transaction of type {transaction_type:?}");
                        None
                    }
                })
                .collect()
    }
}
