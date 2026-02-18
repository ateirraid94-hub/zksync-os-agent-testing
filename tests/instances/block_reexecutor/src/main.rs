use alloy::{primitives::B256, rpc::types::trace::geth::CallFrame};
use anyhow::Result;
use clap::Parser;
use std::collections::HashMap;

mod cache;
mod rpc_client;
mod rpc_oracle;
mod state_view;
mod tx_check;

use cache::{
    block_params_cache_path, load_or_fetch_block_params, load_oracle_caches, oracle_cache_paths,
    save_oracle_caches,
};
use rig::{
    chain::RunConfig, forward_system::system::tracers::call_tracer::CallTracer,
    zk_ee::common_structs::derive_flat_storage_key,
};
use rpc_client::RpcClient;
use state_view::{generate_block_context_interface, ChainStateView};
use tx_check::{check_tx_outputs_against_receipts, filter_supported_receipts};
use zksync_os_revm_runner::revm_runner;

#[derive(Parser, Debug)]
#[command(author, version, about = "Re-execute blocks using external RPC")]
struct Args {
    /// RPC endpoint URL
    #[arg(long, default_value = "http://localhost:8545")]
    endpoint: String,

    /// Block hash to re-execute
    #[arg(long)]
    block_hash: B256,

    /// Enable verbose logging
    #[arg(long, short)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    rig::init_logger();

    println!("Starting block re-execution");
    println!("Endpoint: {}", args.endpoint);
    println!("Block hash: {:?}", args.block_hash);

    let rpc_client = RpcClient::new(args.endpoint.clone());
    let block_params_path = block_params_cache_path(args.block_hash);
    let loaded = load_or_fetch_block_params(&rpc_client, args.block_hash, &block_params_path)?;
    let block = loaded.block;
    let block_metadata = loaded.block_metadata;
    let chain_id = loaded.chain_id;
    let cached_or_fetched_receipts = loaded.receipts;

    let block_number = block.result.header.number;
    let miner = block.result.header.beneficiary;
    let mut block_context = block.get_block_context();

    println!("Native price: {}", block_metadata.result.native_price);
    println!(
        "Pubdata price: {}",
        block_metadata.result.pubdata_price_per_byte
    );

    block_context.native_price = block_metadata.result.native_price;
    block_context.pubdata_price = block_metadata.result.pubdata_price_per_byte;

    let transactions = block.clone().get_transactions();

    println!(
        "Block {} has {} transactions",
        block_number,
        transactions.len()
    );
    println!("Block hash: {:?}", block.result.header.hash);
    println!("Block miner: {:?}", miner);
    println!(
        "Block gas used: {} / {}",
        block.result.header.gas_used, block.result.header.gas_limit
    );

    if transactions.is_empty() {
        println!("No transactions to execute, skipping block");
        return Ok(());
    }

    println!(
        "Block context: timestamp={}, gas_limit={}, coinbase={:?}",
        block_context.timestamp, block_context.gas_limit, block_context.coinbase
    );

    let oracle_factory = rpc_oracle::RpcValueOracleFactory::new(args.endpoint, block_number);
    let (storage_cache_path, preimages_cache_path) = oracle_cache_paths(args.block_hash);
    let (cached_storage, cached_preimages) =
        match load_oracle_caches(&storage_cache_path, &preimages_cache_path) {
            Ok(caches) => caches,
            Err(err) => {
                eprintln!("Failed to load oracle cache from disk: {err}");
                (HashMap::new(), HashMap::new())
            }
        };
    if !cached_storage.is_empty() || !cached_preimages.is_empty() {
        println!(
            "Loaded oracle cache from disk: storage_slots={}, preimages={}",
            cached_storage.len(),
            cached_preimages.len()
        );
    }

    oracle_factory
        .cache
        .lock()
        .expect("failed to lock oracle cache")
        .extend(cached_storage);
    oracle_factory
        .preimages
        .lock()
        .expect("failed to lock oracle preimages")
        .extend(cached_preimages);

    let mut chain = rig::Chain::empty(Some(chain_id));

    println!(
        "Running block with {} transactions using RPC oracle...",
        transactions.len()
    );

