use basic_system::system_functions::modexp::{ModExpAdviceParams, MODEXP_ADVICE_QUERY_ID};
use oracle_provider::OracleQueryProcessor;
use risc_v_simulator::abstractions::memory::MemorySource;
use zk_ee::oracle::query_ids::U256_DIV_REM_ADVICE_QUERY_ID;

use crate::utils::{
    evaluate::{read_memory_as_u64, read_struct},
    usize_slice_iterator::UsizeSliceIteratorOwned,
};

// The u256 crate cannot depend on zk_ee, so it duplicates the query ID as a raw u32 literal
// (0x4005_0030). This compile-time check ensures the two definitions stay in sync.
const _: () = assert!(U256_DIV_REM_ADVICE_QUERY_ID == 0x4005_0030);

pub struct ArithmeticQuery<M: MemorySource> {
    _marker: std::marker::PhantomData<M>,
}

impl<M: MemorySource> Default for ArithmeticQuery<M> {
    fn default() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<M: MemorySource> ArithmeticQuery<M> {
    /// Handle U256 div_rem oracle query.
    ///
    /// Input: 1 packed usize containing two u32 pointers (dividend_ptr in low, divisor_ptr in high).
    /// Output: 8 usize values — 4 u64 limbs for quotient, then 4 u64 limbs for remainder (LE).
    fn process_u256_div_rem_query(
        &self,
        query: Vec<usize>,
        memory: &M,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static + Send + Sync> {
        let mut it = query.into_iter();
        // Two u32 CSR writes get packed into one usize (u64) by QueryBuffer:
        // low 32 bits = dividend_ptr, high 32 bits = divisor_ptr
        let packed = it.next().expect("expected packed pointers");
        assert!(it.next().is_none(), "expected exactly 1 packed usize");

        let dividend_ptr = packed as u32;
        let divisor_ptr = (packed >> 32) as u32;

        // Read one U256 (4 u64 limbs = 32 bytes) from each pointer
        let mut dividend = read_memory_as_u64(memory, dividend_ptr, 4).unwrap();
        let mut divisor = read_memory_as_u64(memory, divisor_ptr, 4).unwrap();

        // ruint::algorithms::div: dividend becomes quotient, divisor becomes remainder
        ruint::algorithms::div(&mut dividend, &mut divisor);

        // Return 8 usize (u64) values: 4 quotient limbs + 4 remainder limbs.
        // The oracle infrastructure splits each usize into two u32 reads for the guest,
        // so the guest sees 16 u32 words total.
        let mut result = Vec::with_capacity(8);
        for limb in dividend.iter() {
            result.push(*limb as usize);
        }
        for limb in divisor.iter() {
            result.push(*limb as usize);
        }

        Box::new(UsizeSliceIteratorOwned::new(result.into_boxed_slice()))
    }
}

impl<M: MemorySource> OracleQueryProcessor<M> for ArithmeticQuery<M> {
    fn supported_query_ids(&self) -> Vec<u32> {
        vec![MODEXP_ADVICE_QUERY_ID, U256_DIV_REM_ADVICE_QUERY_ID]
    }

    fn process_buffered_query(
        &mut self,
        query_id: u32,
        query: Vec<usize>,
        memory: &M,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static + Send + Sync> {
        debug_assert!(self.supports_query_id(query_id));

        if query_id == U256_DIV_REM_ADVICE_QUERY_ID {
            return self.process_u256_div_rem_query(query, memory);
        }

        // ---- Existing modexp handling below ----

        let mut it = query.into_iter();

        let arg_ptr = it.next().expect("A u32 should've been passed in.");

        assert!(
            it.next().is_none(),
            "A single RISC-V ptr should've been passed."
        );

        assert!(arg_ptr.is_multiple_of(4));
        const { assert!(core::mem::align_of::<ModExpAdviceParams>() == 4) }
        const { assert!(core::mem::size_of::<ModExpAdviceParams>().is_multiple_of(4)) }

        let arg = unsafe { read_struct::<ModExpAdviceParams, _>(memory, arg_ptr as u32) }.unwrap();

        const { assert!(8 == core::mem::size_of::<usize>()) };
        assert!(arg.a_ptr > 0);
        let mut n = read_memory_as_u64(memory, arg.a_ptr, arg.a_len * 4).unwrap();
        assert_eq!(arg.b_ptr, 0);
        assert_eq!(arg.b_len, 0);
        assert!(arg.modulus_ptr > 0);
        assert!(arg.modulus_len > 0);
        let mut d = read_memory_as_u64(memory, arg.modulus_ptr, arg.modulus_len * 4).unwrap();

        ruint::algorithms::div(&mut n, &mut d);

        // Trim zeros
        fn strip_leading_zeroes(input: &[u64]) -> &[u64] {
            let mut digits = input.len();
            for el in input.iter().rev() {
                if *el == 0 {
                    digits -= 1;
                } else {
                    break;
                }
            }
            &input[..digits]
        }
        let quotient = strip_leading_zeroes(&n);
        let remainder = strip_leading_zeroes(&d);

        // account for usize being u64 here
        let q_len_in_u32_words = quotient.len() * 2;
        let r_len_in_u32_words = remainder.len() * 2;
        // account for LE, and we will ask quotient first, then remainder
        let header = [(q_len_in_u32_words as u64) | ((r_len_in_u32_words as u64) << 32)];

        let r = header
            .iter()
            .chain(quotient.iter())
            .chain(remainder.iter())
            .map(|x| *x as usize)
            .collect::<Vec<_>>();
        let r = Vec::into_boxed_slice(r);

        let n = UsizeSliceIteratorOwned::new(r);

        Box::new(n)
    }
}
