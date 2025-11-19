use super::*;
use basic_system::system_implementation::flat_storage_model::FlatStorageCommitment;
use basic_system::system_implementation::flat_storage_model::TREE_HEIGHT;
use zk_ee::basic_queries::ZKProofDataQuery;
use zk_ee::common_structs::ProofData;
use zk_ee::system_io_oracle::SimpleOracleQuery;
use zk_ee::types_config::EthereumIOTypesConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZKProofDataResponder {
    pub data: Option<ProofData<FlatStorageCommitment<TREE_HEIGHT>>>,
}

impl ZKProofDataResponder {
    const SUPPORTED_QUERY_IDS: &[u32] =
        &[ZKProofDataQuery::<EthereumIOTypesConfig, FlatStorageCommitment<TREE_HEIGHT>>::QUERY_ID];
}

impl OracleQueryProcessor for ZKProofDataResponder {
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
        _memory: &dyn U32Memory,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static + Send + Sync> {
        assert!(Self::SUPPORTED_QUERY_IDS.contains(&query_id));

        let data = self
            .data
            .take()
            .expect("io implementer data is none (second read or not set initially)");

        DynUsizeIterator::from_constructor(data, UsizeSerializable::iter)
    }
}
