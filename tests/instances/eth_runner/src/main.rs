#![feature(slice_as_array)]
#![recursion_limit = "1024"]

use clap::{Parser, Subcommand};
use ethproofs::ethproofs_live_run;

use crate::ethproofs::EthProofsConnector;
mod block;
mod block_hashes;
mod calltrace;
pub(crate) mod dump_utils;
mod ethproofs;
mod live_run;
mod native_model;
mod post_check;
mod prestate;
mod receipts;
mod single_run;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a range of blocks live from RPC
    LiveRun {
        #[arg(long)]
        start_block: u64,
        #[arg(long)]
        end_block: u64,
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        db: String,
        #[arg(long)]
        witness_output_dir: Option<String>,
        #[arg(long)]
        skip_successful: bool,
        #[arg(long)]
        persist_all: bool,
        #[arg(long)]
        chain_id: Option<u64>,
    },
    // Run a single block from JSON files
    SingleRun {
        /// Path to the block JSON file
        #[arg(long)]
        block_dir: String,
        /// Path to the block hashes JSON file (optional)
        #[arg(long)]
        block_hashes: Option<String>,
        /// If set, the leaves of the tree are put in random
        /// positions to emulate real-world costs
        #[arg(long, action = clap::ArgAction::SetTrue)]
        randomized: bool,
        /// If set, will run prover input generation and dump it
        /// to the desired path.
        #[arg(long)]
        witness_output_dir: Option<String>,
        #[arg(long)]
        chain_id: Option<u64>,
    },
    // Export block ratios from DB
    ExportRatios {
        #[arg(long)]
        db: String,
        #[arg(long)]
        path: Option<String>,
    },
    // Show failed blocks
    ShowStatus {
        #[arg(long)]
        db: String,
    },
    // Prove an ethereum block for Ethproofs
    EthproofsRun {
        #[arg(long)]
        block_number: u64,
        #[arg(long)]
        reth_endpoint: String,
    },
    // Prove ethereum blocks for Ethproofs live
    EthproofsLiveRun {
        #[arg(long)]
        reth_endpoint: String,
    },
    // Prove ethereum blocks for Ethproofs live
    EthproofsWithProofs {
        #[arg(long)]
        reth_endpoint: String,
        // If staging is set, then proofs will be sent to staging server and we pick next available block.
        // If not set, then proofs will be sent to production server and we every 100th block.
        #[arg(long)]
        staging: bool,
        #[arg(long)]
        auth_token: String,
        #[arg(long)]
        cluster_id: u64,
        #[arg(long)]
        /// If set, will select blocks where (block_number % block_mod) == prover_id
        /// If not set, will pick 100th block in production and 10th block in staging.
        block_mod: Option<u64>,
        #[arg(long)]
        prover_id: Option<u64>,
    },
}

fn main() -> anyhow::Result<()> {
    rig::init_logger();
    let cli = Cli::parse();
    match cli.command {
        Command::SingleRun {
            block_dir,
            block_hashes,
            randomized,
            witness_output_dir,
            chain_id,
        } => crate::single_run::single_run(
            block_dir,
            block_hashes,
            randomized,
            witness_output_dir,
            chain_id,
        ),
        Command::LiveRun {
            start_block,
            end_block,
            endpoint,
            db,
            witness_output_dir,
            skip_successful,
            persist_all,
            chain_id,
        } => live_run::live_run(
            start_block,
            end_block,
            endpoint,
            db,
            witness_output_dir,
            skip_successful,
            persist_all,
            chain_id,
        ),
        Command::ExportRatios { db, path } => live_run::export_block_ratios(db, path),
        Command::ShowStatus { db } => live_run::show_status(db),
        Command::EthproofsRun {
            block_number,
            reth_endpoint,
        } => {
            ethproofs::ethproofs_run(block_number, &reth_endpoint, true, None)?;
            Ok(())
        }
        Command::EthproofsLiveRun { reth_endpoint } => ethproofs_live_run(&reth_endpoint),
        Command::EthproofsWithProofs {
            reth_endpoint,
            staging,
            auth_token,
            cluster_id,
            block_mod,
            prover_id,
        } => {
            let connector = EthProofsConnector::new(staging, auth_token, cluster_id);
            let block_mod = block_mod.unwrap_or_else(|| if staging { 10 } else { 100 });
            let prover_id = prover_id.unwrap_or_else(|| 0);
            ethproofs::ethproofs_with_proofs(&reth_endpoint, connector, (prover_id, block_mod))
        }
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn invoke_single_block() {
        crate::single_run::single_run("blocks/19299001".to_string(), None, false, None, Some(1))
            .expect("must succeed");
    }

    const NODE_URL: &str = "";
    const ACCOUNT_DIFFS_URL: &str = "";
    const BEACON_CHAIN_URL: &str = "";

    #[test]
    fn run_dump() {
        let block_number = 23292836;
        let _ = std::fs::create_dir(&format!("blocks/{}", block_number));
        crate::dump_utils::dump_eth_block(
            block_number,
            NODE_URL,
            None,
            // Some(ACCOUNT_DIFFS_URL),
            BEACON_CHAIN_URL,
            format!("blocks/{}", block_number),
        )
        .expect("must dump block data");
    }

    #[test]
    fn invoke_single_eth_block() {
        let block_number = 23292836;
        crate::single_run::single_eth_run::<true>(format!("blocks/{}", block_number), Some(1))
            .expect("must succeed");
    }
}
