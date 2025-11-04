#![no_main]
#![feature(allocator_api)]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use fuzz_precompiles_forward::precompiles::ecadd as ecadd_forward;
use fuzz_precompiles_proving::precompiles::ecadd as ecadd_proving;
use revm_precompile::bn254;
use crate::common::{PointKind,CoordMut,build_point_bytes};

mod common;

const BN254_ECADD_SRC_REQUIRED_LENGTH: usize = 128;
const BN254_ECADD_DST_MIN_LENGTH: usize = 64;

#[derive(Arbitrary, Debug, Clone)]
struct Input {
    // Point generation class
    kind1: PointKind,
    kind2: PointKind,

    // Scalars for constructing valid points
    s1_bytes: [u8; 32],
    s2_bytes: [u8; 32],

    // Coordinate-level mutations
    m_x1: CoordMut,
    m_y1: CoordMut,
    m_x2: CoordMut,
    m_y2: CoordMut,

    // Decide which coordinate to mutate
    spice: u8,

    // Raw bytes
    raw_p1: [u8; 64],
    raw_p2: [u8; 64],

    // Mutate the input bytes by trimming length
    #[arbitrary(with = mutate_len)]
    max_len: u8,
}

fn mutate_len(u: &mut Unstructured<'_>) -> arbitrary::Result<u8> {
    let x: u8 = u.arbitrary()?;
    let len = match x % 8 {
        0 => 0,
        1 => 1,
        2 => BN254_ECADD_SRC_REQUIRED_LENGTH as u8 / 2,
        3 => BN254_ECADD_SRC_REQUIRED_LENGTH as u8 - 1,
        _ => BN254_ECADD_SRC_REQUIRED_LENGTH as u8,
    };
    Ok(len)
}

fn build_input_bytes(i: &Input) -> [u8; BN254_ECADD_SRC_REQUIRED_LENGTH] {
    let p1 = build_point_bytes(i.kind1, i.s1_bytes, i.m_x1, i.m_y1, i.spice, i.raw_p1);
    let p2 = build_point_bytes(i.kind2, i.s2_bytes, i.m_x2, i.m_y2, i.spice.rotate_left(1), i.raw_p2);
    let mut buf = [0u8; BN254_ECADD_SRC_REQUIRED_LENGTH];
    buf[..64].copy_from_slice(&p1);
    buf[64..].copy_from_slice(&p2);
    buf
}

fn fuzz(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let input = match Input::arbitrary(&mut u) {
        Ok(x) => x,
        Err(_) => return,
    };

    let in_bytes = build_input_bytes(&input);
    let n = usize::from(input.max_len);
    let input_slice = &in_bytes[..n.min(BN254_ECADD_SRC_REQUIRED_LENGTH)];

    let mut dst1 = Vec::new();
    let mut dst2 = Vec::new();

    let r_reth = bn254::run_add(input_slice, u64::MAX, u64::MAX);
    let reth_ok = r_reth.as_ref().is_ok_and(|x| !x.reverted);

    let r1 = ecadd_forward(input_slice, &mut dst1);
    let r1_ok = r1.is_ok();

    // Skip if both RETH and Forward run failed
    if !reth_ok && !r1_ok {
        return;
    }

    let r2 = ecadd_proving(input_slice, &mut dst2);
    let r2_ok = r2.is_ok();

    if reth_ok || r1_ok || r2_ok {
        assert!(reth_ok,  "reth reverted but others accepted");
        assert!(r1_ok,   "forward run rejected but reth accepted");
        assert!(r2_ok,   "proving run rejected but reth accepted");

        let reth_out = r_reth.unwrap().bytes.to_vec();

        assert_eq!(reth_out.len(), BN254_ECADD_DST_MIN_LENGTH, "reth output length mismatch");
        assert_eq!(dst1.len(), BN254_ECADD_DST_MIN_LENGTH, "forward output length mismatch");
        assert_eq!(dst2.len(), BN254_ECADD_DST_MIN_LENGTH, "proving output length mismatch");

        assert_eq!(dst1, reth_out, "forward <> reth bytes mismatch");
        assert_eq!(dst2, reth_out, "proving <> reth bytes mismatch");
    }
}

fuzz_target!(|data: &[u8]| {
    // call fuzzing in a separate function, so we can see its coverage
    fuzz(data);
});
