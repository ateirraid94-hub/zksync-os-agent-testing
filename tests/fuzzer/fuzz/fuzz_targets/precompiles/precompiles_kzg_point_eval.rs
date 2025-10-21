#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use revm::precompile::kzg_point_evaluation;

mod common;

#[derive(Debug)]
struct Input {
    versioned_hash: [u8; 32],
    x: [u8; 32],
    y: [u8; 32],
    commitment: [u8; 48],
    proof: [u8; 48],
}

impl Input {
    /// Concatenates all fields into a single `Vec<u8>`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::new();

        // Append the 32-byte fields
        result.extend_from_slice(&self.versioned_hash);
        result.extend_from_slice(&self.x);
        result.extend_from_slice(&self.y);

        // Append the 48-byte fields
        result.extend_from_slice(&self.commitment);
        result.extend_from_slice(&self.proof);

        result
    }
}

impl<'a> Arbitrary<'a> for Input {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {

        let mut x = [0u8; 32];
        u.fill_buffer(&mut x)?;

        let mut y = [0u8; 32];
        u.fill_buffer(&mut y)?;

        let mut commitment = [0u8; 48];
        u.fill_buffer(&mut commitment)?;

        let versioned_hash = kzg_point_evaluation::kzg_to_versioned_hash(&commitment);

        let mut proof = [0u8; 48];
        u.fill_buffer(&mut proof)?;

        Ok(Self {
            versioned_hash,
            x,
            y,
            commitment,
            proof
        })
    }
}

fuzz_target!(|input: Input| {
    let block_output = common::run_precompile(
        "000000000000000000000000000000000000000a",
        input.to_bytes().as_ref(),
    );

    #[allow(unused_variables)]
    let output = block_output
        .tx_results
        .first()
        .unwrap()
        .clone()
        .expect("Tx should have succeeded");

    let zksync_os_bytes = output.as_returned_bytes();
    let bytes: alloy::primitives::Bytes = input.to_bytes().into();
    let revm_res = kzg_point_evaluation::run(&bytes, 1 << 27);

    match revm_res {
        Ok(revm) => assert_eq!(zksync_os_bytes, revm.bytes.to_vec()),
        Err(_) => assert!(common::is_zero(zksync_os_bytes)),
    }
});
