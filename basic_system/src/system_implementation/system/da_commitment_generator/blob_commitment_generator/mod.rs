use crate::system_functions::point_evaluation::{
    point_evaluation_as_system_function_inner, POINT_EVAL_PRECOMPILE_SUCCESS_RESPONSE,
};
use crate::system_implementation::system::da_commitment_generator::DACommitmentGenerator;
use arrayvec::ArrayVec;
use crypto::ark_ff::Field;
use crypto::ark_ff::One;
use crypto::ark_ff::PrimeField;
use crypto::ark_ff::Zero;
use crypto::sha3::Keccak256;
use crypto::{parse_u256_be, u256_to_be, MiniDigest};
use zk_ee::memory::ArrayBuilder;
use zk_ee::oracle::IOOracle;
use zk_ee::reference_implementations::{BaseResources, DecreasingNative};
use zk_ee::system::Resource;
use zk_ee::utils::write_bytes::WriteBytes;
use zk_ee::utils::Bytes32;

pub mod brp_roots_of_unity;
pub mod commitment_and_proof_advice;
pub mod polynomial_evaluation;

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
pub const ENCODABLE_BYTES_PER_BLOB: usize = BLOB_CHUNK_SIZE * ELEMENTS_PER_4844_BLOCK;

pub const MAX_NUMBER_OF_BLOBS: usize = 9;

pub const BUFFER_CAPACITY: usize = ENCODABLE_BYTES_PER_BLOB * MAX_NUMBER_OF_BLOBS;

///
/// Blobs DA commitment generator.
///
/// It encodes pubdata into the blobs using alloy default(`SimpleCoder`) encoding.
/// The first element in the first blob used to encode length as `[0, len BE, 23 zeroes] BE`.
/// Then blobs are filled with elements created from data chunked by 31 byte as `[0, chunk(31 byte)] BE`.
///
pub struct BlobCommitmentGenerator {
    buffer: ArrayVec<u8, BUFFER_CAPACITY>,
    versioned_hashes_hasher: Keccak256,
}

impl BlobCommitmentGenerator {
    pub fn new() -> Self {
        let mut buffer = ArrayVec::new();
        // we allocate 31 byte to encode length as a separate field element for convenience
        buffer.extend([0u8; 31]);
        Self {
            buffer,
            versioned_hashes_hasher: Keccak256::new(),
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
        self.buffer.try_extend_from_slice(buf).unwrap()
    }
}

impl<O: IOOracle> DACommitmentGenerator<O> for BlobCommitmentGenerator {
    fn da_commitment(&mut self, oracle: &mut O) -> Bytes32 {
        // len should be [0, len be, 23 zeroes] BE
        let length = self.buffer.len() - 31;
        self.buffer[0..8].copy_from_slice(&(length as u64).to_be_bytes());
        for chunk in self.buffer.chunks(ENCODABLE_BYTES_PER_BLOB) {
            cycle_marker::wrap!("blob_versioned_hash", {
                self.versioned_hashes_hasher
                    .update(&blob_versioned_hash(chunk, oracle));
            });
        }
        self.versioned_hashes_hasher.finalize_reset().into()
    }
}

///
/// Returns blob versioned hash.
///
/// Please note, that `data` is not the blob itself, but data we encode into the blob.
/// For encoding, we chunk `data` by 31 bytes and interpret each chunk as BE blob element.
///
fn blob_versioned_hash(data: &[u8], oracle: &mut impl IOOracle) -> [u8; 32] {
    let commitment_and_proof =
        commitment_and_proof_advice::blob_commitment_and_proof_advice(data, oracle);
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
    let mut inf_resources = <BaseResources<DecreasingNative> as Resource>::FORMAL_INFINITE;
    point_evaluation_as_system_function_inner(
        &buffer,
        &mut point_evaluation_output,
        &mut inf_resources,
    )
    .unwrap();
    assert_eq!(
        point_evaluation_output.build(),
        POINT_EVAL_PRECOMPILE_SUCCESS_RESPONSE
    );

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
