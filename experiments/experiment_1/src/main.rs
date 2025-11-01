mod non_determenism;

use non_determenism::VecDequeNonDetermenismSource;
use zksync_os_experiments_utils::run_binary::run_binary;

pub fn main() {
    let bin_path = "risc_v_binary/app.bin";

    let mut non_determinism_source = VecDequeNonDetermenismSource::default();
    non_determinism_source.oracle.push_back(11);

    let result = run_binary(bin_path, non_determinism_source);

    println!("Input: {:?}", result[1]);
    println!("Output: {:?}", result[0]);
}
