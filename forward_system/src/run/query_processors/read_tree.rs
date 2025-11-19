use super::*;
use crate::run::ReadStorageTree;
use basic_system::system_implementation::flat_storage_model::*;
use basic_system::system_implementation::flat_storage_model::{
    ExactIndexQuery, PreviousIndexQuery, PROOF_FOR_INDEX_QUERY_ID,
};
use zk_ee::common_structs::derive_flat_storage_key;
use zk_ee::kv_markers::InitialStorageSlotData;
use zk_ee::kv_markers::StorageAddress;
use zk_ee::types_config::EthereumIOTypesConfig;
use zk_ee::{
    basic_queries::InitialStorageSlotQuery,
    kv_markers::{UsizeDeserializable, UsizeSerializable},
    system_io_oracle::{dyn_usize_iterator::DynUsizeIterator, SimpleOracleQuery},
    utils::Bytes32,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadTreeResponder<T: ReadStorageTree> {
    pub tree: T,
}

impl<T: ReadStorageTree> ReadTreeResponder<T> {
    const SUPPORTED_QUERY_IDS: &[u32] = &[
        InitialStorageSlotQuery::<EthereumIOTypesConfig>::QUERY_ID,
        PreviousIndexQuery::QUERY_ID,
        ExactIndexQuery::QUERY_ID,
        PROOF_FOR_INDEX_QUERY_ID,
    ];
}

impl<T: ReadStorageTree> OracleQueryProcessor for ReadTreeResponder<T> {
    fn supported_query_ids(&self) -> Vec<u32> {
        Self::SUPPORTED_QUERY_IDS.to_vec()
    }

    fn supports_query_id(&self, query_id: u32) -> bool {
        Self::SUPPORTED_QUERY_IDS.contains(&query_id)
    }

    fn process_buffered_query(
        &mut self,
        query_id: u32,
        query: Vec<usize>,
        _memory: &dyn U32Memory,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static + Send + Sync> {
        assert!(Self::SUPPORTED_QUERY_IDS.contains(&query_id));

        match query_id {
            PreviousIndexQuery::QUERY_ID => {
                let key = <PreviousIndexQuery as SimpleOracleQuery>::Input::from_iter(
                    &mut query.into_iter(),
                )
                .expect("must deserialize key");
                let prev_index = self.tree.prev_tree_index(key);

                DynUsizeIterator::from_constructor(prev_index, UsizeSerializable::iter)
            }
            ExactIndexQuery::QUERY_ID => {
                let key = <PreviousIndexQuery as SimpleOracleQuery>::Input::from_iter(
                    &mut query.into_iter(),
                )
                .expect("must deserialize key");
                let existing = self
                    .tree
                    .tree_index(key)
                    .expect("Reading index for key that is not in the tree");

                DynUsizeIterator::from_constructor(existing, UsizeSerializable::iter)
            }
            InitialStorageSlotQuery::<EthereumIOTypesConfig>::QUERY_ID => {
                let StorageAddress { address, key } = <InitialStorageSlotQuery<
                    EthereumIOTypesConfig,
                > as SimpleOracleQuery>::Input::from_iter(
                    &mut query.into_iter()
                )
                .expect("must deserialize the address/slot");
                let flat_key = derive_flat_storage_key(&address, &key);
                let slot_data: InitialStorageSlotData<EthereumIOTypesConfig> =
                    if let Some(cold) = self.tree.read(flat_key) {
                        InitialStorageSlotData {
                            initial_value: cold,
                            is_new_storage_slot: false,
                        }
                    } else {
                        // default value, but it's potentially new storage slot in state!
                        InitialStorageSlotData {
                            initial_value: Bytes32::ZERO,
                            is_new_storage_slot: true,
                        }
                    };
                DynUsizeIterator::from_constructor(slot_data, UsizeSerializable::iter)
            }
            PROOF_FOR_INDEX_QUERY_ID => {
                let index = u64::from_iter(&mut query.into_iter()).expect("must deserialize index");
                let existing = self.tree.merkle_proof(index);
                let proof = ValueAtIndexProof {
                    proof: ExistingReadProof { existing },
                };
                DynUsizeIterator::from_constructor(proof, UsizeSerializable::iter)
            }
            _ => unreachable!(),
        }
    }
}
