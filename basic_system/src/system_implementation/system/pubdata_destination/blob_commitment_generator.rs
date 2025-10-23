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
use zk_ee::oracle::query_ids::ADVICE_SUBSPACE_MASK;
use zk_ee::reference_implementations::{BaseResources, DecreasingNative};
use zk_ee::system::Resource;
use zk_ee::utils::Bytes32;
use zk_ee::utils::write_bytes::WriteBytes;
use crate::system_functions::point_evaluation::{POINT_EVAL_PRECOMPILE_SUCCESS_RESPONSE, point_evaluation_as_system_function_inner};
use crate::system_implementation::system::pubdata_destination::DACommitmentGenerator;

/// Number of bytes we encode in one blob element
pub const BLOB_CHUNK_SIZE: usize = 31;
/// Number of element in one blob
pub const ELEMENTS_PER_4844_BLOCK: usize = 4096;
/// Number of bytes we can encode in one blob
// 1 element is used to encode len(following the alloy encoding)
pub const ENCODABLE_BYTES_PER_BLOB: usize = (BLOB_CHUNK_SIZE - 1) * ELEMENTS_PER_4844_BLOCK;

pub struct BlobCommitmentGenerator {
    pubdata_buffer: ArrayVec<u8, ENCODABLE_BYTES_PER_BLOB>,
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
    fn write(&mut self, buf: &[u8]) {
        if buf.len() < self.pubdata_buffer.capacity() - self.pubdata_buffer.len() {
            self.pubdata_buffer.try_extend_from_slice(buf).unwrap();
            return;
        }
        let (filling_part, remainder) = buf.split_at(self.pubdata_buffer.capacity() - self.pubdata_buffer.len());
        self.pubdata_buffer.try_extend_from_slice(filling_part).unwrap();

        let brp_roots_of_unity = blob_polynom::calculate_brp_root_of_unity();
        cycle_marker::wrap!("blob_versioned_hash", {
            self.versioned_hashes_hasher.update(&blob_versioned_hash(self.pubdata_buffer.as_slice(), &brp_roots_of_unity));
        });
        self.pubdata_buffer.clear();
        // theoretically remainder can be still bigger than buffer_capacity,
        // so we are making call to the `write` again to handle it recursively
        self.write(remainder);
    }
}

impl DACommitmentGenerator for BlobCommitmentGenerator {
    fn da_commitment(&mut self) -> Bytes32 {
        if !self.pubdata_buffer.is_empty() {
            let brp_roots_of_unity = blob_polynom::calculate_brp_root_of_unity();
            cycle_marker::wrap!("blob_versioned_hash", {
            self.versioned_hashes_hasher.update(&blob_versioned_hash(self.pubdata_buffer.as_slice(), &brp_roots_of_unity));
        });
        }
        self.versioned_hashes_hasher.finalize_reset().into()
    }
}

