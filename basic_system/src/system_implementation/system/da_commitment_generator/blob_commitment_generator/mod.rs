use arrayvec::ArrayVec;
use crypto::ark_ff::PrimeField;
use crypto::ark_ff::Zero;
use crypto::ark_ff::One;
use crypto::ark_ff::Field;
use crypto::{BigInt, MiniDigest, parse_u256_be, parse_u256_le, u256_to_be};
use crypto::sha3::Keccak256;
use crypto::BigInteger;
use zk_ee::memory::ArrayBuilder;
use zk_ee::oracle::IOOracle;
use zk_ee::reference_implementations::{BaseResources, DecreasingNative};
use zk_ee::system::Resource;
use zk_ee::utils::Bytes32;
use zk_ee::utils::write_bytes::WriteBytes;
use crate::system_functions::point_evaluation::{POINT_EVAL_PRECOMPILE_SUCCESS_RESPONSE, point_evaluation_as_system_function_inner};
use crate::system_implementation::system::da_commitment_generator::DACommitmentGenerator;

pub mod commitment_and_proof_advice;
pub mod polynomial_evaluation;
pub mod brp_roots_of_unity;

///
/// Number of bytes we encode in one blob element
///
pub const BLOB_CHUNK_SIZE: usize = 31;

///
/// Number of field elements per EIP-4844 blob
///
pub const ELEMENTS_PER_4844_BLOCK: usize = 4096;

///
/// Number of bytes we can encode in one blob
///
// 1 element is used to encode len(following the alloy encoding)
pub const ENCODABLE_BYTES_PER_BLOB: usize = (BLOB_CHUNK_SIZE - 1) * ELEMENTS_PER_4844_BLOCK;

pub const MAX_NUMBER_OF_BLOBS: usize = 9;

pub const PUBDATA_WITH_BLOBS_LIMIT: usize = ENCODABLE_BYTES_PER_BLOB * MAX_NUMBER_OF_BLOBS;

pub struct BlobCommitmentGenerator {
    pubdata_buffer: ArrayVec<u8, PUBDATA_WITH_BLOBS_LIMIT>,
    versioned_hashes_hasher: Keccak256,
}

impl BlobCommitmentGenerator {
    pub fn new() -> Self {
        Self {
            pubdata_buffer: ArrayVec::new(),
            versioned_hashes_hasher: Keccak256::new()
        }
    }
}

impl WriteBytes for BlobCommitmentGenerator {
    // TODO: we can migrate to it, once we do input generation per batch
    // fn write(&mut self, buf: &[u8]) {
    //     if buf.len() < self.pubdata_buffer.capacity() - self.pubdata_buffer.len() {
    //         self.pubdata_buffer.try_extend_from_slice(buf).unwrap();
    //         return;
    //     }
    //     let (filling_part, remainder) = buf.split_at(self.pubdata_buffer.capacity() - self.pubdata_buffer.len());
    //     self.pubdata_buffer.try_extend_from_slice(filling_part).unwrap();
    //
    //     cycle_marker::wrap!("blob_versioned_hash", {
    //         self.versioned_hashes_hasher.update(&blob_versioned_hash(self.pubdata_buffer.as_slice()));
    //     });
    //     self.pubdata_buffer.clear();
    //     // theoretically remainder can be still bigger than buffer_capacity,
    //     // so we are making call to the `write` again to handle it recursively
    //     self.write(remainder);
    // }
    fn write(&mut self, buf: &[u8]) {
        // overflow shouldn't be reachable, operator validates pubdata limit during forward run
        self.pubdata_buffer.try_extend_from_slice(buf).unwrap()
    }
}

impl<O: IOOracle> DACommitmentGenerator<O> for BlobCommitmentGenerator {
    fn da_commitment(&mut self, oracle: &mut O) -> Bytes32 {
        if !self.pubdata_buffer.is_empty() {
            cycle_marker::wrap!("blob_versioned_hash", {
                self.versioned_hashes_hasher.update(&blob_versioned_hash(self.pubdata_buffer.as_slice(), oracle));
            });
        }
        self.versioned_hashes_hasher.finalize_reset().into()
    }
}


fn blob_versioned_hash(data: &[u8], oracle: &mut impl IOOracle) -> [u8; 32] {
    let commitment_and_proof = commitment_and_proof_advice::blob_commitment_and_proof_advice(data, oracle);
    let versioned_hash = versioned_hash_for_kzg(&commitment_and_proof[..48]);
    let evaluation_point = calculate_evaluation_point(data, &versioned_hash);
    let opening_value = polynomial_evaluation::evaluate_blob_polynomial(data, &evaluation_point);


    let mut buffer = [0u8; 192];
    buffer[0..32].copy_from_slice(&versioned_hash);
    buffer[32..64].copy_from_slice(&u256_to_be(evaluation_point.into_bigint()));
    buffer[64..96].copy_from_slice(&u256_to_be(opening_value.into_bigint()));
    buffer[96..192].copy_from_slice(&commitment_and_proof);

    let mut point_evaluation_output = ArrayBuilder::<64>::default();
    // TODO: it will also verify versioned hash against commitment, what we don't need in fact
    point_evaluation_as_system_function_inner(&buffer, &mut point_evaluation_output, &mut <BaseResources<DecreasingNative> as Resource>::FORMAL_INFINITE).unwrap();
    assert_eq!(point_evaluation_output.build(), POINT_EVAL_PRECOMPILE_SUCCESS_RESPONSE);

    versioned_hash
}


fn versioned_hash_for_kzg(data: &[u8]) -> [u8; 32] {
    use crypto::sha256::Digest;
    let mut hash: [u8; 32] = crypto::sha256::Sha256::digest(data).into();
    hash[0] = 1; // KZG_VERSIONED_HASH_VERSION_BYTE

    hash
}

fn calculate_evaluation_point(data: &[u8], versioned_hash: &[u8]) -> crypto::bls12_381::Fr {
    let mut hasher = crypto::blake2s::Blake2s256::new();
    hasher.update(versioned_hash);
    hasher.update(data);
    let hash = hasher.finalize();
    // truncate hash to 128 bits
    // NOTE: it is safe to draw a random scalar at max 128 bits because of the schwartz zippel lemma
    crypto::bls12_381::Fr::from_bigint(parse_u256_be(hash.rsplit_array_ref::<16>().1)).unwrap()
}