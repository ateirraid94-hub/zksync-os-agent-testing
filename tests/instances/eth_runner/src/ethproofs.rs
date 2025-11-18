use crate::live_run::rpc::{self, EthProofPayload};
use alloy::consensus::Header;
use alloy_primitives::U256;
use alloy_rlp::Encodable;
use anyhow::Context;
use anyhow::Ok;

use rig::log::info;
use rig::*;

use std::thread::sleep;
use std::time::Duration;

const ETH_CHAIN_ID: u64 = 1;

#[allow(clippy::too_many_arguments)]
fn eth_run(
    mut chain: Chain<false>,
    header: Header,
    block_number: u64,
    transactions: Vec<Vec<u8>>,
    block_hashes: Vec<U256>,
    witness: alloy_rpc_types_debug::ExecutionWitness,
    withdrawals_encoding: Vec<u8>,
    write_to_file: bool,
    app: Option<String>,
) -> anyhow::Result<Vec<u32>> {
    chain.set_last_block_number(block_number - 1);

    chain.set_block_hashes(block_hashes.try_into().unwrap());

    let witness_output_dir = if write_to_file {
        let mut suffix = block_number.to_string();
        suffix.push_str("_witness");
        Some(std::path::PathBuf::from(&suffix))
    } else {
        None
    };
    let (_result_keeper, witness) = chain.run_eth_block_with_options::<true>(
        transactions,
        witness,
        header,
        withdrawals_encoding,
        witness_output_dir,
        app,
        true,
        true,
    );

    Ok(witness.unwrap())
}

/// Runs ethproofs to generate execution witness for a given block number.
/// Returns the witness and the duration it took to generate it (without time spent on fetching data).
pub fn ethproofs_run(
    block_number: u64,
    reth_endpoint: &str,
    write_to_file: bool,
    app: Option<String>,
) -> anyhow::Result<(Vec<u32>, f64)> {
    // Fetch data from RPC endpoints
    let block = rpc::get_block(reth_endpoint, block_number)
        .context(format!("Failed to fetch block for {block_number}"))?;
    let witness = rpc::get_witness(reth_endpoint, block_number)
        .context(format!("Failed to fetch witness for {block_number}"))?
        .result;

    // get current time
    let current_time = std::time::SystemTime::now();

    let mut headers: Vec<Header> = witness
        .headers
        .iter()
        .map(|el| alloy_rlp::decode_exact(&el[..]).expect("must decode headers from witness"))
        .collect();
    assert!(headers.len() > 0);
    assert!(headers.is_sorted_by(|a, b| { a.number < b.number }));
    headers.reverse();

    assert_eq!(headers[0].number, block_number - 1);
    let mut block_hashes: Vec<U256> = headers
        .iter()
        .map(|el| U256::from_be_bytes(el.hash_slow().0))
        .collect();
    block_hashes.resize(256, U256::ZERO); // those will not be accessed

    info!("Running block: {block_number}");
    info!("Block gas used: {}", block.result.header.gas_used);

    let header = block.result.header.clone().into();

    let withdrawals_encoding = if let Some(withdrawals) = block.result.withdrawals.clone() {
        let mut buff = vec![];
        withdrawals.encode(&mut buff);

        buff
    } else {
        Vec::new()
    };
    let transactions = block.get_all_raw_transactions();

    let chain = Chain::empty(Some(ETH_CHAIN_ID));
    let witness = eth_run(
        chain,
        header,
        block_number,
        transactions,
        block_hashes,
        witness,
        withdrawals_encoding,
        write_to_file,
        app,
    )?;
    // compute time taken
    let duration = current_time.elapsed().unwrap();
    println!("Time taken: {:?}", duration);
    Ok((witness, duration.as_secs_f64()))
}

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const CONFIRMATIONS: u64 = 2;

