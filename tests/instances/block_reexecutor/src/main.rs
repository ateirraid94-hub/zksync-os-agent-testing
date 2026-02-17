use alloy::{
    primitives::{Address, B256},
    rpc::types::trace::geth::CallFrame,
    serde::storage,
};
use anyhow::Result;
use clap::Parser;

mod rpc_client;
mod rpc_oracle;

use rig::{
    chain::RunConfig,
    forward_system::{run::ReadStorage, system::tracers::call_tracer::CallTracer},
    utils::AccountProperties,
    zk_ee::{common_structs::derive_flat_storage_key, utils::Bytes32},
    zksync_os_interface::{
        self,
        traits::{PreimageSource, ReadStorage as InterfaceReadStorage},
    },
    BlockContext, Chain,
};
use rpc_client::RpcClient;
use zksync_os_revm_runner::{
    convert_alloy::{FromAlloy, IntoAlloy},
    revm_runner,
    revm_state_provider::ViewState,
};

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

    // For now, skip transaction execution since it requires complex encoding
    if transactions.is_empty() {
        println!("No transactions to execute, skipping block");
        return Ok(());
    }

    println!(
        "Block context: timestamp={}, gas_limit={}, coinbase={:?}",
        block_context.timestamp, block_context.gas_limit, block_context.coinbase
    );

    let oracle_factory =
        rpc_oracle::RpcValueOracleFactory::new(args.endpoint.clone(), block_number);

    let chain_id = rpc_client.get_chain_id()?;

    let mut chain = rig::Chain::empty(Some(chain_id));

    // For now, skip transaction processing due to complexity of EncodedTx conversion
    // We'll run an empty block to test the oracle_factory functionality
    println!(
        "Running block with {} transactions using RPC oracle...",
        transactions.len()
    );

    let da_commitment_scheme = None; // Use default DA commitment scheme
    let run_config = Some(RunConfig {
        profiler_config: None,
        witness_output_file: None,
        app: None,
        only_forward: true,
        check_storage_diff_hashes: false,
        not_update_state_after_block_execution: true, // don't apply state changes to the chain, since we want to keep it unchanged for revm
    }); // Use default run config

    let mut tracer = CallTracer::default();

    let block_output = chain.run_block_with_oracle_factory_and_tracer(
        transactions,
        Some(block_context.clone()),
        da_commitment_scheme,
        run_config,
        &oracle_factory,
        &mut tracer,
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

    // Insert preimages and storage values into the chain's state view so that they can be accessed during REVM execution
    for (hash, preimage) in preimages {
        chain.preimage_source.inner.insert(hash, preimage);
    }

    for ((address, slot), value) in storage {
        let flat_key = derive_flat_storage_key(&address, &slot);
        chain.state_tree.cold_storage.insert(flat_key, value);
    }

    println!("Block execution completed successfully!");
    println!("Block output: gas_used = {}", block_output.header.gas_used);
    println!("Block output: transactions = {:?}", block_output.tx_results);

    println!("Block re-execution completed");

    let trace = tracer
        .transactions
        .into_iter()
        .map(|tx| CallFrame::from(tx))
        .collect::<Vec<_>>();

    // Save the tracer output to a file for further analysis
    let tracer_output_path = format!("tracer_output_{}.json", block_number);
    std::fs::write(&tracer_output_path, serde_json::to_string_pretty(&trace)?)?;
    println!("Tracer output saved to {}", tracer_output_path);

    println!("Runnning ZKsync OS REVM");

    let block_context = generate_block_context_interface(&chain, &block_context);

    let state_view = ChainStateView { chain: chain };

    let raw_transactions = block.get_transactions_raw();

    let mut revm_runner = revm_runner::RevmRunner::new(state_view);

    revm_runner.run(raw_transactions, block_context, Some(block_output))?;

    Ok(())
}

#[derive(Clone)]
pub struct ChainStateView {
    pub chain: Chain,
}

impl PreimageSource for ChainStateView {
    fn get_preimage(&mut self, hash: B256) -> Option<Vec<u8>> {
        let hash = Bytes32::from_alloy(hash);
        self.chain.preimage_source.inner.get(&hash).cloned()
    }
}

impl InterfaceReadStorage for ChainStateView {
    fn read(&mut self, key: B256) -> Option<B256> {
        let key = Bytes32::from_alloy(key);
        let value = self.chain.state_tree.read(key);

        value.map(|v| v.into_alloy())
    }
}

impl ViewState for ChainStateView {
    fn get_account(&mut self, address: Address) -> Option<AccountProperties> {
        let address = ruint::aliases::B160::from_alloy(address);
        self.chain.get_account_properties_maybe(&address)
    }

    fn account_nonce(&mut self, address: Address) -> Option<u64> {
        let account = self.get_account(address);

        account.map(|account| account.nonce)
    }
}

use zksync_os_interface::types::BlockContext as BlockContextInterface;
pub fn generate_block_context_interface(
    chain: &Chain,
    rig_block_context: &BlockContext,
) -> BlockContextInterface {
    BlockContextInterface {
        block_number: chain.next_block_number(),
        timestamp: rig_block_context.timestamp,
        eip1559_basefee: rig_block_context.eip1559_basefee,
        chain_id: chain.chain_id(),
        block_hashes: zksync_os_interface::types::BlockHashes(chain.block_hashes()),
        pubdata_price: rig_block_context.pubdata_price,
        native_price: rig_block_context.native_price,
        coinbase: rig_block_context.coinbase.into_alloy(),
        gas_limit: rig_block_context.gas_limit,
        pubdata_limit: rig_block_context.pubdata_limit,
        mix_hash: rig_block_context.mix_hash,
        execution_version: 0, // TODO meaningless here
        blob_fee: Default::default(),
    }
}
