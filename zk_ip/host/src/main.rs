use airbender_crypto::{blake2s::Blake2s256, MiniDigest};
use airbender_host::{Inputs, Program, Result, Runner};
use std::path::PathBuf;
use alloy_primitives::{address, fixed_bytes, hex};

fn main() -> Result<()> {
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
    inputs.push(&leafs[1])?; // path[0]
    inputs.push(&middle[1])?; // path[1]

    inputs.push(&asset_ids[2])?;
    inputs.push(&2u32)?;
    inputs.push(&balance(3))?;
    inputs.push(&leafs[3])?;
    inputs.push(&middle[0])?;

    inputs.push(&0u32)?; // number of new tokens in logs
    inputs.push(&1u32)?; // number of logs

    inputs.push(&0u16)?; // tx number in batch
    inputs.push(&address!("0x0000000000000000000000000000000000008008").0.0)?; // sender
    inputs.push(&fixed_bytes!("0x0000000000000000000000000000000000000000000000000000000000010003").0)?; // key
    inputs.push(&fixed_bytes!("0x14803e5a9c544a45f7954aec0a520d3c71eded48e11622734838297866adcdf6").0)?; // value
    // message
    inputs.push(&hex!("0x9c884fd10000000000000000000000000000000000000000000000000000000000000104b1f317b7effffcd4e3cf53784ae442ecc4e835c532aaf0e60a046fa8efb96e8500000000000000000000000080cff9f04a22f0fae0d93abbb3abb1295a17460800000000000000000000000080cff9f04a22f0fae0d93abbb3abb1295a1746080000000000000000000000008eed0c30ec2dfa992ad6790bdf2461fd72c9fe4f000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000001c1010000000000000000000000000000000000000000000000000000000000000009000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000180000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000004574254430000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000457425443000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000").to_vec())?;

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

    let commitment = Blake2s256::digest([new_root, root, /*logs_root*/].concat());
    let expected_output = std::array::from_fn(|i| {
        u32::from_be_bytes(commitment[i * 4..(i + 1) * 4].try_into().unwrap())
    });

    assert_eq!(exec_output, expected_output);

    Ok(())
}
