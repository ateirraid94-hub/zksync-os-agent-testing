use alloy_primitives::{Address, address, hex};
use airbender_crypto::{sha3::Keccak256, MiniDigest};

pub type H256 = [u8; 32];

pub const L2_BOOTLOADER: Address = address!("0x0000000000000000000000000000000000008001");
pub const L2_KNOWN_CODE_STORAGE: Address = address!("0x0000000000000000000000000000000000008004");
pub const L2_TO_L1_MESSENGER: Address = address!("0x0000000000000000000000000000000000008008");
pub const L2_BASE_TOKEN: Address = address!("0x000000000000000000000000000000000000800a");
pub const L2_COMPRESSOR: Address = address!("0x000000000000000000000000000000000000800e");

pub const L2_ASSET_ROUTER: Address = address!("0x0000000000000000000000000000000000010003");
pub const L2_NATIVE_TOKEN_VAULT: Address = address!("0x0000000000000000000000000000000000010004");
pub const L2_INTEROP_CENTER: Address = address!("0x000000000000000000000000000000000001000d");
pub const L2_INTEROP_HANDLER: Address = address!("0x000000000000000000000000000000000001000e");
pub const L2_ASSET_TRACKER: Address = address!("0x000000000000000000000000000000000001000f");

pub const L2_LOG_LENGTH: usize = 88;

// keccak256([0; L2_LOG_LENGTH])
pub const EMPTY_LOG_HASH: H256 = hex!("0x72abee45b59e344af8a6e520241c4744aff26ed411f4c4b00f8af09adada43ba");

pub struct L2Log {
    pub tx_number_in_batch: u16,
    pub sender: [u8; 20], // Address
    pub key: H256,
    pub value: H256,
}

impl L2Log {
    pub fn hash(&self) -> H256 {
        let mut buffer = [0u8; L2_LOG_LENGTH];
        buffer[0] = 0; // shard_id = rollup
        buffer[1] = 1; // is_service = true
        buffer[2..4].copy_from_slice(&self.tx_number_in_batch.to_be_bytes());
        buffer[4..24].copy_from_slice(&self.sender);
        buffer[24..56].copy_from_slice(&self.key);
        buffer[56..88].copy_from_slice(&self.value);
        Keccak256::digest(&buffer)
    }
}

pub fn h256_to_u32_array(hash: H256) -> [u32; 8] {
    std::array::from_fn(|i| u32::from_be_bytes(hash[i * 4..(i + 1) * 4].try_into().unwrap()))
}

#[cfg(test)]
mod test {
    use airbender_crypto::{blake2s::Blake2s256, MiniDigest};
    use airbender_host::{Inputs, Program, Result, Runner};
    use std::path::PathBuf;
    use alloy_primitives::{address, fixed_bytes, hex};
    use super::L2Log;

