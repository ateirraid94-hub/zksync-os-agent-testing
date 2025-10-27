#![no_main]
#![feature(allocator_api)]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use fuzz_precompiles_forward::precompiles::ecadd as ecadd_forward;
use fuzz_precompiles_proving::precompiles::ecadd as ecadd_proving;
use revm_precompile::bn254;

const P_BE: [u8; 32] = [
    0x30, 0x64, 0x4E, 0x72, 0xE1, 0x31, 0xA0, 0x29,
    0xB8, 0x50, 0x45, 0xB6, 0x81, 0x81, 0x58, 0x5D,
    0x28, 0x33, 0xE8, 0x48, 0x79, 0xB9, 0x70, 0x91,
    0x43, 0xE1, 0xF5, 0x93, 0xF0, 0x00, 0x00, 0x01,
];

#[derive(Arbitrary, Debug, Clone)]
struct Input {
    inf1: bool,
    inf2: bool,
    seed1: [u8; 32],
    seed2: [u8; 32],
    seed3: [u8; 32],
    seed4: [u8; 32],
}

#[inline]
fn be_lt(a: &[u8; 32], b: &[u8; 32]) -> bool {
    for i in 0..32 {
        if a[i] < b[i] { return true; }
        if a[i] > b[i] { return false; }
    }
    false
}

fn reduce_mod_p(mut x: [u8; 32]) -> [u8; 32] {
    let mut p = P_BE;
    for _ in 0..2 {
        if !be_lt(&x, &p) {
            let mut borrow = 0u16;
            for i in (0..32).rev() {
                let xi = x[i] as u16;
                let pi = p[i] as u16;
                let t = 256 + xi - pi - (borrow & 1);
                x[i] = (t & 0xFF) as u8;
                borrow = (t >> 8) ^ 1;
            }
        }
    }
    x
}

fn make_point(
    x_seed: [u8; 32],
    y_seed: [u8; 32],
    force_infinity: bool,
) -> ([u8; 32], [u8; 32]) {
    if force_infinity {
        return ([0u8; 32], [0u8; 32]);
    }
    let mut x = reduce_mod_p(x_seed);
    let mut y = reduce_mod_p(y_seed);
    (x, y)
}

fn build_input_bytes(i: &Input) -> [u8; 128] {
    let (x1, y1) = make_point(i.seed1, i.seed2, i.inf1);
    let (x2, y2) = make_point(i.seed3, i.seed4, i.inf2);
    let mut buf = [0u8; 128];
    buf[0..32].copy_from_slice(&x1);
    buf[32..64].copy_from_slice(&y1);
    buf[64..96].copy_from_slice(&x2);
    buf[96..128].copy_from_slice(&y2);
    buf
}

fn fuzz(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let input = match Input::arbitrary(&mut u) {
        Ok(x) => x,
        Err(_) => return,
    };

    let in_bytes = build_input_bytes(&input);

    let mut dst1 = Vec::new();
    let mut dst2 = Vec::new();

    let r_reth = bn254::run_add(&in_bytes, u64::MAX, u64::MAX);
    let reth_ok = r_reth.as_ref().is_ok_and(|x| !x.reverted);

    let r1 = ecadd_forward(&in_bytes, &mut dst1);
    let r1_ok = r1.is_ok();

    // Skip if both RETH and Forward run failed
    if !reth_ok && !r1_ok {
        return;
    }

    let r2 = ecadd_proving(&in_bytes, &mut dst2);
    let r2_ok = r2.is_ok();

    if reth_ok || r1_ok || r2_ok {
        assert!(reth_ok,  "reth reverted but others accepted");
        assert!(r1_ok,   "forward run rejected but reth accepted");
        assert!(r2_ok,   "proving run rejected but reth accepted");

        let reth_out = r_reth.unwrap().bytes.to_vec();
        assert_eq!(dst1, reth_out, "forward <> reth bytes mismatch");
        assert_eq!(dst2, reth_out, "proving <> reth bytes mismatch");
    }
}

fuzz_target!(|data: &[u8]| {
    // call fuzzing in a separate function, so we can see its coverage
    fuzz(data);
});