    let run_config = Some(RunConfig {
        profiler_config: None,
        witness_output_file: None,
        app: None,
        only_forward: true,
        check_storage_diff_hashes: false,
        not_update_state_after_block_execution: true,
    });

    let mut tracer = CallTracer::default();

    let block_output = chain.run_block_with_oracle_factory_and_tracer(
        transactions,
        Some(block_context.clone()),
        None,
        run_config,
        &oracle_factory,
        &mut tracer,
    );

    let receipts = filter_supported_receipts(&block, cached_or_fetched_receipts)?;
    check_tx_outputs_against_receipts(&block_output, &receipts)?;
    println!(
        "Transaction output check passed against {} cached RPC receipts",
        receipts.len()
    );

    let preimages = oracle_factory
        .preimages
        .lock()
        .expect("failed to lock oracle preimages")
        .clone();
    let storage = oracle_factory
        .cache
        .lock()
        .expect("failed to lock oracle cache")
        .clone();

    save_oracle_caches(
        &storage_cache_path,
        &preimages_cache_path,
        &storage,
        &preimages,
    )?;
    println!(
        "Saved oracle cache to disk: storage_slots={}, preimages={}",
        storage.len(),
        preimages.len()
    );

    for (hash, preimage) in &preimages {
        chain.preimage_source.inner.insert(*hash, preimage.clone());
    }

    for ((address, slot), value) in &storage {
        let flat_key = derive_flat_storage_key(&address, &slot);
        chain.state_tree.cold_storage.insert(flat_key, *value);
    }

    println!("Block execution completed successfully!");
    println!("Block output: gas_used = {}", block_output.header.gas_used);
    println!("Block output: transactions = {:?}", block_output.tx_results);

    println!("Block re-execution completed");

    let trace = tracer
        .transactions
        .into_iter()
        .map(CallFrame::from)
        .collect::<Vec<_>>();
    let trace = apply_root_gas_used_from_block_output(trace, &block_output);
    let trace = trace
        .into_iter()
        .map(normalize_call_frame_for_geth_output)
        .collect::<Vec<_>>();

    let tracer_output_path = format!("tracer_output_{}.json", block_number);
    std::fs::write(&tracer_output_path, serde_json::to_string_pretty(&trace)?)?;
    println!("Tracer output saved to {}", tracer_output_path);

    println!("Runnning ZKsync OS REVM");

    let block_context = generate_block_context_interface(&chain, &block_context);
    let state_view = ChainStateView { chain };
    let raw_transactions = block.get_transactions_raw();

    let mut revm_runner = revm_runner::RevmRunner::new(state_view);

    let revm_trace =
        revm_runner.run_with_call_traces(raw_transactions, block_context, Some(block_output))?;
    let revm_trace_output_path = format!("revm_call_trace_{}.json", block_number);
    std::fs::write(
        &revm_trace_output_path,
        serde_json::to_string_pretty(&revm_trace)?,
    )?;
    println!("REVM call trace saved to {}", revm_trace_output_path);

    Ok(())
}

fn apply_root_gas_used_from_block_output(
    mut traces: Vec<CallFrame>,
    block_output: &rig::zksync_os_interface::types::BlockOutput,
) -> Vec<CallFrame> {
    if traces.len() != block_output.tx_results.len() {
        eprintln!(
            "trace/result length mismatch: traces={} tx_results={}",
            traces.len(),
            block_output.tx_results.len()
        );
    }

    for (frame, tx_result) in traces.iter_mut().zip(block_output.tx_results.iter()) {
        if let Ok(tx_output) = tx_result {
            frame.gas_used = alloy::primitives::U256::from(tx_output.gas_used);
        }
    }

    traces
}

fn normalize_call_frame_for_geth_output(mut frame: CallFrame) -> CallFrame {
    frame.calls = frame
        .calls
        .into_iter()
        .map(normalize_call_frame_for_geth_output)
        .collect();

    if matches!(frame.typ.as_str(), "STATICCALL") {
        frame.value = None;
    }

    if frame
        .output
        .as_ref()
        .is_some_and(|output| output.is_empty())
    {
        frame.output = None;
    }
    frame
}
