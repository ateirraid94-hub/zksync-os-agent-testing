use alloy_rlp::Encodable;
use arrayvec::ArrayVec;
use crypto::MiniDigest;
use crypto::sha3::Keccak256;
use zk_ee::utils::Bytes32;
use zk_ee::utils::write_bytes::WriteBytes;
use crate::system_functions::modexp::ModExpAdviceParams;

pub const BLOB_CHUNK_SIZE: usize = 31;
pub const ELEMENTS_PER_4844_BLOCK: usize = 4096;
pub const ENCODABLE_BYTES_PER_BLOB: usize = BLOB_CHUNK_SIZE * ELEMENTS_PER_4844_BLOCK;

pub struct BlobCommitmentGenerator {
    pubdata_buffer: ArrayVec<u8, ENCODABLE_BYTES_PER_BLOB>,
    versioned_hashes_hasher: Keccak256,
    // TODO: length?
}


impl WriteBytes for BlobCommitmentGenerator {
    fn write(&mut self, buf: impl AsRef<[u8]>) {
        if buf.as_ref().length() < self.pubdata_buffer.capacity() - self.pubdata_buffer.len() {
            self.pubdata_buffer.try_extend_from_slice(buf).unwrap();
            return;
        }
        let (filling_part, remainder) = buf.as_ref().split_at(self.pubdata_buffer.capacity() - self.pubdata_buffer.len());
        self.pubdata_buffer.try_extend_from_slice(filling_part).unwrap();

        self.versioned_hashes_hasher.update(blob_versioned_hash(self.pubdata_buffer.as_slice()));

        self.pubdata_buffer.clear();
        // theoretically remainder can be still bigger than buffer_capacity,
        // so we are making call to the `write` again to handle it recursively
        self.write(remainder);
    }
}

fn blob_versioned_hash(blob: &[u8]) -> Bytes32 {
    // TODO: we need to:
    // 1. get commitment and kzg proof(maybe also versioned hash + point + value)
    // 2. Calculate versioned hash(can be part of point eval)
    // 3. Calculate z(point to veify)
    // 4. Calculate polynom value
    // 5. Verify versioned hash corresponds to value in this point (point eval)
    let blob_pointer = blob.as_ptr() as usize as u32;

    // buffer to save commitment advice from oracle,
    // follows point evaluation system function layout
    let buffer = [0u8; 192];
    // write versioned hash, commitment, and proof
    let mut hasher = crypto::blake2s::Blake2s256::new();
    hasher.update(blob);
    hasher.update(buffer[..32]);
    // truncate hash to 128 bits
    // NOTE: it is safe to draw a random scalar at max 128 bits because of the schwartz zippel
    // lemma
    let challenge = hasher.finalize()[0..16];
    let mut truncated_hash = [zero_u8; 16];
    let challenge_hash = boojum::gadgets::keccak256::keccak256(
        cs,
        linear_hash_output
            .into_iter()
            .chain(versioned_hash.into_iter())
            .collect::<Vec<UInt8<F>>>()
            .as_slice(),
    );
    todo!()
}

fn blob_polynom_value_at_point(blob: &[u8], challenge: &[u8; 16]) -> Bytes32 {
    // We do not need internal representation, just canonical scalar
    fn parse_scalar(input: &[u8; 32]) -> Result<<crypto::bls12_381::Fr as PrimeField>::BigInt, ()> {
        // Arkworks has strange format for integer serialization, so we do manually
        let mut repr = [0u64; 4];
        for (dst, src) in repr.iter_mut().zip(input.as_rchunks::<8>().1.iter().rev()) {
            *dst = u64::from_be_bytes(*src);
        }
        let repr = crypto::BigInt::new(repr);
        if repr >= crypto::bls12_381::Fr::MODULUS {
            Err(())
        } else {
            Ok(repr)
        }
    }

    let mut repr = [0u64; 4];
    for (dst, src) in repr.iter_mut().zip(challenge.as_rchunks::<8>().1.iter().rev()) {
        *dst = u64::from_be_bytes(*src);
    }
    let repr = crypto::BigInt::new(repr);


}