use crate::live_run::N_PREV_BLOCKS;
use crate::{
    block::Block,
    calltrace::CallTrace,
    prestate::{DiffTrace, PrestateTrace},
    receipts::BlockReceipts,
};
use alloy::consensus::BlockHeader;
use alloy::eips::eip4844::BlobTransactionSidecarItem;
use alloy::primitives::B256;
use alloy::primitives::U256;
use alloy_primitives::{Address, FixedBytes};
use alloy_rpc_types_debug::ExecutionWitness;
use alloy_rpc_types_eth::{Account, EIP1186AccountProofResponse};
use anyhow::anyhow;
use anyhow::Result;
use rig::log::debug;
use std::time::Duration;
use std::{io::Read, str::FromStr};
use ureq::{json, Agent, AgentBuilder};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct JsonResponse<T> {
    pub(crate) result: T,
}

/// Converts u64 to hex string with "0x" prefix.
fn to_hex(n: u64) -> String {
    format!("0x{n:x}")
}

/// Fetches the full block data with transactions.
pub fn get_witness(endpoint: &str, block_number: u64) -> Result<JsonResponse<ExecutionWitness>> {
    debug!("RPC: get_witness({block_number})");
    let body = json!({
        "method": "debug_executionWitness",
        "params": [to_hex(block_number)],
        "id": 1,
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;
    let block = serde_json::from_str(&res)?;
    Ok(block)
}

/// Fetches the full block data with transactions.
pub fn get_block(endpoint: &str, block_number: u64) -> Result<Block> {
    debug!("RPC: get_block({block_number})");
    let body = json!({
        "method": "eth_getBlockByNumber",
        "params": [to_hex(block_number), true],
        "id": 1,
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;
    let block = serde_json::from_str(&res)?;
    Ok(block)
}

/// Fetches the latest block number.
pub fn get_block_number(endpoint: &str) -> Result<u64> {
    debug!("RPC: eth_blockNumber");
    let body = json!({
        "method": "eth_blockNumber",
        "id": 1,
        "params": [],
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;
    let res: serde_json::Value = serde_json::from_str(&res)?;
    let number = res["result"]
        .as_str()
        .ok_or_else(|| anyhow!("No block number found in response"))?;
    let number = u64::from_str_radix(number.trim_start_matches("0x"), 16)?;
    Ok(number)
}

/// Fetches the block hash.
pub fn get_block_hash(endpoint: &str, block_number: u64) -> Result<B256> {
    debug!("RPC: get_block_hash({block_number})");

    let body = json!({
        "method": "eth_getBlockByNumber",
        "params": [to_hex(block_number), true],
        "id": 1,
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;
    let res: serde_json::Value = serde_json::from_str(&res)?;
    let hash_hex = res["result"]["hash"]
        .as_str()
        .ok_or_else(|| anyhow!("No block hash found in response"))?;
    let hash = B256::from_str(hash_hex)?;
    Ok(hash)
}

/// Fetches the block receipts.
pub fn get_receipts(endpoint: &str, block_number: u64) -> Result<BlockReceipts> {
    debug!("RPC: get_receipts({block_number})");
    let body = json!({
        "method": "eth_getBlockReceipts",
        "params": [to_hex(block_number)],
        "id": 1,
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;
    let v = serde_json::from_str(&res)?;
    Ok(v)
}

/// Fetches the prestate trace.
pub fn get_prestate(endpoint: &str, block_number: u64) -> Result<PrestateTrace> {
    debug!("RPC: get_prestate({block_number})");
    let body = json!({
        "method": "debug_traceBlockByNumber",
        "params": [to_hex(block_number), { "tracer": "prestateTracer" }],
        "id": 1,
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;
    let v = serde_json::from_str(&res)?;
    Ok(v)
}

/// Fetches the diff trace.
pub fn get_difftrace(endpoint: &str, block_number: u64) -> Result<DiffTrace> {
    debug!("RPC: get_difftrace({block_number})");
    let body = json!({
        "method": "debug_traceBlockByNumber",
        "params": [to_hex(block_number), {
            "tracer": "prestateTracer",
            "tracerConfig": { "diffMode": true }
        }],
        "id": 1,
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;
    let v = serde_json::from_str(&res)?;
    Ok(v)
}

pub fn get_calltrace(endpoint: &str, block_number: u64) -> Result<CallTrace> {
    debug!("RPC: get_calltrace({block_number})");
    use serde::Deserialize;
    use serde_json::Deserializer;

    let body = json!({
        "method": "debug_traceBlockByNumber",
        "params": [to_hex(block_number), {
            "tracer": "callTracer",
        }],
        "id": 1,
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;

    let mut de = Deserializer::from_str(&res);
    de.disable_recursion_limit();

    let calltrace = CallTrace::deserialize(&mut de)?;
    Ok(calltrace)
}

pub fn get_account_proof(
    endpoint: &str,
    address: Address,
    block_number: u64,
) -> Result<(Account, Vec<u8>)> {
    debug!("RPC: eth_getProof({address}, {block_number})");
    let body = json!({
        "method": "eth_getProof",
        "params": [format!("{}", address).to_ascii_lowercase(), [], to_hex(block_number)],
        "id": 1,
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;
    let v: JsonResponse<EIP1186AccountProofResponse> = serde_json::from_str(&res)?;
    let v = v.result;
    let account = Account {
        nonce: v.nonce,
        balance: v.balance,
        storage_root: v.storage_hash,
        code_hash: v.code_hash,
    };
    let leaf = v
        .account_proof
        .last()
        .map(|el| el.to_vec())
        .unwrap_or_default();
    Ok((account, leaf))
}

pub fn get_blob_sidecars(
    endpoint: &str,
    block_number: u64,
) -> Result<Vec<BlobTransactionSidecarItem>> {
    debug!("RPC: eth_getBlobSidecars({block_number})");
    let body = json!({
        "method": "eth_getBlobSidecars",
        "params": [to_hex(block_number)],
        "id": 1,
        "jsonrpc": "2.0"
    });
    let res = send(endpoint, body)?;
    let v: JsonResponse<Vec<BlobTransactionSidecarItem>> = serde_json::from_str(&res)?;
    let v = v.result;

    Ok(v)
}

#[derive(serde::Deserialize)]
struct BeaconChainBlobsResponse {
    data: Vec<BlobTransactionSidecarItem>,
}

#[derive(serde::Deserialize)]
struct BeaconChainHeaderResponse {
    data: BeaconChainHeaderDataOuter,
}

#[derive(serde::Deserialize)]
struct BeaconChainHeaderDataOuter {
    root: FixedBytes<32>,
    header: BeaconChainHeaderDataInner,
    // ignore the rest
}

#[derive(serde::Deserialize)]
struct BeaconChainHeaderDataInner {
    message: BeaconChainHeaderMessage,
    // ignore the rest
}

#[derive(serde::Deserialize)]
struct BeaconChainHeaderMessage {
    slot: String,
    // ignore the rest
}

pub fn get_blobs_from_beacon_chain(
    beacon_chain_endpoint: &str,
    header: &impl BlockHeader,
) -> Result<Vec<BlobTransactionSidecarItem>> {
    let beacon_chain_parent = header
        .parent_beacon_block_root()
        .ok_or(anyhow::anyhow!("no parent beacon block hash"))?;
    debug!("RPC: beacon/headers({beacon_chain_parent})");
    let rpc = format!(
        "{}/eth/v1/beacon/headers/{}",
        beacon_chain_endpoint, beacon_chain_parent
    );
    let res = get(&rpc)?;
    let v: BeaconChainHeaderResponse = serde_json::from_str(&res)?;
    let data = &v.data;
    assert_eq!(data.root, beacon_chain_parent);
    let slot_idx = u64::from_str_radix(&data.header.message.slot, 10)?;
    let target_slot_idx = slot_idx + 1;
    debug!("RPC: beacon/blob_sidecars({target_slot_idx})");
    let rpc = format!(
        "{}/eth/v1/beacon/blob_sidecars/{}",
        beacon_chain_endpoint, target_slot_idx
    );
    let res = get(&rpc)?;
    let v: BeaconChainBlobsResponse = serde_json::from_str(&res)?;
    let v = v.data;

    Ok(v)
}

fn send(endpoint: &str, body: serde_json::Value) -> Result<String> {
    let agent: Agent = AgentBuilder::new()
        .timeout(Duration::from_secs(600)) // total request timeout
        .build();
    let response = agent
        .post(endpoint)
        .set("Content-Type", "application/json")
        .send_json(body)?;

    let mut out = String::new();
    response.into_reader().read_to_string(&mut out)?;
    Ok(out)
}

fn get(endpoint: &str) -> Result<String> {
    let response = ureq::get(endpoint)
        .set("Content-Type", "application/json")
        .send_bytes(&[])?;

    let mut out = String::new();
    response.into_reader().read_to_string(&mut out)?;
    Ok(out)
}

pub fn fetch_block_hashes_array(
    endpoint: &str,
    block_number: u64,
) -> Result<[U256; N_PREV_BLOCKS]> {
    use anyhow::Context;
    let mut hashes = [U256::ZERO; N_PREV_BLOCKS];
    // Add values for most recent blocks
    for offset in 1..=N_PREV_BLOCKS {
        let n = block_number - (offset as u64);
        let hash =
            get_block_hash(endpoint, n).context(format!("Failed to fetch block hash for {n}"))?;

        hashes[offset - 1] = U256::from_be_bytes(hash.0);
    }

    Ok(hashes)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EthProofPayload {
    pub block_number: u64,
    pub cluster_id: u64,
    // in millis
    pub proving_time: u64,
    pub proving_cycles: u64,
    pub proof: String,
    pub verifier_id: String,
}

pub fn send_ethproofs(
    endpoint: &str,
    auth_token: String,
    proof: EthProofPayload,
) -> Result<String> {
    let agent: Agent = AgentBuilder::new().build();

    let response = agent
        .post(endpoint)
        // Add bearer auth header
        .set("Authorization", &format!("Bearer {}", auth_token))
        .set("Content-Type", "application/json")
        .send_json(proof)?;

    let mut out = String::new();
    response.into_reader().read_to_string(&mut out)?;
    Ok(out)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProofRequest {
    pub block_number: u64,
    pub cluster_id: u64,
}

pub fn update_proof_request(
    endpoint: &str,
    auth_token: String,
    proof_request: ProofRequest,
) -> Result<String> {
    let agent: Agent = AgentBuilder::new().build();

    let response = agent
        .post(endpoint)
        // Add bearer auth header
        .set("Authorization", &format!("Bearer {}", auth_token))
        .set("Content-Type", "application/json")
        .send_json(proof_request)?;

    let mut out = String::new();
    response.into_reader().read_to_string(&mut out)?;
    Ok(out)
}
