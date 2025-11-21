#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
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

pub use single_run::{create_eth_run_oracle, read_eth_run_oracle};

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
    EthproofsWithProofsNoSubmission {
        #[arg(long)]
        reth_endpoint: String,
        /// If set, will select blocks where (block_number % block_mod) == prover_id
        /// If not set, will pick 100th block in production and 10th block in staging.
        block_mod: Option<u64>,
        #[arg(long)]
        prover_id: Option<u64>,
    },
    FetchWitness {
        #[arg(long)]
        reth_endpoint: String,
        #[arg(long)]
        block_number: u64,
        #[arg(long)]
        witness_output_dir: String,
    },
    ProveWithWitness {
        #[arg(long)]
        witness_input: String,
        #[arg(long)]
        worker_threads: Option<usize>,
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
            ethproofs::ethproofs_with_proofs(
                &reth_endpoint,
                Some(connector),
                (prover_id, block_mod),
            )
        }
        Command::EthproofsWithProofsNoSubmission {
            reth_endpoint,
            block_mod,
            prover_id,
        } => {
            let block_mod = block_mod.unwrap_or_else(|| 10);
            let prover_id = prover_id.unwrap_or_else(|| 0);
            ethproofs::ethproofs_with_proofs(&reth_endpoint, None, (prover_id, block_mod))
        }
        Command::FetchWitness {
            reth_endpoint,
            block_number,
            witness_output_dir,
        } => ethproofs::ethproofs_fetch_witness(&reth_endpoint, block_number, &witness_output_dir),
        Command::ProveWithWitness {
            witness_input,
            worker_threads,
        } => {
            panic!("disabled due to CLI need");
            // ethproofs::ethproofs_prove_with_witness(&witness_input, worker_threads.unwrap_or(16))
        }
    }
}

#[cfg(test)]
mod test {
    use execution_utils::{
        setups::prover::{common_constants, worker::Worker},
        unrolled::{UnrolledProgramProof, UnrolledProgramSetup},
    };
    use risc_v_simulator::{cycle::IMStandardIsaConfigWithUnsignedMulDiv, setup};

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
        let block_number = 23846054;
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
        let block_number = 23846054;
        // crate::single_run::single_eth_run::<true>(format!("blocks/{}", block_number), Some(1))
        //     .expect("must succeed");

