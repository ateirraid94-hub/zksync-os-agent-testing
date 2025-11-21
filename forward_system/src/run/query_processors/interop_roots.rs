use std::alloc::Global;

use super::*;
use oracle_provider::OracleQueryProcessor;
use zk_ee::oracle::{
    query_ids::INTEROP_ROOTS_QUERY_ID,
    usize_serialization::usize_serializable_dynamic::UsizeSerializableDynamic,
};

/// Oracle query processor for interop roots data.
/// Provides interoperability root information to the ZKsync OS runtime.
#[cfg_attr(feature = "testing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub struct InteropRootsResponder {
    pub interop_roots: Vec<InteropRoot>,
}

impl InteropRootsResponder {
    const SUPPORTED_QUERY_IDS: &[u32] = &[INTEROP_ROOTS_QUERY_ID];
}

impl<M: MemorySource> OracleQueryProcessor<M> for InteropRootsResponder {
    fn supported_query_ids(&self) -> Vec<u32> {
        Self::SUPPORTED_QUERY_IDS.to_vec()
    }

    fn supports_query_id(&self, query_id: u32) -> bool {
        Self::SUPPORTED_QUERY_IDS.contains(&query_id)
    }

    fn process_buffered_query(
        &mut self,
        query_id: u32,
        _query: Vec<usize>,
        _memory: &M,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static> {
        assert!(Self::SUPPORTED_QUERY_IDS.contains(&query_id));

        DynUsizeIterator::from_constructor(self.interop_roots.clone(), |x| {
            UsizeSerializableDynamic::iter(x, Global)
        })
    }
}
