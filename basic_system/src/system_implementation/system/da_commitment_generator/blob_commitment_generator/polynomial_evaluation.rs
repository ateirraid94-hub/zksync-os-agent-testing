use super::*;
use super::brp_roots_of_unity::BRP_ROOTS_OF_UNITY;

///
/// Evaluate blob polynomial in the given point.
/// Follows alloy SimpleCoder data encoding format.
///
pub fn evaluate_blob_polynomial(data: &[u8], x: &crypto::bls12_381::Fr) -> crypto::bls12_381::Fr {
    let mut poly = [crypto::bls12_381::Fr::zero(); ELEMENTS_PER_4844_BLOCK];
    let mut poly_iter = poly.iter_mut();
    // len should be [0, len be, 23 zeroes] BE
    let mut length_element = [0u8; 31];
    length_element[..8].copy_from_slice(&(data.len() as u64).to_be_bytes());
    *poly_iter.next().unwrap() = crypto::bls12_381::Fr::from_bigint(parse_u256_be(&length_element)).unwrap();
    let chunks = data.array_chunks::<BLOB_CHUNK_SIZE>();
    let mut last_chunk = [0u8; 31];
    let remainder = chunks.remainder();
    last_chunk[..remainder.len()].copy_from_slice(remainder);
    for chunk in chunks {
        *poly_iter.next().unwrap() = crypto::bls12_381::Fr::from_bigint(parse_u256_be(chunk)).unwrap();
    }
    *poly_iter.next().unwrap() = crypto::bls12_381::Fr::from_bigint(parse_u256_be(&last_chunk)).unwrap();

    // barycentric Lagrange interpolation evaluation

    let mut inverses_in = [crypto::bls12_381::Fr::zero(); ELEMENTS_PER_4844_BLOCK];

    for i in 0..ELEMENTS_PER_4844_BLOCK {
        // If the point to evaluate at is one of the evaluation points by which the polynomial is
        // given, we can just return the result directly. Note that special-casing this is
        // necessary, as the formula below would divide by zero otherwise.
        if *x == BRP_ROOTS_OF_UNITY[i] {
            return poly[i];
        }
        inverses_in[i] = *x - BRP_ROOTS_OF_UNITY[i];
    }

    let inverses = fr_batch_inv(&inverses_in);

    let mut out = crypto::bls12_381::Fr::zero();
    let mut tmp = crypto::bls12_381::Fr::zero();
    for i in 0..ELEMENTS_PER_4844_BLOCK {
        tmp = inverses[i] * BRP_ROOTS_OF_UNITY[i];
        tmp *= poly[i];
        out += tmp;
    }

    tmp = crypto::bls12_381::Fr::from_bigint(parse_u256_be(&(ELEMENTS_PER_4844_BLOCK as u64).to_be_bytes())).unwrap();
    out /= tmp;
    tmp = x.pow([ELEMENTS_PER_4844_BLOCK as u64]);
    tmp -= crypto::bls12_381::Fr::one();
    out *= tmp;

    out
}

// TODO: it's not a good idea to pass big arrays at the stack, rust may be not really efficient...
#[inline(never)]
fn fr_batch_inv<const N: usize>(input: &[crypto::bls12_381::Fr; N]) -> [crypto::bls12_381::Fr; N] {
    let mut accumulator = crypto::bls12_381::Fr::one();
    let mut inverses = core::array::from_fn(|i| {
        let inverse = accumulator.clone();
        accumulator *= input[i];
        inverse
    });

    accumulator.inverse_in_place();

    for i in (0..N).rev() {
        inverses[i] *= accumulator;
        accumulator *= input[i];
    }

    inverses
}
