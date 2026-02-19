use airbender_crypto::{blake2s::Blake2s256, MiniDigest};
use airbender_host::{Inputs, Program, Result, Runner};
use std::path::PathBuf;

fn main() -> Result<()> {
    let dist_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../guest/dist/app");
    let program = Program::load(&dist_dir)?;

    let balance = |x| {
        let mut balance = [0u8; 32];
        balance[31] = x;
        balance
    };

    let leafs: [_; 4] = std::array::from_fn(|i| {
        let x = i as u8 + 1;
        Blake2s256::digest([[x; 32], balance(x)].concat())
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

    inputs.push(&[1u8; 32])?; // asset_id
    inputs.push(&0u32)?; // index
    inputs.push(&balance(1))?; // balance
    inputs.push(&leafs[1])?; // path[0]
    inputs.push(&middle[1])?; // path[1]

    inputs.push(&[3u8; 32])?;
    inputs.push(&2u32)?;
    inputs.push(&balance(3))?;
    inputs.push(&leafs[3])?;
    inputs.push(&middle[0])?;

    inputs.push(&0u32)?; // number of new tokens in logs
    inputs.push(&0u32)?; // number of logs
                         // TODO test with 1 log

    let simulator = program.simulator_runner().build()?;
    let execution = simulator.run(inputs.words())?;
    let exec_output = execution.receipt.output;
    println!(
        "Execution finished: cycles={}, reached_end={}, output={:?}",
        execution.cycles_executed, execution.reached_end, exec_output
    );

    let commitment = Blake2s256::digest([root, root, [0; 32]].concat());
    let expected_output = std::array::from_fn(|i| {
        u32::from_be_bytes(commitment[i * 4..(i + 1) * 4].try_into().unwrap())
    });

    assert_eq!(exec_output, expected_output);

    Ok(())
}
