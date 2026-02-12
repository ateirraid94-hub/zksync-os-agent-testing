use crypto::{blake2s::Blake2s256, MiniDigest};
use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
use std::path::PathBuf;
use std::str::FromStr;
use zksync_os_runner::run;

fn u8_array_to_u32_array(input: [u8; 32]) -> [u32; 8] {
    std::array::from_fn(|i| {
        u32::from_be_bytes([
            input[i * 4],
            input[i * 4 + 1],
            input[i * 4 + 2],
            input[i * 4 + 3],
        ])
    })
}

fn blake_hash_parts(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut input = [0; 64];
    input[..32].copy_from_slice(&left);
    input[32..].copy_from_slice(&right);
    let digest = Blake2s256::digest(input);
    digest
}

fn balance(amount: u8) -> [u8; 32] {
    let mut array = [0; 32];
    array[31] = amount;
    array
}

fn main() {
    let mut non_determinism_source = QuasiUARTSource::default();

    let leafs: [_; 4] =
        std::array::from_fn(|i| blake_hash_parts([i as u8 + 1; 32], balance(i as u8 + 1)));
    let middle = [
        blake_hash_parts(leafs[0], leafs[1]),
        blake_hash_parts(leafs[2], leafs[3]),
    ];
    let root = blake_hash_parts(middle[0], middle[1]);

    non_determinism_source.oracle.extend(
        u8_array_to_u32_array(root), // prev root
    );
    non_determinism_source.oracle.extend(&[
        4, // prev_tree_size
        2, // number of old tokens in diffs
    ]);
    // [asset_id, index, prev_balance, [path]]
    non_determinism_source.oracle.extend(&[0x01010101; 8]);
    non_determinism_source.oracle.push_back(0);
    non_determinism_source.oracle.extend(&u8_array_to_u32_array(balance(1)));
    non_determinism_source.oracle.extend(&u8_array_to_u32_array(leafs[1]));
    non_determinism_source.oracle.extend(&u8_array_to_u32_array(middle[1]));

    non_determinism_source.oracle.extend(&[0x03030303; 8]);
    non_determinism_source.oracle.push_back(2);
    non_determinism_source.oracle.extend(&u8_array_to_u32_array(balance(3)));
    non_determinism_source.oracle.extend(&u8_array_to_u32_array(leafs[3]));
    non_determinism_source.oracle.extend(&u8_array_to_u32_array(middle[0]));

    // number of new tokens in diffs
    // [asset_id, [path]]
    // number of logs
    // [???]
    non_determinism_source.oracle.extend(&[ 0, 0]);

    let output = run(
        PathBuf::from_str("binary/app.bin").unwrap(),
        None,
        1 << 25,
        non_determinism_source,
    );
    dbg!(output);
    assert_eq!(output, u8_array_to_u32_array(blake_hash_parts(root, root)));
}
