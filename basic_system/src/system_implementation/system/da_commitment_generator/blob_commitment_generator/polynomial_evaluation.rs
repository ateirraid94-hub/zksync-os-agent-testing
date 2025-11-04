use super::brp_roots_of_unity::BRP_ROOTS_OF_UNITY;
use super::*;
use core::mem::MaybeUninit;

///
/// Evaluate blob polynomial in the given point.
///
/// Please note, that `data` is not the blob itself, but data we encode into the blob.
/// For encoding, we chunk `data` by 31 bytes and interpret each chunk as BE blob element.
///
pub fn evaluate_blob_polynomial(data: &[u8], x: &crypto::bls12_381::Fr) -> crypto::bls12_381::Fr {
    let mut poly: [MaybeUninit<crypto::bls12_381::Fr>; ELEMENTS_PER_4844_BLOCK] =
        [MaybeUninit::uninit(); ELEMENTS_PER_4844_BLOCK];
    let mut poly_iter = poly.iter_mut();

    let chunks = data.array_chunks::<BLOB_CHUNK_SIZE>();
    let mut last_chunk = [0u8; 31];
    let remainder = chunks.remainder();
    last_chunk[..remainder.len()].copy_from_slice(remainder);
    for chunk in chunks {
        poly_iter
            .next()
            .unwrap()
            .write(crypto::bls12_381::Fr::from_bigint(parse_u256_be(chunk)).unwrap());
    }
    poly_iter
        .next()
        .unwrap()
        .write(crypto::bls12_381::Fr::from_bigint(parse_u256_be(&last_chunk)).unwrap());
    let poly = unsafe { MaybeUninit::array_assume_init(poly) };

    // barycentric Lagrange interpolation evaluation

    let inverses_in: [crypto::bls12_381::Fr; ELEMENTS_PER_4844_BLOCK] =
        core::array::from_fn(|i| *x - BRP_ROOTS_OF_UNITY[i]);

    // batch inverse

    let mut accumulator = crypto::bls12_381::Fr::one();
    let mut inverses: [crypto::bls12_381::Fr; ELEMENTS_PER_4844_BLOCK] =
        core::array::from_fn(|i| {
            let inverse = accumulator.clone();
            accumulator *= inverses_in[i];
            inverse
        });

    match accumulator.inverse_in_place() {
        None => {
            // It's `None` when accumulator equals to zero, it means that point to evaluate at is
            // one of the evaluation points by which the polynomial is given, we can just return
            // the result directly.
            for i in 0..ELEMENTS_PER_4844_BLOCK {
                if *x == BRP_ROOTS_OF_UNITY[i] {
                    return poly[i];
                }
            }
            unreachable!()
        }
        Some(_) => {}
    }

    for i in (0..ELEMENTS_PER_4844_BLOCK).rev() {
        inverses[i] *= accumulator;
        accumulator *= inverses_in[i];
    }

    let mut out = crypto::bls12_381::Fr::zero();
    let mut tmp: crypto::bls12_381::Fr;
    for i in 0..ELEMENTS_PER_4844_BLOCK {
        tmp = inverses[i] * BRP_ROOTS_OF_UNITY[i];
        tmp *= poly[i];
        out += tmp;
    }

    tmp = crypto::bls12_381::Fr::from_bigint(parse_u256_be(
        &(ELEMENTS_PER_4844_BLOCK as u64).to_be_bytes(),
    ))
    .unwrap();
    out /= tmp;
    tmp = x.pow([ELEMENTS_PER_4844_BLOCK as u64]);
    tmp -= crypto::bls12_381::Fr::one();
    out *= tmp;

    out
}