fn blob_versioned_hash(data: &[u8], brp_roots_of_unity: &[crypto::bls12_381::Fr]) -> [u8; 32] {
    let commitment_and_proof = blob_commitment_and_proof_advice(data);
    let versioned_hash = versioned_hash_for_kzg(&commitment_and_proof[..48]);
    let evaluation_point = calculate_evaluation_point(data, &versioned_hash);
    let opening_value = blob_polynom::evaluate_blob_polynomial(data, &evaluation_point, brp_roots_of_unity);

    #[cfg(target_arch = "riscv32")]
    {
        use core::fmt::Write;
        oracle::QuasiUART::new().write_fmt(format_args!("p commitment {:?}\n", &commitment_and_proof[0..48]));
        oracle::QuasiUART::new().write_fmt(format_args!("p versioned hash {:?}\n", &versioned_hash));
        let mut hasher = crypto::blake2s::Blake2s256::new();
        hasher.update(&versioned_hash);
        hasher.update(data);
        let hash = hasher.finalize();
        oracle::QuasiUART::new().write_fmt(format_args!("p hash {:?}\n", hash));
        oracle::QuasiUART::new().write_fmt(format_args!("p eval point {:?}\n", evaluation_point));
        oracle::QuasiUART::new().write_fmt(format_args!("p value {:?}\n", opening_value));
    }

    let mut buffer = [0u8; 192];
    buffer[0..32].copy_from_slice(&versioned_hash);
    buffer[32..64].copy_from_slice(&u256_to_be(evaluation_point.into_bigint()));
    buffer[64..96].copy_from_slice(&u256_to_be(opening_value.into_bigint()));
    buffer[96..192].copy_from_slice(&commitment_and_proof);

    let mut point_evaluation_output = ArrayBuilder::<64>::default();
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

pub const VERSIONED_HASH_ADVICE_QUERY_ID: u32 = ADVICE_SUBSPACE_MASK | 0x20;

#[cfg(target_arch = "riscv32")]
fn blob_commitment_and_proof_advice(
    data: &[u8]
) -> [u8; 96] {
    // TODO: rework to accept from outside, or think how to avoid duplication
    let mut oracle = oracle::CsrBasedIOOracle::<oracle::CSRBasedNonDeterminismSource>::init();
    let mut it = oracle
        .raw_query(
            VERSIONED_HASH_ADVICE_QUERY_ID,
            &(data.as_ptr() as usize as u32, data.len() as u32),
        )
        .unwrap();

    let mut buffer = [0u8; 96];
    unsafe {
        let buffer_u32_ptr: *mut u32 = buffer.as_mut_ptr().cast::<[u32; 24]>().cast();
        for i in 0..24 {
            buffer_u32_ptr.add(i).write(it.next().unwrap() as u32)
        }
    }
    assert!(it.next().is_none());
    buffer
}


#[cfg(not(target_arch = "riscv32"))]
fn blob_commitment_and_proof_advice(
    _blob: &[u8]
) -> [u8; 96] {
    [0u8; 96]
}

mod blob_polynom {
    use crypto::parse_u256_be;
    use super::*;

    const FIELD_ELEMENTS_PER_EXT_BLOB: usize = ELEMENTS_PER_4844_BLOCK << 1;

    const ROOT_OF_UNITY: BigInt<4> = BigInt([
        0x6fdd00bfc78c8967, 0x146b58bc434906ac, 0x2ccddea2972e89ed, 0x485d512737b1da3d
    ]);

    // TODO: precalculate
    pub fn calculate_brp_root_of_unity() -> [crypto::bls12_381::Fr; FIELD_ELEMENTS_PER_EXT_BLOB] {
        let mut roots_of_unity = [crypto::bls12_381::Fr::zero(); FIELD_ELEMENTS_PER_EXT_BLOB];
        roots_of_unity[0] = crypto::bls12_381::Fr::one();
        roots_of_unity[1] = crypto::bls12_381::Fr::from_bigint(ROOT_OF_UNITY).unwrap();
        for i in 2..FIELD_ELEMENTS_PER_EXT_BLOB {
            roots_of_unity[i] = roots_of_unity[i-1]*roots_of_unity[1];
            if roots_of_unity[i].is_one() {
                break;
            }
        }
        fn log2_pow2(mut n: u64) -> u64 {
            let mut position = 0;
            n >>= 1;
            while n > 0 {
                position += 1;
                n >>= 1;
            }
            position
        }

        fn reverse_bits(mut n: u64) -> u64 {
            let mut result = 0;
            for _ in 0..64 {
                result <<= 1;
                result |= (n & 1);
                n >>= 1;
            }
            result
        }
        let unused_bit_len = 64 - log2_pow2(FIELD_ELEMENTS_PER_EXT_BLOB as u64);
        for i in 0..FIELD_ELEMENTS_PER_EXT_BLOB {
            let r = (reverse_bits(i as u64) >> unused_bit_len) as usize;
            if r > i {
                roots_of_unity.swap(i, r);
            }
        }
        roots_of_unity
    }

    ///
    /// Evaluate blob polynomial in the given point.
    /// Please note that the function accepts not the blob itself, but the data that will be encoded in the blob.
    ///
    /// For encoding, it follows alloy default(`SimpleCoder` format):
    /// - 1st element is `Fr::from_be([0, data_len_be, 23 zeroes])`
    /// - `i`-th element(`i > 0`) is `Fr::from_be([0, data[(i-1)*31..i*31])`(`data` padded with zeroes if needed)
    ///
    pub fn evaluate_blob_polynomial(data: &[u8], x: &crypto::bls12_381::Fr, brp_roots_of_unity: &[crypto::bls12_381::Fr]) -> crypto::bls12_381::Fr {
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
            // TODO: can be checked by x^4096 == 0 or even in inverse_in(if grand product == 0)
            if *x == brp_roots_of_unity[i] {
                return poly[i];
            }
            inverses_in[i] = *x - brp_roots_of_unity[i];
        }

        let inverses = fr_batch_inv(&inverses_in);

        let mut out = crypto::bls12_381::Fr::zero();
        let mut tmp = crypto::bls12_381::Fr::zero();
        for i in 0..ELEMENTS_PER_4844_BLOCK {
            tmp = inverses[i] * brp_roots_of_unity[i];
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
}

// TODO: duplicate from proof system, rework oracle part
#[cfg(target_arch = "riscv32")]
mod oracle {
    use riscv_common::{csr_read_word, csr_write_word};

    #[derive(Clone, Copy, Debug)]
    pub struct CSRBasedNonDeterminismSource;

    impl NonDeterminismCSRSourceImplementation for CSRBasedNonDeterminismSource
    {
        #[inline(always)]
        fn csr_read_impl() -> usize {
            const {
                assert!(core::mem::size_of::<usize>() == core::mem::size_of::<u32>());
            }
            csr_read_word() as usize
        }
        #[inline(always)]
        fn csr_write_impl(value: usize) {
            core::hint::black_box(csr_write_word(value))
        }
    }

    use zk_ee::{
        oracle::usize_serialization::{UsizeDeserializable, UsizeSerializable},
        oracle::IOOracle,
        system::errors::internal::InternalError,
    };

    pub trait NonDeterminismCSRSourceImplementation: 'static + Clone + Copy + core::fmt::Debug {
        fn csr_read_impl() -> usize;
        fn csr_write_impl(value: usize);
    }

    #[derive(Clone, Copy, Debug)]
    pub struct CsrBasedIOOracle<I: NonDeterminismCSRSourceImplementation> {
        _marker: core::marker::PhantomData<I>,
    }

    pub struct CsrBasedIOOracleIterator<I: NonDeterminismCSRSourceImplementation> {
        remaining: usize,
        _marker: core::marker::PhantomData<I>,
    }

    impl<I: NonDeterminismCSRSourceImplementation> Iterator for CsrBasedIOOracleIterator<I> {
        type Item = usize;
        fn next(&mut self) -> Option<Self::Item> {
            if self.remaining == 0 {
                None
            } else {
                self.remaining -= 1;
                Some(I::csr_read_impl())
            }
        }
    }

    impl<I: NonDeterminismCSRSourceImplementation> ExactSizeIterator for CsrBasedIOOracleIterator<I> {
        fn len(&self) -> usize {
            self.remaining
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct DummyCSRImpl;

    impl NonDeterminismCSRSourceImplementation for DummyCSRImpl {
        fn csr_read_impl() -> usize {
            0
        }
        fn csr_write_impl(_value: usize) {}
    }
    impl<I: NonDeterminismCSRSourceImplementation> CsrBasedIOOracle<I> {
        pub fn init() -> Self {
            Self {
                _marker: core::marker::PhantomData,
            }
        }
    }

    impl<NDS: NonDeterminismCSRSourceImplementation> IOOracle for CsrBasedIOOracle<NDS> {
        type RawIterator<'a> = CsrBasedIOOracleIterator<NDS>;

        fn raw_query<'a, I: UsizeSerializable + UsizeDeserializable>(
            &'a mut self,
            query_type: u32,
            input: &I,
        ) -> Result<Self::RawIterator<'a>, InternalError> {
            const {
                assert!(core::mem::size_of::<usize>() == core::mem::size_of::<u32>());
            }
            NDS::csr_write_impl(query_type as usize);
            let iter_to_write = UsizeSerializable::iter(input);
            // write length
            let iterator_len = iter_to_write.len();
            assert!(iterator_len == <I as UsizeSerializable>::USIZE_LEN);
            NDS::csr_write_impl(iterator_len);
            // write content
            let mut remaining_len = iterator_len;
            for value in iter_to_write {
                assert!(iterator_len != 0);
                NDS::csr_write_impl(value);
                remaining_len -= 1;
            }
            assert!(remaining_len == 0);
            // we can expect that length of the result is returned via read
            let remaining_len = NDS::csr_read_impl();
            let it = CsrBasedIOOracleIterator::<NDS> {
                remaining: remaining_len,
                _marker: core::marker::PhantomData,
            };

            Ok(it)
        }
    }


    #[derive(Default)]
    pub struct QuasiUART {
        buffer: [u8; 4],
        len: usize,
    }

    impl QuasiUART {
        const HELLO_MARKER: u32 = u32::MAX;

        #[inline(never)]
        pub const fn new() -> Self {
            Self {
                buffer: [0u8; 4],
                len: 0,
            }
        }

        #[inline(never)]
        pub fn write_entry_sequence(&mut self, message_len: usize) {
            csr_write_word(Self::HELLO_MARKER as usize);
            // now write length is words for query
            csr_write_word(message_len.next_multiple_of(4) / 4 + 1);
            csr_write_word(message_len);
        }

        #[inline(never)]
        pub fn write_word(&self, word: u32) {
            csr_write_word(word as usize);
        }

        #[inline(never)]
        pub fn read_word(&self) -> usize {
            csr_read_word() as usize
        }

        #[inline(never)]
        fn write_byte(&mut self, byte: u8) {
            self.buffer[self.len] = byte;
            self.len += 1;
            if self.len == 4 {
                self.len = 0;
                let word = u32::from_le_bytes(self.buffer);
                self.write_word(word);
            }
        }

        fn flush(&mut self) {
            if self.len == 0 {
                // cleanup and return
                for dst in self.buffer.iter_mut() {
                    *dst = 0;
                }
                return;
            }
            for i in self.len..4 {
                self.buffer[i] = 0u8;
            }
            self.len = 0;
            csr_write_word(u32::from_le_bytes(self.buffer) as usize);
        }
    }

    impl core::fmt::Write for QuasiUART {
        fn write_str(&mut self, s: &str) -> Result<(), core::fmt::Error> {
            self.write_entry_sequence(s.len());
            for c in s.bytes() {
                self.write_byte(c);
            }
            self.flush();

            Ok(())
        }
    }
}