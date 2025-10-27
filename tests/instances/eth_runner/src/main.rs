#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![feature(slice_as_array)]
#![recursion_limit = "1024"]

use clap::{Parser, Subcommand};
use ethproofs::ethproofs_live_run;
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
        } => ethproofs::ethproofs_run(block_number, &reth_endpoint),
        Command::EthproofsLiveRun { reth_endpoint } => ethproofs_live_run(&reth_endpoint),
    }
}

#[cfg(test)]
mod test {
    use execution_utils::{setups::prover::worker::Worker, unrolled::{UnrolledProgramProof, UnrolledProgramSetup}};
    use risc_v_simulator::cycle::IMStandardIsaConfigWithUnsignedMulDiv;

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
        let block_number = 23620012;
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
        // 23619282
        // 23620012
        let block_number = 23620012;
        crate::single_run::single_eth_run::<true>(format!("blocks/{}", block_number), Some(1))
            .expect("must succeed");
    }

    #[test]
    fn prove_single_block() {
        use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
        use std::{io::Read, path::Path};
        use execution_utils::setups::read_binary;
        use std::fs::File;

        let block_number = 23620012;

        let mut file = File::open(&format!("{}_witness", block_number)).expect("should open file");
        let mut witness = vec![];
        file.read_to_end(&mut witness).expect("must read witness from file");
        let witness = hex::decode(core::str::from_utf8(&witness).unwrap()).unwrap();
        assert_eq!(witness.len() % 4, 0);
        let witness: Vec<_> = witness.as_chunks::<4>().0.iter().map(|el| u32::from_be_bytes(*el)).collect();
        let source = QuasiUARTSource::new_with_reads(witness);
        let (binary, binary_u32) = read_binary(Path::new("../../../zksync_os/app.bin"));
        let (text, text_u32) = read_binary(Path::new("../../../zksync_os/app.text"));
        println!("Computing setup");
        let setup = execution_utils::unrolled::compute_setup_for_machine_configuration::<IMStandardIsaConfigWithUnsignedMulDiv>(&binary, &text);
        serde_json::to_writer_pretty(File::create("setup.json").unwrap(), &setup).unwrap();
        let worker = Worker::new_with_num_threads(8);
        println!("Computing proof");
        let proof = execution_utils::unrolled::prove_unrolled_for_machine_configuration_into_program_proof::<IMStandardIsaConfigWithUnsignedMulDiv>(&binary_u32, &text_u32, 1 << 31, source, 1 << 30, &worker);
        serde_json::to_writer_pretty(File::create("proof.json").unwrap(), &proof).unwrap();
        // println!("Verifying...");
        // let result = execution_utils::unrolled::verify_unrolled_base_layer_for_machine_configuration::<IMStandardIsaConfigWithUnsignedMulDiv>(&proof, &setup).expect("is valid proof");
        // assert!(result.iter().all(|el| *el == 0) == false);
        // dbg!(result);
    }

    // #[test]
    // fn verify_single_block() {
    //     use std::fs::File;

    //     let setup: UnrolledProgramSetup = serde_json::from_reader(&File::open("setup.json").unwrap()).unwrap();
    //     let proof: UnrolledProgramProof = serde_json::from_reader(&File::open("proof.json").unwrap()).unwrap();

    //     println!("Verifying...");
    //     let result = execution_utils::unrolled::verify_unrolled_base_layer_for_machine_configuration::<IMStandardIsaConfigWithUnsignedMulDiv>(&proof, &setup).expect("is valid proof");
    //     assert!(result.iter().all(|el| *el == 0) == false);
    //     dbg!(result);
    // }

    #[test]
    fn prove_recursion_over_base() {
        use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
        use std::{io::Read, path::Path};
        use execution_utils::setups::read_binary;
        use std::fs::File;
        use risc_v_simulator::cycle::IWithoutByteAccessIsaConfigWithDelegation;

        let setup: UnrolledProgramSetup = serde_json::from_reader(&File::open("setup.json").unwrap()).unwrap();
        let proof: UnrolledProgramProof = serde_json::from_reader(&File::open("proof.json").unwrap()).unwrap();

        // println!("Verifying out of circuit ...");
        // let result = execution_utils::unrolled::verify_unrolled_base_layer_for_machine_configuration::<IWithoutByteAccessIsaConfigWithDelegation>(&proof, &setup).expect("is valid proof");
        // assert!(result.iter().all(|el| *el == 0) == false);

        let witness = proof.flatten_into_responses(&[1984, 1991, 1994, 1995]);

        let source = QuasiUARTSource::new_with_reads(witness);
        let (binary, binary_u32) = read_binary(Path::new("../../../../zksync-airbender/tools/verifier/unrolled_base_layer.bin"));
        let (text, text_u32) = read_binary(Path::new("../../../../zksync-airbender/tools/verifier/unrolled_base_layer.text"));
        println!("Computing setup");
        let setup = execution_utils::unrolled::compute_setup_for_machine_configuration::<IWithoutByteAccessIsaConfigWithDelegation>(&binary, &text);
        serde_json::to_writer_pretty(File::create("setup_recursion_over_base.json").unwrap(), &setup).unwrap();
        let worker = Worker::new_with_num_threads(8);
        println!("Computing proof");
        let proof = execution_utils::unrolled::prove_unrolled_for_machine_configuration_into_program_proof::<IWithoutByteAccessIsaConfigWithDelegation>(&binary_u32, &text_u32, 1 << 31, source, 1 << 30, &worker);
        serde_json::to_writer_pretty(File::create("proof_recursion_over_base.json").unwrap(), &proof).unwrap();
    }
}
