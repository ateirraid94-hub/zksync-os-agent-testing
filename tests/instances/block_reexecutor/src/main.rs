use alloy::{primitives::B256, rpc::types::trace::geth::CallFrame};
use anyhow::Result;
use clap::Parser;

mod rpc_client;
mod rpc_oracle;

use rig::{chain::RunConfig, forward_system::system::tracers::call_tracer::CallTracer};
use rpc_client::RpcClient;

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

    // Initialize logger
    rig::init_logger();

    println!("Starting block re-execution");
    println!("Endpoint: {}", args.endpoint);
    println!("Block hash: {:?}", args.block_hash);

    // Fetch block data
    println!("Fetching block data...");
    let rpc_client = RpcClient::new(args.endpoint.clone());
    let block = rpc_client.get_block_by_hash(args.block_hash)?;
    let block_number = block.result.header.number;
    println!("Fetched block number: {}", block_number);

    let miner = block.result.header.beneficiary;
    let mut block_context = block.get_block_context();


    println!("Fetching block metadata...");
    // TODO should be replaced with hash or better replay record fetching once available in RPC
    // Otherwise params can be inconsistent with the actually used
    let block_metadata = rpc_client.get_block_metadata(block_number)?;

    println!("Native price: {}", block_metadata.result.native_price);
    println!("Pubdata price: {}", block_metadata.result.pubdata_price_per_byte);

    block_context.native_price = block_metadata.result.native_price;
    block_context.pubdata_price = block_metadata.result.pubdata_price_per_byte;

    let transactions = block.clone().get_transactions();

    println!("Block {} has {} transactions", block_number, transactions.len());
    println!("Block hash: {:?}", block.result.header.hash);
    println!("Block miner: {:?}", miner);
    println!("Block gas used: {} / {}", block.result.header.gas_used, block.result.header.gas_limit);

    // For now, skip transaction execution since it requires complex encoding
    if transactions.is_empty() {
        println!("No transactions to execute, skipping block");
        return Ok(());
    }

    println!("Block context: timestamp={}, gas_limit={}, coinbase={:?}",
          block_context.timestamp, block_context.gas_limit, block_context.coinbase);

    
    let oracle_factory = rpc_oracle::RpcValueOracleFactory::new(
        args.endpoint.clone(),
        block_number,
    );

    let chain_id = rpc_client.get_chain_id()?;

    let mut chain = rig::Chain::empty(Some(chain_id));

    // For now, skip transaction processing due to complexity of EncodedTx conversion
    // We'll run an empty block to test the oracle_factory functionality
    println!("Running block with {} transactions using RPC oracle...", transactions.len());

    let da_commitment_scheme = None; // Use default DA commitment scheme
    let run_config = Some(RunConfig {
        profiler_config: None,
        witness_output_file: None,
        app: None,
        only_forward: true,
        check_storage_diff_hashes: false,
    }); // Use default run config

    let mut tracer = CallTracer::default();

    let block_output = chain.run_block_with_oracle_factory_and_tracer(
        transactions,
        Some(block_context),
        da_commitment_scheme,
        run_config,
        &oracle_factory,
        &mut tracer,
    );

    
    println!("Block execution completed successfully!");
    println!("Block output: gas_used = {}", block_output.header.gas_used);
    println!("Block output: transactions = {:?}", block_output.tx_results);

    println!("Block re-execution completed");


    let trace = tracer.transactions.into_iter().map(|tx| CallFrame::from(tx)).collect::<Vec<_>>();

    // Save the tracer output to a file for further analysis
    let tracer_output_path = format!("tracer_output_{}.json", block_number);
    std::fs::write(&tracer_output_path, serde_json::to_string_pretty(&trace)?)?;
    println!("Tracer output saved to {}", tracer_output_path);

    Ok(())
}