    #[test]
    fn test_full_tree() -> Result<()> {
        let dist_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../guest/dist/app");
        let program = Program::load(&dist_dir)?;

        let balance = |x| {
            let mut balance = [0u8; 32];
            balance[31] = x;
            balance
        };

        let asset_ids = [
            [1u8; 32],
            [2u8; 32],
            fixed_bytes!("0xb1f317b7effffcd4e3cf53784ae442ecc4e835c532aaf0e60a046fa8efb96e85").0,
            [4u8; 32],
        ];
        let mut leafs: [_; 4] = std::array::from_fn(|i| {
            let x = i as u8 + 1;
            Blake2s256::digest([asset_ids[i], balance(x)].concat())
        });
        let middle = [
            Blake2s256::digest([leafs[0], leafs[1]].concat()),
            Blake2s256::digest([leafs[2], leafs[3]].concat()),
        ];
        let root = Blake2s256::digest([middle[0], middle[1]].concat());

        let mut inputs = Inputs::new();

        inputs.push(&root)?;
        inputs.push(&4u32)?; // tree size
        inputs.push(&[0xEEu8; 32])?; // base token asset id
        inputs.push(&2u32)?; // number of existing tokens in logs

        inputs.push(&asset_ids[0])?; // asset_id
        inputs.push(&0u32)?; // index
        inputs.push(&balance(1))?; // balance
        inputs.push(&vec![leafs[1], middle[1]])?; // path

        inputs.push(&asset_ids[2])?;
        inputs.push(&2u32)?;
        inputs.push(&balance(3))?;
        inputs.push(&vec![leafs[3], middle[0]])?;

        inputs.push(&1u32)?; // number of logs

        // taken from https://github.com/matter-labs/era-contracts/blob/7a00eaebcd3d1ce362efbd28b05b4fa9032167fa/l1-contracts/test/foundry/l1/integration/l2-tests-abstract/L2AssetTrackerData.sol#L469
        let log = L2Log {
            tx_number_in_batch: 0,
            sender: address!("0x0000000000000000000000000000000000008008").0.0,
            key: fixed_bytes!("0x0000000000000000000000000000000000000000000000000000000000010003").0,
            value: fixed_bytes!("0x14803e5a9c544a45f7954aec0a520d3c71eded48e11622734838297866adcdf6").0,
        };
        let message = hex!("0x9c884fd10000000000000000000000000000000000000000000000000000000000000104b1f317b7effffcd4e3cf53784ae442ecc4e835c532aaf0e60a046fa8efb96e8500000000000000000000000080cff9f04a22f0fae0d93abbb3abb1295a17460800000000000000000000000080cff9f04a22f0fae0d93abbb3abb1295a1746080000000000000000000000008eed0c30ec2dfa992ad6790bdf2461fd72c9fe4f000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000001c1010000000000000000000000000000000000000000000000000000000000000009000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000180000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000004574254430000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000457425443000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000").to_vec();

        // taken from https://github.com/matter-labs/era-contracts/blob/7a00eaebcd3d1ce362efbd28b05b4fa9032167fa/l1-contracts/test/foundry/l1/integration/l2-tests-abstract/L2AssetTrackerData.sol#L619
            // logs[i][j++] = L2Log({
            //
            //     l2ShardId: 0,
            //     isService: true,
            //     txNumberInBatch: 0,
            //     sender: 0x0000000000000000000000000000000000008008,
            //     key: 0x0000000000000000000000000000000000000000000000000000000000010003,
            //     value: 0xeecf03bb601fcd4091c9ec2f869b85383aedbcb91d1ee58285cdf2cf91cee5a3
            // });
            // j = 0;
            // messages[i] = new bytes[](1);
            // messages[i][
            //     0
            // ] = hex"9c884fd10000000000000000000000000000000000000000000000000000000000000104b5eab7cc8c9114c3115a034b49b3d87b0b352aa88c2a9d5ff7339cde105aa44c000000000000000000000000d3478ceef3e634756968fdcba16702ea2c042ada000000000000000000000000d3478ceef3e634756968fdcba16702ea2c042ada000000000000000000000000e4992410678240d6a0687983f58638e2a913275f000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000001c10100000000000000000000000000000000000000000000000000000000000001040000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000001800000000000000000000000000000000000000000000000000000000000000060000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000065a4b73796e6300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000060000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000025a4b0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000";

        inputs.push(&log.tx_number_in_batch)?;
        inputs.push(&log.sender)?;
        inputs.push(&log.key)?;
        inputs.push(&log.value)?;
        inputs.push(&message)?;

        let simulator = program.simulator_runner().build()?;
        let execution = simulator.run(inputs.words())?;
        let exec_output = execution.receipt.output;
        println!(
            "Execution finished: cycles={}, reached_end={}, output={:?}",
            execution.cycles_executed, execution.reached_end, exec_output
        );

        // decrease balance by 1
        leafs[2] = Blake2s256::digest([asset_ids[2], balance(2)].concat());
        let middle = [
            Blake2s256::digest([leafs[0], leafs[1]].concat()),
            Blake2s256::digest([leafs[2], leafs[3]].concat()),
        ];
        let new_root = Blake2s256::digest([middle[0], middle[1]].concat());
        let logs_root = log.hash();
        let commitment = Blake2s256::digest([new_root, root, logs_root].concat());
        let expected_output = h256_to_u32_array(commitment);
        assert_eq!(exec_output, expected_output);
        Ok(())
    }
}