        crate::single_run::single_minimal_eth_run::<true>(format!("blocks/{}", block_number), Some(1))
            .expect("must succeed");
    }

    #[test]
    fn prove_single_block() {
        use execution_utils::setups::read_and_pad_binary;
        use std::fs::File;
        use std::path::Path;

        let oracle = {
            use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
            use std::io::Read;

            let block_number = 23620012;
            let mut file =
                File::open(&format!("{}_witness", block_number)).expect("should open file");
            let mut witness = vec![];
            file.read_to_end(&mut witness)
                .expect("must read witness from file");
            let witness = hex::decode(core::str::from_utf8(&witness).unwrap()).unwrap();
            assert_eq!(witness.len() % 4, 0);
            let witness: Vec<_> = witness
                .as_chunks::<4>()
                .0
                .iter()
                .map(|el| u32::from_be_bytes(*el))
                .collect();
            let source = QuasiUARTSource::new_with_reads(witness);

            source
        };

        // let oracle = {
        // use crate::read_eth_run_oracle;
        //     let block_number = 23832885;
        //     let oracle = read_eth_run_oracle(format!("blocks/{}", block_number)).expect("must create proof oracle");

        //     oracle
        // };

        let (binary, binary_u32) = read_and_pad_binary(Path::new("../../../zksync_os/app.bin"));
        let (text, text_u32) = read_and_pad_binary(Path::new("../../../zksync_os/app.text"));
        println!("Computing setup");
        let setup = execution_utils::unrolled::compute_setup_for_machine_configuration::<
            IMStandardIsaConfigWithUnsignedMulDiv,
        >(&binary, &text);
        serde_json::to_writer_pretty(File::create("setup.json").unwrap(), &setup).unwrap();
        let compiled_layouts =
            execution_utils::setups::get_unrolled_circuits_artifacts_for_machine_type::<
                IMStandardIsaConfigWithUnsignedMulDiv,
            >(&binary_u32);
        serde_json::to_writer_pretty(File::create("layouts.json").unwrap(), &compiled_layouts)
            .unwrap();
        let worker = Worker::new_with_num_threads(8);
        println!("Computing proof");
        let proof =
            execution_utils::unrolled::prove_unrolled_for_machine_configuration_into_program_proof::<
                IMStandardIsaConfigWithUnsignedMulDiv,
            >(&binary_u32, &text_u32, 1 << 31, oracle, 1 << 30, &worker);
        serde_json::to_writer_pretty(File::create("proof.json").unwrap(), &proof).unwrap();
        // println!("Verifying...");
        // let result = execution_utils::unrolled::verify_unrolled_base_layer_for_machine_configuration::<IMStandardIsaConfigWithUnsignedMulDiv>(&proof, &setup).expect("is valid proof");
        // assert!(result.iter().all(|el| *el == 0) == false);
        // dbg!(result);
    }

    // #[test]
    // fn verify_single_block() {
    //     use execution_utils::setups::read_and_pad_binary;
    //     use std::fs::File;
    //     use std::path::Path;

    //     let (_, binary_u32) = read_and_pad_binary(Path::new("../../../zksync_os/app.bin"));

    //     let setup: UnrolledProgramSetup = serde_json::from_reader(&File::open("setup.json").unwrap()).unwrap();
    //     let proof: UnrolledProgramProof = serde_json::from_reader(&File::open("proof.json").unwrap()).unwrap();

    //     println!("Verifying...");
    //     let cicuit_set = execution_utils::unrolled::get_unrolled_circuits_artifacts_for_machine_type::<IMStandardIsaConfigWithUnsignedMulDiv>(&binary_u32);
    //     let result = execution_utils::unrolled::verify_unrolled_layer_proof(&proof, &setup, &cicuit_set, true).expect("is valid proof");
    //     assert!(result.iter().all(|el| *el == 0) == false);
    //     dbg!(result);
    // }

    // #[test]
    // fn run_recursion_over_base_in_simulator() {
    //     use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
    //     use std::fs::File;
    //     use std::{io::Read, path::Path};
    //     use execution_utils::setups::read_and_pad_binary;

    //     let setup: UnrolledProgramSetup =
    //         serde_json::from_reader(&File::open("setup.json").unwrap()).unwrap();
    //     let proof: UnrolledProgramProof =
    //         serde_json::from_reader(&File::open("proof.json").unwrap()).unwrap();

    //     let (_, binary_u32) = read_and_pad_binary(Path::new("../../../zksync_os/app.bin"));
    //     let cicuit_set = execution_utils::unrolled::get_unrolled_circuits_artifacts_for_machine_type::<IMStandardIsaConfigWithUnsignedMulDiv>(&binary_u32);

    //     assert_eq!(setup.circuit_families_setups.len(), 6);

    //     for (family, proofs) in proof.circuit_families_proofs.iter() {
    //         println!("{} proofs for family {}", proofs.len(), family);
    //         let setup_cap = &setup.circuit_families_setups[family];
    //         for proof in proofs.iter() {
    //             assert_eq!(proof.setup_tree_caps.len(), setup_cap.len());
    //             for (a, b) in proof.setup_tree_caps.iter().zip(setup_cap.iter()) {
    //                 assert_eq!(&a.cap[..], &b.cap[..]);
    //             }
    //         }
    //     }

    //     let mut witness = setup.flatten_for_recursion();
    //     witness.extend(proof.flatten_into_responses(&[1984, 1991, 1994, 1995]));
    //     let source = QuasiUARTSource::new_with_reads(witness);

    //     let (result, _) = zksync_os_runner::run_transpiler::run_and_get_effective_cycles::<{ common_constants::rom::ROM_SECOND_WORD_BITS }>(
    //         Path::new("../../../../zksync-airbender/tools/verifier/unrolled_base_layer.bin").to_path_buf(),
    //         Path::new("../../../../zksync-airbender/tools/verifier/unrolled_base_layer.text").to_path_buf(),
    //         None,
    //         1 << 32,
    //         source,
    //     );

    //     dbg!(result);
    // }

    #[test]
    fn check_base_layer_recursion_chain_params() {
        use std::fs::File;
        use std::{io::Read, path::Path};

        let setup: UnrolledProgramSetup =
            serde_json::from_reader(&File::open("setup.json").unwrap()).unwrap();
        dbg!(setup.end_params);
        let (hash_chain, preimage) = UnrolledProgramSetup::begin_recursion_chain(&setup.end_params);
        dbg!(hash_chain);
        dbg!(preimage);
    }

    #[test]
    fn prove_recursion_over_base() {
        use execution_utils::setups::read_and_pad_binary;
        use execution_utils::setups::CompiledCircuitsSet;
        use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
        use risc_v_simulator::cycle::IWithoutByteAccessIsaConfigWithDelegation;
        use std::fs::File;
        use std::{io::Read, path::Path};

        let base_layer_setup: UnrolledProgramSetup =
            serde_json::from_reader(&File::open("setup.json").unwrap()).unwrap();
        let proof: UnrolledProgramProof =
            serde_json::from_reader(&File::open("proof.json").unwrap()).unwrap();
        let layout: CompiledCircuitsSet =
            serde_json::from_reader(&File::open("layouts.json").unwrap()).unwrap();

        for (family, proofs) in proof.circuit_families_proofs.iter() {
            println!("{} proofs for family {}", proofs.len(), family);
        }
        for (delegation_type, proofs) in proof.delegation_proofs.iter() {
            println!("{} proofs for delegation {}", proofs.len(), delegation_type);
        }

        let responses =
            execution_utils::unrolled::flatten_proof_into_responses_for_unrolled_recursion(
                &proof,
                &base_layer_setup,
                &layout,
                true,
            );
        let source = QuasiUARTSource::new_with_reads(responses);

        let (binary, binary_u32) = read_and_pad_binary(Path::new(
            "../../../../zksync-airbender/tools/verifier/recursion_in_unrolled_layer.bin",
        ));
        let (text, text_u32) = read_and_pad_binary(Path::new(
            "../../../../zksync-airbender/tools/verifier/recursion_in_unrolled_layer.text",
        ));
        println!("Computing setup");
        let setup = execution_utils::unrolled::compute_setup_for_machine_configuration::<
            IWithoutByteAccessIsaConfigWithDelegation,
        >(&binary, &text);
        serde_json::to_writer_pretty(
            File::create("setup_recursion_over_base.json").unwrap(),
            &setup,
        )
        .unwrap();
        let compiled_layouts =
            execution_utils::setups::get_unrolled_circuits_artifacts_for_machine_type::<
                IWithoutByteAccessIsaConfigWithDelegation,
            >(&binary_u32);
        serde_json::to_writer_pretty(
            File::create("layouts_recursion_over_base.json").unwrap(),
            &compiled_layouts,
        )
        .unwrap();
        let worker = Worker::new_with_num_threads(8);
        println!("Computing proof");
        let mut proof =
            execution_utils::unrolled::prove_unrolled_for_machine_configuration_into_program_proof::<
                IWithoutByteAccessIsaConfigWithDelegation,
            >(&binary_u32, &text_u32, 1 << 31, source, 1 << 30, &worker);

        // make a hash chain
        let (hash_chain, preimage) =
            UnrolledProgramSetup::begin_recursion_chain(&base_layer_setup.end_params);
        proof.recursion_chain_hash = Some(hash_chain);
        proof.recursion_chain_preimage = Some(preimage);
        dbg!(proof.recursion_chain_hash);
        dbg!(proof.recursion_chain_preimage);
        serde_json::to_writer_pretty(
            File::create("proof_recursion_over_base.json").unwrap(),
            &proof,
        )
        .unwrap();
    }

    // #[test]
    // fn verify_recursion_proof() {
    //     use std::fs::File;

    //     let setup: UnrolledProgramSetup = serde_json::from_reader(&File::open("setup_recursion_over_base.json").unwrap()).unwrap();
    //     let proof: UnrolledProgramProof = serde_json::from_reader(&File::open("proof_recursion_over_base.json").unwrap()).unwrap();

    // assert_eq!(setup.circuit_families_setups.len(), 4);
    //     println!("Verifying...");
    //     let result = execution_utils::unrolled::verify_unrolled_recursion_layer_via_full_statement_verifier(&proof, &setup).expect("is valid proof");
    //     assert!(result.iter().all(|el| *el == 0) == false);
    //     dbg!(result);
    // }

    // #[test]
    // fn run_recursion_over_recursion_in_simulator() {
    //     use execution_utils::setups::CompiledCircuitsSet;
    //     use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
    //     use std::fs::File;
    //     use std::{io::Read, path::Path};

    //     let setup: UnrolledProgramSetup =
    //         serde_json::from_reader(&File::open("setup_recursion_over_base.json").unwrap()).unwrap();
    //     let proof: UnrolledProgramProof =
    //         serde_json::from_reader(&File::open("proof_recursion_over_base.json").unwrap()).unwrap();
    //     let layout: CompiledCircuitsSet =
    //         serde_json::from_reader(&File::open("layouts_recursion_over_base.json").unwrap()).unwrap();

    //     assert_eq!(setup.circuit_families_setups.len(), 4);

    //     for (family, proofs) in proof.circuit_families_proofs.iter() {
    //         println!("{} proofs for family {}", proofs.len(), family);
    //         let setup_cap = &setup.circuit_families_setups[family];
    //         for proof in proofs.iter() {
    //             assert_eq!(proof.setup_tree_caps.len(), setup_cap.len());
    //             for (a, b) in proof.setup_tree_caps.iter().zip(setup_cap.iter()) {
    //                 assert_eq!(&a.cap[..], &b.cap[..]);
    //             }
    //         }
    //     }

    //     let mut witness = setup.flatten_for_recursion();
    //     witness.extend(proof.flatten_into_responses(&[1984, 1991]));
    //     let source = QuasiUARTSource::new_with_reads(witness);

    //     let (result, _) = zksync_os_runner::run_transpiler::run_and_get_effective_cycles::<{ common_constants::rom::ROM_SECOND_WORD_BITS }>(
    //         Path::new("../../../../zksync-airbender/tools/verifier/unrolled_recursion_layer.bin").to_path_buf(),
    //         Path::new("../../../../zksync-airbender/tools/verifier/unrolled_recursion_layer.text").to_path_buf(),
    //         None,
    //         1 << 32,
    //         source,
    //     );

    //     dbg!(result);
    // }

    #[test]
    fn prove_recursion_over_recursion() {
        use execution_utils::setups::read_and_pad_binary;
        use execution_utils::setups::CompiledCircuitsSet;
        use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
        use risc_v_simulator::cycle::IWithoutByteAccessIsaConfigWithDelegation;
        use std::fs::File;
        use std::{io::Read, path::Path};

        let recursion_over_base_setup: UnrolledProgramSetup =
            serde_json::from_reader(&File::open("setup_recursion_over_base.json").unwrap())
                .unwrap();
        let recursion_over_base_proof: UnrolledProgramProof =
            serde_json::from_reader(&File::open("proof_recursion_over_base.json").unwrap())
                .unwrap();
        let recursion_over_base_layout: CompiledCircuitsSet =
            serde_json::from_reader(&File::open("layouts_recursion_over_base.json").unwrap())
                .unwrap();

        for (family, proofs) in recursion_over_base_proof.circuit_families_proofs.iter() {
            println!("{} proofs for family {}", proofs.len(), family);
        }
        for (delegation_type, proofs) in recursion_over_base_proof.delegation_proofs.iter() {
            println!("{} proofs for delegation {}", proofs.len(), delegation_type);
        }

        let mut witness = recursion_over_base_setup.flatten_for_recursion();
        witness.extend(
            recursion_over_base_proof
                .flatten_into_responses(&[1984, 1991], &recursion_over_base_layout),
        );
        let source = QuasiUARTSource::new_with_reads(witness);

        let (binary, binary_u32) = read_and_pad_binary(Path::new(
            "../../../../zksync-airbender/tools/verifier/unrolled_recursion_layer.bin",
        ));
        let (text, text_u32) = read_and_pad_binary(Path::new(
            "../../../../zksync-airbender/tools/verifier/unrolled_recursion_layer.text",
        ));
        println!("Computing setup");
        let setup = execution_utils::unrolled::compute_setup_for_machine_configuration::<
            IWithoutByteAccessIsaConfigWithDelegation,
        >(&binary, &text);
        serde_json::to_writer_pretty(
            File::create("setup_recursion_over_recursion.json").unwrap(),
            &setup,
        )
        .unwrap();
        let compiled_layouts =
            execution_utils::setups::get_unrolled_circuits_artifacts_for_machine_type::<
                IWithoutByteAccessIsaConfigWithDelegation,
            >(&binary_u32);
        serde_json::to_writer_pretty(
            File::create("layouts_recursion_over_recursion.json").unwrap(),
            &compiled_layouts,
        )
        .unwrap();
        let worker = Worker::new_with_num_threads(8);
        println!("Computing proof");
        let mut proof =
            execution_utils::unrolled::prove_unrolled_for_machine_configuration_into_program_proof::<
                IWithoutByteAccessIsaConfigWithDelegation,
            >(&binary_u32, &text_u32, 1 << 31, source, 1 << 30, &worker);

        let existing_hash_chain = recursion_over_base_proof.recursion_chain_hash.unwrap();
        let existing_preimage = recursion_over_base_proof.recursion_chain_preimage.unwrap();
        // extend a hash chain
        let (hash_chain, preimage) = UnrolledProgramSetup::continue_recursion_chain(
            &recursion_over_base_setup.end_params,
            &existing_hash_chain,
            &existing_preimage,
        );
        proof.recursion_chain_hash = Some(hash_chain);
        proof.recursion_chain_preimage = Some(preimage);
        dbg!(proof.recursion_chain_hash);
        dbg!(proof.recursion_chain_preimage);
        serde_json::to_writer_pretty(
            File::create("proof_recursion_over_recursion.json").unwrap(),
            &proof,
        )
        .unwrap();

        bincode::serde::encode_into_std_write(
            &(
                proof.circuit_families_proofs.clone(),
                proof.delegation_proofs.clone(),
                proof.inits_and_teardowns_proofs.clone(),
            ),
            &mut File::create("proof_recursion_over_recursion.bin").unwrap(),
            bincode::config::standard(),
        )
        .unwrap();

        for (family, proofs) in proof.circuit_families_proofs.iter() {
            println!("{} proofs for family {}", proofs.len(), family);
        }
        for (delegation_type, proofs) in proof.delegation_proofs.iter() {
            println!("{} proofs for delegation {}", proofs.len(), delegation_type);
        }
    }

    #[test]
    fn prove_unified_recursion() {
        use execution_utils::setups::read_and_pad_binary;
        use execution_utils::setups::CompiledCircuitsSet;
        use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
        use risc_v_simulator::cycle::IWithoutByteAccessIsaConfigWithDelegation;
        use std::fs::File;
        use std::{io::Read, path::Path};

        let recursion_over_recursion_setup: UnrolledProgramSetup =
            serde_json::from_reader(&File::open("setup_recursion_over_recursion.json").unwrap())
                .unwrap();
        let recursion_over_recursion_proof: UnrolledProgramProof =
            serde_json::from_reader(&File::open("proof_recursion_over_recursion.json").unwrap())
                .unwrap();
        let recursion_over_recursion_layout: CompiledCircuitsSet =
            serde_json::from_reader(&File::open("layouts_recursion_over_recursion.json").unwrap())
                .unwrap();

        for (family, proofs) in recursion_over_recursion_proof
            .circuit_families_proofs
            .iter()
        {
            println!("{} proofs for family {}", proofs.len(), family);
        }
        for (delegation_type, proofs) in recursion_over_recursion_proof.delegation_proofs.iter() {
            println!("{} proofs for delegation {}", proofs.len(), delegation_type);
        }

        let mut witness = recursion_over_recursion_setup.flatten_for_recursion();
        witness.extend(
            recursion_over_recursion_proof
                .flatten_into_responses(&[1984, 1991], &recursion_over_recursion_layout),
        );
        let source = QuasiUARTSource::new_with_reads(witness);

        let (binary, binary_u32) = read_and_pad_binary(Path::new(
            "../../../../zksync-airbender/tools/verifier/unrolled_recursion_layer.bin",
        ));
        let (text, text_u32) = read_and_pad_binary(Path::new(
            "../../../../zksync-airbender/tools/verifier/unrolled_recursion_layer.text",
        ));

        println!("Computing setup");
        let setup =
            execution_utils::unified_circuit::compute_unified_setup_for_machine_configuration::<
                IWithoutByteAccessIsaConfigWithDelegation,
            >(&binary, &text);
        serde_json::to_writer_pretty(
            File::create("unified_setup_over_recursion.json").unwrap(),
            &setup,
        )
        .unwrap();
        let compiled_layouts =
            execution_utils::setups::get_unified_circuit_artifact_for_machine_type::<
                IWithoutByteAccessIsaConfigWithDelegation,
            >(&binary_u32);
        serde_json::to_writer_pretty(
            File::create("unified_layout_over_recursion.json").unwrap(),
            &compiled_layouts,
        )
        .unwrap();
        let worker = Worker::new_with_num_threads(8);
        println!("Computing proof");
        let mut proof =
            execution_utils::unified_circuit::prove_unified_for_machine_configuration_into_program_proof::<
                IWithoutByteAccessIsaConfigWithDelegation,
            >(&binary_u32, &text_u32, 1 << 31, source, 1 << 30, &worker);

        let existing_hash_chain = recursion_over_recursion_proof.recursion_chain_hash.unwrap();
        let existing_preimage = recursion_over_recursion_proof
            .recursion_chain_preimage
            .unwrap();
        // extend a hash chain
        let (hash_chain, preimage) = UnrolledProgramSetup::continue_recursion_chain(
            &recursion_over_recursion_setup.end_params,
            &existing_hash_chain,
            &existing_preimage,
        );
        proof.recursion_chain_hash = Some(hash_chain);
        proof.recursion_chain_preimage = Some(preimage);
        dbg!(proof.recursion_chain_hash);
        dbg!(proof.recursion_chain_preimage);
        serde_json::to_writer_pretty(
            File::create("unified_proof_over_recursion.json").unwrap(),
            &proof,
        )
        .unwrap();

        bincode::serde::encode_into_std_write(
            &(
                proof.circuit_families_proofs.clone(),
                proof.delegation_proofs.clone(),
            ),
            &mut File::create("unified_proof_over_recursion.bin").unwrap(),
            bincode::config::standard(),
        )
        .unwrap();

        for (family, proofs) in proof.circuit_families_proofs.iter() {
            println!("{} proofs for family {}", proofs.len(), family);
        }
        for (delegation_type, proofs) in proof.delegation_proofs.iter() {
            println!("{} proofs for delegation {}", proofs.len(), delegation_type);
        }
    }

    // #[test]
    // fn verify_unified_proof() {
    //     use std::fs::File;
    //     use execution_utils::setups::CompiledCircuitsSet;

    //     let setup: UnrolledProgramSetup = serde_json::from_reader(&File::open("unified_setup_over_recursion.json").unwrap()).unwrap();
    //     let proof: UnrolledProgramProof = serde_json::from_reader(&File::open("unified_proof_over_recursion.json").unwrap()).unwrap();
    //     let layout: CompiledCircuitsSet =
    //         serde_json::from_reader(&File::open("unified_layout_over_recursion.json").unwrap()).unwrap();

    //     assert_eq!(setup.circuit_families_setups.len(), 1);
    //     println!("Verifying...");
    //     let result = execution_utils::unified_circuit::verify_unrolled_recursion_layer_via_full_statement_verifier(&proof, &setup, &layout).expect("is valid proof");
    //     assert!(result.iter().all(|el| *el == 0) == false);
    //     dbg!(result);
    // }
}
