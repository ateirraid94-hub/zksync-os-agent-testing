use crate::run::StorageCommitment;
use crate::run::{NextTxResponse, PreimageSource, ReadStorage, ReadStorageTree, TxSource};
use oracle_provider::MemorySource;
use oracle_provider::OracleQueryProcessor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use zk_ee::common_structs::ProofData;
use zk_ee::oracle::basic_queries::ZKProofDataQuery;
use zk_ee::oracle::query_ids::BLOCK_METADATA_QUERY_ID;
use zk_ee::oracle::simple_oracle_query::SimpleOracleQuery;
use zk_ee::oracle::usize_serialization::dyn_usize_iterator::DynUsizeIterator;
use zk_ee::oracle::usize_serialization::UsizeSerializable;

use super::BlockContext;

#[derive(Debug)]
pub struct NativeBatchBlockInput<T, PS, TS> {
    pub block_context: BlockContext,
    pub proof_data: ProofData<StorageCommitment>,
    pub tree: T,
    pub preimage_source: PS,
    pub tx_source: TS,
}

#[derive(Debug)]
pub struct BatchCursor {
    index: Arc<AtomicUsize>,
    len: usize,
}

impl BatchCursor {
    pub fn new(len: usize) -> Self {
        assert!(
            len > 0,
            "batch-native prover input requires at least one block"
        );
        Self {
            index: Arc::new(AtomicUsize::new(0)),
            len,
        }
    }

    pub fn current(&self) -> usize {
        self.index.load(Ordering::SeqCst).min(self.len - 1)
    }

    pub fn advance(&self) {
        let current = self.index.load(Ordering::SeqCst);
        if current + 1 < self.len {
            self.index.store(current + 1, Ordering::SeqCst);
        }
    }
}

impl Clone for BatchCursor {
    fn clone(&self) -> Self {
        Self {
            index: Arc::clone(&self.index),
            len: self.len,
        }
    }
}

#[derive(Debug)]
pub struct BatchTxSource<TS> {
    sources: Vec<TS>,
    cursor: BatchCursor,
}

impl<TS> BatchTxSource<TS> {
    pub fn new(sources: Vec<TS>, cursor: BatchCursor) -> Self {
        Self { sources, cursor }
    }

    fn current(&self) -> usize {
        self.cursor.current()
    }
}

impl<TS: TxSource> TxSource for BatchTxSource<TS> {
    fn get_next_tx(&mut self) -> NextTxResponse {
        let idx = self.current();
        self.sources[idx].get_next_tx()
    }
}

#[derive(Debug)]
pub struct BatchPreimageSource<PS> {
    sources: Vec<PS>,
    cursor: BatchCursor,
}

impl<PS> BatchPreimageSource<PS> {
    pub fn new(sources: Vec<PS>, cursor: BatchCursor) -> Self {
        Self { sources, cursor }
    }

    fn current(&self) -> usize {
        self.cursor.current()
    }
}

impl<PS: PreimageSource> PreimageSource for BatchPreimageSource<PS> {
    fn get_preimage(&mut self, hash: zk_ee::utils::Bytes32) -> Option<Vec<u8>> {
        let idx = self.current();
        self.sources[idx].get_preimage(hash)
    }
}

#[derive(Debug)]
pub struct BatchTree<T> {
    trees: Vec<T>,
    cursor: BatchCursor,
}

impl<T> BatchTree<T> {
    pub fn new(trees: Vec<T>, cursor: BatchCursor) -> Self {
        Self { trees, cursor }
    }

    fn current(&self) -> usize {
        self.cursor.current()
    }
}

impl<T: ReadStorageTree> ReadStorage for BatchTree<T> {
    fn read(&mut self, key: zk_ee::utils::Bytes32) -> Option<zk_ee::utils::Bytes32> {
        let idx = self.current();
        self.trees[idx].read(key)
    }
}

impl<T: ReadStorageTree> ReadStorageTree for BatchTree<T> {
    fn tree_index(&mut self, key: zk_ee::utils::Bytes32) -> Option<u64> {
        let idx = self.current();
        self.trees[idx].tree_index(key)
    }

    fn merkle_proof(&mut self, tree_index: u64) -> super::LeafProof {
        let idx = self.current();
        self.trees[idx].merkle_proof(tree_index)
    }

    fn prev_tree_index(&mut self, key: zk_ee::utils::Bytes32) -> u64 {
        let idx = self.current();
        self.trees[idx].prev_tree_index(key)
    }
}

#[derive(Debug)]
pub struct BatchBlockMetadataResponder {
    block_metadata: Vec<BlockContext>,
    cursor: BatchCursor,
}

impl BatchBlockMetadataResponder {
    pub fn new(block_metadata: Vec<BlockContext>, cursor: BatchCursor) -> Self {
        Self {
            block_metadata,
            cursor,
        }
    }
}

impl<M: MemorySource> OracleQueryProcessor<M> for BatchBlockMetadataResponder {
    fn supported_query_ids(&self) -> Vec<u32> {
        vec![BLOCK_METADATA_QUERY_ID]
    }

    fn supports_query_id(&self, query_id: u32) -> bool {
        query_id == BLOCK_METADATA_QUERY_ID
    }

    fn process_buffered_query(
        &mut self,
        query_id: u32,
        _query: Vec<usize>,
        _memory: &M,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static + Send + Sync> {
        assert_eq!(query_id, BLOCK_METADATA_QUERY_ID);
        let block_metadata = self.block_metadata[self.cursor.current()];
        DynUsizeIterator::from_constructor(block_metadata, UsizeSerializable::iter)
    }
}

#[derive(Debug)]
pub struct BatchZKProofDataResponder {
    proof_data: Vec<ProofData<StorageCommitment>>,
    cursor: BatchCursor,
}

impl BatchZKProofDataResponder {
    pub fn new(proof_data: Vec<ProofData<StorageCommitment>>, cursor: BatchCursor) -> Self {
        Self { proof_data, cursor }
    }
}

impl<M: MemorySource> OracleQueryProcessor<M> for BatchZKProofDataResponder {
    fn supported_query_ids(&self) -> Vec<u32> {
        vec![ZKProofDataQuery::<
            zk_ee::types_config::EthereumIOTypesConfig,
            StorageCommitment,
        >::QUERY_ID]
    }

    fn supports_query_id(&self, query_id: u32) -> bool {
        query_id
            == ZKProofDataQuery::<
                zk_ee::types_config::EthereumIOTypesConfig,
                StorageCommitment,
            >::QUERY_ID
    }

    fn process_buffered_query(
        &mut self,
        query_id: u32,
        _query: Vec<usize>,
        _memory: &M,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static + Send + Sync> {
        assert_eq!(
            query_id,
            ZKProofDataQuery::<zk_ee::types_config::EthereumIOTypesConfig, StorageCommitment>::QUERY_ID
        );
        DynUsizeIterator::from_constructor(
            self.proof_data[self.cursor.current()],
            UsizeSerializable::iter,
        )
    }
}