pub fn ethproofs_live_run(reth_endpoint: &str) -> anyhow::Result<()> {
    let mut next = rpc::get_block_number(reth_endpoint)?.saturating_sub(CONFIRMATIONS);

    ethproofs_run(next, reth_endpoint, true, None)?;

    loop {
        let head = rpc::get_block_number(reth_endpoint)?.saturating_sub(CONFIRMATIONS);
        if head > next {
            for n in (next + 1)..=head {
                ethproofs_run(n, reth_endpoint, true, None)?;
            }
            next = head;
        } else {
            sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(not(feature = "with_gpu_prover"))]
pub fn ethproofs_with_proofs(
    _reth_endpoint: &str,
    _connector: EthProofsConnector,
    _block_selector: (u64, u64),
) -> anyhow::Result<()> {
    panic!("Ethproofs with proofs requires the 'with_gpu_prover' feature to be enabled");
}

#[cfg(feature = "with_gpu_prover")]
pub fn ethproofs_with_proofs(
    reth_endpoint: &str,
    connector: EthProofsConnector,
    block_selector: (u64, u64),
) -> anyhow::Result<()> {
    use base64::Engine;
    use bincode::config::standard;

    use cli_lib::prover_utils::UnrolledProver;
    use rig::chain::get_zksync_os_img_path;
    // For now, we just use the 'default' app.bin from zksync-os dir.
    let bin_path = get_zksync_os_img_path(&None);
    let path = &bin_path.into_os_string().into_string().unwrap();
    let path = path.strip_suffix(".bin").unwrap().to_string();

    let pp = UnrolledProver::new(&path, 16);

    let mut next = 0;

    loop {
        let head = rpc::get_block_number(reth_endpoint)?;
        let head = connector.select_block(head, block_selector);
        if head > next {
            println!("Generating proof for block {}", head);
            let (witness, duration) = ethproofs_run(
                head,
                reth_endpoint,
                false,
                None, //Some(bin_path_without_bin.clone()),
            )?;
            let mut total_proof_time = Some(duration);

            let start_time = std::time::SystemTime::now();
            let (proof, cycles) = pp.prove(witness);
            total_proof_time =
                total_proof_time.map(|t| t + start_time.elapsed().unwrap().as_secs_f64()); // Placeholder for actual proof data.

            // Bincode serialize and then base64 encode the proof.
            let serialized_proof = bincode::serde::encode_to_vec(&proof, standard())
                .context("Failed to serialize the program proof")?;
            let encoded_proof = base64::engine::general_purpose::STANDARD.encode(&serialized_proof);

            connector.send_proof(head, &encoded_proof, total_proof_time.unwrap(), cycles)?;

            next = head;
        } else {
            sleep(POLL_INTERVAL);
        }
    }
}

pub struct EthProofsConnector {
    pub staging: bool,
    pub auth_token: String,
    pub cluster_id: u64,
    pub url: String,
}

impl EthProofsConnector {
    pub fn new(staging: bool, auth_token: String, cluster_id: u64) -> Self {
        let url = if staging {
            "https://staging--ethproofs.netlify.app/api/v0/".to_string()
        } else {
            "https://ethproofs.netlify.app/api/v0/".to_string()
        };
        Self {
            staging,
            auth_token,
            cluster_id,
            url,
        }
    }

    pub fn select_block(&self, candidate_block: u64, (prover_id, block_mod): (u64, u64)) -> u64 {
        // This is the block that we should pick.
        let selected_block = candidate_block - (candidate_block % block_mod) + prover_id;

        // But if it turns out to be larger than candidate_block, we need to wait for the next round.
        // And we'll return the previous round's block number to indicate that.
        if selected_block > candidate_block {
            // Return block from the previous round.
            return selected_block - block_mod;
        }
        return selected_block;
    }
    pub fn send_proof(
        &self,
        block_number: u64,
        serialized_proof: &str,
        time_spent: f64,
        cycles: u64,
    ) -> anyhow::Result<()> {
        println!(
            "Sending proof for block {} to ethproofs server, time spent: {}s , proof size: {} bytes",
            block_number, time_spent, serialized_proof.len()
        );
        let payload = EthProofPayload {
            block_number,
            cluster_id: self.cluster_id,
            proving_time: (time_spent * 1000.0) as u64,
            proving_cycles: cycles,
            proof: serialized_proof.to_string(),
            verifier_id: "None".to_string(),
        };
        let response = rpc::send_ethproofs(
            &format!("{}proofs/proved", self.url),
            self.auth_token.clone(),
            payload,
        )?;
        println!("Response from server: {}", response);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_block_selection() {
        let connector = EthProofsConnector::new(true, "token".to_string(), 1);
        assert_eq!(connector.select_block(100, (0, 10)), 100);
        assert_eq!(connector.select_block(100, (5, 10)), 95);
        assert_eq!(connector.select_block(100, (9, 10)), 99);
        assert_eq!(connector.select_block(105, (0, 10)), 100);
        assert_eq!(connector.select_block(105, (5, 10)), 105);
        assert_eq!(connector.select_block(105, (9, 10)), 99);
    }
}
