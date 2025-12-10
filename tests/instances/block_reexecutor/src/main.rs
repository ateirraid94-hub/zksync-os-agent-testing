use anyhow::Result;
use clap::Parser;

mod rpc_client;
mod rpc_oracle;

use rig::chain::RunConfig;
use rpc_client::RpcClient;
use ruint::aliases::U256;

#[derive(Parser, Debug)]
#[command(author, version, about = "Re-execute blocks using external RPC")]
struct Args {
    /// RPC endpoint URL
    #[arg(long, default_value = "http://localhost:8545")]
    endpoint: String,

    /// Block number to re-execute
    #[arg(long)]
    block_number: u64,

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
    println!("Block number: {}", args.block_number);

    // Fetch block data
    println!("Fetching block data...");
    let rpc_client = RpcClient::new(args.endpoint.clone());
    let block = rpc_client.get_block(args.block_number)?;

    let miner = block.result.header.beneficiary;
    let mut block_context = block.get_block_context();

    // TODO: temp pubdata approximation
    const NATIVE_PRICE: u128 = 1_000_000;
    const NATIVE_PER_GAS: u128 = 100;

    // Amount of native resource spent per blob.
    const NATIVE_PER_BLOB: u64 = 50_000_000;
    // Effective number of bytes stored in a blob for `SimpleCoder`.
    const BYTES_USED_PER_BLOB: u64 = (4096 - 1) * 31;
    // Amount of native resource spent per pubdata byte (assuming blob is fully filled).
    const NATIVE_PER_BLOB_BYTE: u64 = NATIVE_PER_BLOB / BYTES_USED_PER_BLOB;

    let native_price = block_context.native_price;
    println!("NATIVE PRICE: {}", native_price);

    let base_pubdata_price = U256::from(1_000_000);
    block_context.pubdata_price = base_pubdata_price * U256::from(50000) + U256::from(NATIVE_PER_BLOB_BYTE) * native_price; // For re-execution, set pubdata price to 0

    let transactions = block.clone().get_transactions();

    println!("Block {} has {} transactions", args.block_number, transactions.len());
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
        args.block_number,
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

    let block_output = chain.run_block_with_oracle_factory(
        transactions,
        Some(block_context),
        da_commitment_scheme,
        run_config,
        &oracle_factory,
    );

    
    println!("Block execution completed successfully!");
    println!("Block output: gas_used = {}", block_output.header.gas_used);
    println!("Block output: transactions = {:?}", block_output.tx_results);

    println!("Block re-execution completed");
    Ok(())
}