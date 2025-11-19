use basic_system::system_functions::{FieldHintOp, FieldOpsHint, FIELD_OPS_ADVISE_QUERY_ID};
use oracle_provider::OracleQueryProcessor;
use oracle_provider::U32Memory;
use zk_ee::kv_markers::UsizeSerializable;
use zk_ee::system_io_oracle::dyn_usize_iterator::DynUsizeIterator;
use zk_ee::utils::Bytes32;

mod impls;

use crate::utils::evaluate::{read_memory_as_u64, read_struct};

#[derive(Default)]
pub struct FieldOpsQuery;

impl OracleQueryProcessor for FieldOpsQuery {
    fn supported_query_ids(&self) -> Vec<u32> {
        vec![FIELD_OPS_ADVISE_QUERY_ID]
    }

    fn process_buffered_query(
        &mut self,
        query_id: u32,
        query: Vec<usize>,
        memory: &dyn U32Memory,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static + Send + Sync> {
        debug_assert!(self.supports_query_id(query_id));

        let mut it = query.into_iter();

        let arg_ptr = it.next().expect("A u32 should've been passed in.");

        assert!(
            it.next().is_none(),
            "A single RISC-V ptr should've been passed."
        );

        assert!(arg_ptr % 4 == 0);
        const { assert!(core::mem::align_of::<FieldOpsHint>() == 4) }
        const { assert!(core::mem::size_of::<FieldOpsHint>() % 4 == 0) }

        let arg = unsafe { read_struct::<FieldOpsHint>(memory, arg_ptr as u32) }.unwrap();

        let Some(op) = FieldHintOp::parse_u32(arg.op) else {
            panic!("Unknown field hint op {}", arg.op);
        };

        const { assert!(8 == core::mem::size_of::<usize>()) };
        assert!(arg.src_ptr > 0);
        assert_eq!(arg.src_len_u32_words, 8);
        let n = read_memory_as_u64(memory, arg.src_ptr, arg.src_len_u32_words / 2).unwrap();
        let n = Bytes32::from_array(
            n.into_iter()
                .map(|el| el.to_le_bytes())
                .flatten()
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        );

        match op {
            FieldHintOp::Secp256k1BaseFieldSqrt => {
                let t = impls::secp256k1_base_field_sqrt(n);
                DynUsizeIterator::from_constructor(t, UsizeSerializable::iter)
            }
            FieldHintOp::Secp256k1BaseFieldInverse => {
                let t = impls::secp256k1_base_field_inverse(n);
                DynUsizeIterator::from_constructor(t, UsizeSerializable::iter)
            }
            FieldHintOp::Secp256k1ScalarFieldInverse => {
                let t = impls::secp256k1_scalar_field_inverse(n);
                DynUsizeIterator::from_constructor(t, UsizeSerializable::iter)
            }
            _ => {
                panic!("Unknown field hint op {}", arg.op);
            }
        }
    }
}
