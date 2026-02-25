use super::*;
use airbender_crypto::{blake2s::Blake2s256, MiniDigest};
use airbender_host::{Inputs, Program, Result, Runner};
use alloy_primitives::{address, fixed_bytes, hex, U256};
use std::path::PathBuf;

fn log1() -> (L2Log, Vec<u8>) {
    // taken from https://github.com/matter-labs/era-contracts/blob/7a00eaebcd3d1ce362efbd28b05b4fa9032167fa/l1-contracts/test/foundry/l1/integration/l2-tests-abstract/L2AssetTrackerData.sol#L469
    let log = L2Log {
        tx_number_in_batch: 0,
        sender: address!("0x0000000000000000000000000000000000008008").0 .0,
        key: fixed_bytes!("0x0000000000000000000000000000000000000000000000000000000000010003").0,
        value: fixed_bytes!("0x14803e5a9c544a45f7954aec0a520d3c71eded48e11622734838297866adcdf6").0,
    };
    let message = hex!("0x9c884fd10000000000000000000000000000000000000000000000000000000000000104b1f317b7effffcd4e3cf53784ae442ecc4e835c532aaf0e60a046fa8efb96e8500000000000000000000000080cff9f04a22f0fae0d93abbb3abb1295a17460800000000000000000000000080cff9f04a22f0fae0d93abbb3abb1295a1746080000000000000000000000008eed0c30ec2dfa992ad6790bdf2461fd72c9fe4f000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000001c1010000000000000000000000000000000000000000000000000000000000000009000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000180000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000004574254430000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000457425443000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000").to_vec();
    (log, message)
}

fn log2() -> (L2Log, Vec<u8>) {
    // taken from https://github.com/matter-labs/era-contracts/blob/7a00eaebcd3d1ce362efbd28b05b4fa9032167fa/l1-contracts/test/foundry/l1/integration/l2-tests-abstract/L2AssetTrackerData.sol#L619
    let log = L2Log {
        tx_number_in_batch: 0,
        sender: address!("0x0000000000000000000000000000000000008008").0 .0,
        key: fixed_bytes!("0x0000000000000000000000000000000000000000000000000000000000010003").0,
        value: fixed_bytes!("0xeecf03bb601fcd4091c9ec2f869b85383aedbcb91d1ee58285cdf2cf91cee5a3").0,
    };
    let message = hex!("0x9c884fd10000000000000000000000000000000000000000000000000000000000000104b5eab7cc8c9114c3115a034b49b3d87b0b352aa88c2a9d5ff7339cde105aa44c000000000000000000000000d3478ceef3e634756968fdcba16702ea2c042ada000000000000000000000000d3478ceef3e634756968fdcba16702ea2c042ada000000000000000000000000e4992410678240d6a0687983f58638e2a913275f000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000001c10100000000000000000000000000000000000000000000000000000000000001040000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000001800000000000000000000000000000000000000000000000000000000000000060000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000065a4b73796e6300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000060000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000025a4b0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000").to_vec();
    (log, message)
}

const ASSET_ID_1: H256 =
    fixed_bytes!("0xb1f317b7effffcd4e3cf53784ae442ecc4e835c532aaf0e60a046fa8efb96e85").0; // from log 1
const ASSET_ID_2: H256 =
    fixed_bytes!("0xb5eab7cc8c9114c3115a034b49b3d87b0b352aa88c2a9d5ff7339cde105aa44c").0; // from log 2

struct MerkleTree {
    asset_ids: Vec<H256>,
    balances: Vec<U256>,
    layers: Vec<Vec<H256>>,
}

impl MerkleTree {
    fn new(asset_ids: Vec<H256>, balances: Vec<U256>) -> Self {
        assert_eq!(asset_ids.len(), balances.len());
        assert_eq!(asset_ids.len(), asset_ids.len().next_power_of_two());
        let mut tree = Self {
            asset_ids,
            balances,
            layers: vec![],
        };
        tree.build();
        tree
    }

    fn build(&mut self) {
        let size = self.asset_ids.len().next_power_of_two();
        let height = size.trailing_zeros() as usize;

        // build first layer
        self.layers = vec![vec![]];
        for (asset_id, balance) in self.asset_ids.iter().zip(&self.balances) {
            self.layers[0].push(Blake2s256::digest(
                [*asset_id, balance.to_be_bytes()].concat(),
            ));
        }

        for i in 0..height {
            let mut new_layer = vec![];
            for chunk in self.layers[i].as_chunks::<2>().0 {
                new_layer.push(Blake2s256::digest(chunk.concat()));
            }
            self.layers.push(new_layer);
        }
    }

    fn input(&self, input: &mut Inputs, mut index: usize) -> Result<()> {
        input.push(&self.asset_ids[index])?;
        input.push(&(index as u32))?; // index
        input.push::<H256>(&self.balances[index].to_be_bytes())?; // balance
        let mut path = vec![];
        let height = self.asset_ids.len().trailing_zeros() as usize;
        for i in 0..height {
            path.push(self.layers[i][index ^ 1]);
            index >>= 1;
        }
        input.push(&path)?;
        Ok(())
    }

    fn root(&self) -> H256 {
        self.layers[self.layers.len() - 1][0]
    }
}

fn push_log(input: &mut Inputs, log: &L2Log, message: &Vec<u8>) -> Result<()> {
    input.push(&log.tx_number_in_batch)?;
    input.push(&log.sender)?;
    input.push(&log.key)?;
    input.push(&log.value)?;
    input.push(&message)?;
    Ok(())
}

#[test]
fn test_full_tree() -> Result<()> {
    let dist_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../guest/dist/app");
    let program = Program::load(&dist_dir)?;

    let asset_ids = [[1u8; 32], [2u8; 32], ASSET_ID_1, [4u8; 32]];

    let mut balances = [U256::from(1), U256::from(2), U256::from(3), U256::from(4)];

    let tree = MerkleTree::new(asset_ids.to_vec(), balances.to_vec());
    let mut inputs = Inputs::new();
    let old_root = tree.root();

    inputs.push(&old_root)?;
    inputs.push(&4u32)?; // tree size
    inputs.push(&[0xEEu8; 32])?; // base token asset id
    inputs.push(&2u32)?; // number of existing tokens in logs

    tree.input(&mut inputs, 0)?;
    tree.input(&mut inputs, 2)?;

    inputs.push(&1u32)?; // number of logs

    let (log, message) = log1();
    push_log(&mut inputs, &log, &message)?;

    let simulator = program.simulator_runner().build()?;
    let execution = simulator.run(inputs.words())?;
    let exec_output = execution.receipt.output;
    println!(
        "Execution finished: cycles={}, reached_end={}, output={:?}",
        execution.cycles_executed, execution.reached_end, exec_output
    );

    // decrease balance by 1
    balances[2] = U256::from(2);
    let tree = MerkleTree::new(asset_ids.to_vec(), balances.to_vec());
    let logs_root = log.hash();
    let commitment = Blake2s256::digest([tree.root(), old_root, logs_root].concat());
    let expected_output = h256_to_u32_array(commitment);
    assert_eq!(exec_output, expected_output);
    Ok(())
}
