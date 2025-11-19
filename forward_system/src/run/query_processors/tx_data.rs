use super::*;
use crate::run::NextTxResponse;
use crate::run::TxSource;
use zk_ee::oracle::ReadIterWrapper;
use zk_ee::system_io_oracle::{
    dyn_usize_iterator::DynUsizeIterator, NEXT_TX_SIZE_QUERY_ID, TX_DATA_WORDS_QUERY_ID,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxDataResponder<TS: TxSource> {
    pub tx_source: TS,
    pub next_tx: Option<Vec<u8>>,
}

impl<TS: TxSource> TxDataResponder<TS> {
    const SUPPORTED_QUERY_IDS: &[u32] = &[NEXT_TX_SIZE_QUERY_ID, TX_DATA_WORDS_QUERY_ID];
}

impl<TS: TxSource> OracleQueryProcessor for TxDataResponder<TS> {
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

        match query_id {
            NEXT_TX_SIZE_QUERY_ID => {
                let len = match &self.next_tx {
                    Some(next_tx) => next_tx.len(),
                    None => {
                        match self.tx_source.get_next_tx() {
                            NextTxResponse::SealBlock => 0,
                            NextTxResponse::Tx(next_tx) => {
                                let next_tx_len = next_tx.len();
                                // `0` interpreted as seal batch
                                assert_ne!(next_tx_len, 0);
                                self.next_tx = Some(next_tx);
                                next_tx_len
                            }
                        }
                    }
                } as u32;

                DynUsizeIterator::from_constructor(len, UsizeSerializable::iter)
            }
            TX_DATA_WORDS_QUERY_ID => {
                let Some(tx) = self.next_tx.take() else {
                    panic!(
                        "trying to read next tx content before size query or after seal response"
                    );
                };

                DynUsizeIterator::from_constructor(tx, |inner_ref| {
                    ReadIterWrapper::from(inner_ref.iter().copied())
                })
            }
            _ => unreachable!(),
        }
    }
}
