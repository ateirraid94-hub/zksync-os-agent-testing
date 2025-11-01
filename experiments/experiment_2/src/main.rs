mod non_determenism;

use non_determenism::DummyStorageNonDetermenismSource;
use zksync_os_experiments_utils::run_binary::run_binary;

pub fn main() {
    let bin_path = "risc_v_binary/app.bin";

    let mut non_determinism_source = DummyStorageNonDetermenismSource::default();
    non_determinism_source.set(0, 1);
    non_determinism_source.set(1, 2);

    let expected_commitment: u32 = 1 + 2 + 3;
    let result = run_binary(bin_path, non_determinism_source);

    println!("Commitment: {:?}", result[0]);
    println!("Expected commitment: {:?}", expected_commitment);
}
