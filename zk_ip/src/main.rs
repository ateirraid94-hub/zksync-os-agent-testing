
use zksync_os_runner::run;
use risc_v_simulator::abstractions::non_determinism::QuasiUARTSource;
use std::str::FromStr;
use std::path::PathBuf;

/// Quick test that uses the .bin file that computes the n-th fibonacci number.
fn main() {
    let mut non_determinism_source = QuasiUARTSource::default();
    // non_determinism_source.oracle.push_back(11);
    // non_determinism_source.oracle.push_back(11);


    non_determinism_source.oracle.extend(&[
        1, 2, 3, 4, 5, 6, 7, 8, // prev_root
        1, // tree_height
        2, // n
        // first diff
        0, // sign - minus
        0, 0, 0, 0, 0, 0, 0, 5, // amount
        1, 1, 1, 1, 1, 1, 1, 1, // asset_id
        0, 0, 0, 0, 0, 0, 0, 10, // prev_balance
        0, // index
        // path for first token (1 node)
        2, 2, 2, 2, 2, 2, 2, 2,
        // second diff
        1, // sign - plus
        0, 0, 0, 0, 0, 0, 0, 3, // amount
        3, 3, 3, 3, 3, 3, 3, 3, // asset_id
        0, 0, 0, 0, 0, 0, 0, 5, // prev_balance
        1, // index
        // path for second token (1 node)
        4,4 ,4 ,4 ,4 ,4 ,4 ,4,
    ]);


    let output = run(
        PathBuf::from_str("binary/app.bin").unwrap(),
        None,
        1 << 25,
        non_determinism_source,
    );
    dbg!(output);
}
