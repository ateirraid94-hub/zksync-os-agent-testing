use alloy::primitives::{address, Address, Bytes, U256};
use alloy::rpc::types::trace::geth::CallFrame;
use anyhow::{anyhow, bail, Context as AnyhowContext};
use forward_system::run::convert_alloy::IntoAlloy;
use revm::{
    context::{ContextTr, TxEnv},
    context_interface::block::BlobExcessGasAndPrice,
    database::{CacheDB, EmptyDB},
    inspector::InspectCommitEvm,
    DatabaseRef,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};
use zksync_os_interface::types::{BlockContext, BlockOutput};
use zksync_os_revm::{DefaultZk, ZKsyncTx, ZKsyncTxError, ZkBuilder, ZkContext, ZkSpecId};
use zksync_os_tests_common::zksync_tx::{ZKsyncSpecificTxEnvelope, ZKsyncTxEnvelope};

use crate::helpers::{
    calculate_excess_blob_gas_from_blob_base_fee, internal_service_call_into_revm_tx,
    zk_tx_into_revm_tx, BLOB_BASE_FEE_UPDATE_FRACTION,
};
use crate::revm_state_provider::{RevmStateProvider, ViewState};
use crate::storage_diff_comp::CompareReport;

const L2_BASE_TOKEN_ADDRESS: Address = address!("000000000000000000000000000000000000800a");
const L2_ASSET_TRACKER_ADDRESS: Address = address!("000000000000000000000000000000000001000f");
const HANDLE_FINALIZE_BASE_TOKEN_BRIDGING_ON_L2_SELECTOR: [u8; 4] = [0x03, 0x11, 0x7c, 0x8c];

struct ReplayTx {
    tx: ZKsyncTx<TxEnv>,
    include_trace: bool,
    original_tx_index: Option<usize>,
}

pub struct RevmRunner<State>
where
    State: ViewState,
{
    state: State,
    spec: ZkSpecId,
}

impl<State> RevmRunner<State>
where
    State: ViewState,
{
    pub fn new(state: State) -> Self {
        Self {
            state,
            spec: ZkSpecId::AtlasV3,
        }
    }

    pub fn with_spec(mut self, spec: ZkSpecId) -> Self {
        self.spec = spec;
        self
    }

    pub fn set_spec(&mut self, spec: ZkSpecId) -> &mut Self {
        self.spec = spec;
        self
    }

    pub fn run(
        &mut self,
        transactions: Vec<ZKsyncTxEnvelope>,
        block_context: BlockContext,
        block_output: Option<BlockOutput>,
    ) -> anyhow::Result<()> {
        let (_traces, _errors, compare_report) =
            self.run_with_call_traces(transactions, block_context, block_output)?;

        if let Some(report) = compare_report {
            if !report.is_empty() {
                log::warn!("State mismatch found after REVM replay");
                report.log_tracing(100);
                bail!(
                    "REVM consistency mismatch: storage={} account={}",
                    report.storage.len(),
                    report.accounts.len()
                );
            }
        }

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    pub fn run_with_call_traces(
        &mut self,
        transactions: Vec<ZKsyncTxEnvelope>,
        block_context: BlockContext,
        block_output: Option<BlockOutput>,
    ) -> anyhow::Result<(
        Vec<CallFrame>,
        Vec<(usize, ZKsyncTxError)>,
        Option<CompareReport>,
    )> {
        let blob_fee: u64 = block_context
            .blob_fee
            .try_into()
            .context("Blob fee should fit into u64")?;
        let block_basefee: u64 = block_context
            .eip1559_basefee
            .try_into()
            .context("Block base fee should fit into u64")?;

        let state_provider = RevmStateProvider::new(
            self.state.clone(),
            block_context.block_hashes,
            block_context.block_number.saturating_sub(1),
        );
        let settlement_layer_chain_id = Self::read_settlement_layer_chain_id(self.state.clone())?;

        let blob_excess_gas_and_price = BlobExcessGasAndPrice::new(
            calculate_excess_blob_gas_from_blob_base_fee(blob_fee, BLOB_BASE_FEE_UPDATE_FRACTION),
            BLOB_BASE_FEE_UPDATE_FRACTION
                .try_into()
                .expect("Blob base fee update fraction should fit into u64"),
        );

        let cache_db = CacheDB::new(state_provider);
        let mut evm = ZkContext::<EmptyDB>::default()
            .with_db(cache_db)
            .modify_cfg_chained(|cfg| {
                cfg.chain_id = block_context.chain_id;
                cfg.spec = self.spec;
            })
            .modify_block_chained(|block| {
                block.number = U256::from(block_context.block_number);
                block.timestamp = U256::from(block_context.timestamp);
                block.beneficiary = block_context.coinbase;
                block.basefee = block_basefee;
                block.gas_limit = block_context.gas_limit;
                block.prevrandao = Some(block_context.mix_hash.into());
                block.blob_excess_gas_and_price = Some(blob_excess_gas_and_price);
            })
            .build_zk_with_inspector(TracingInspector::new(TracingInspectorConfig::default_geth()));

        let revm_txs = Self::build_revm_txs(
            &transactions,
            block_output.as_ref(),
            block_context.gas_limit,
            settlement_layer_chain_id,
        )?;

        let mut call_traces = Vec::with_capacity(transactions.len());
        let mut invalid_transactions = vec![];
        for replay_tx in revm_txs {
            let tx_execution = match evm.inspect_tx_commit(replay_tx.tx) {
                Ok(res) => res,
                Err(err) => match err {
                    revm::context_interface::result::EVMError::Transaction(e) => {
                        if let Some(idx) = replay_tx.original_tx_index {
                            invalid_transactions.push((idx, e.clone()));
                            continue;
                        }
                        return Err(anyhow!(
                            "Synthetic bootloader replay tx failed validation: {:?}",
                            e
                        ));
                    }
                    revm::context_interface::result::EVMError::Header(e) => {
                        return Err(anyhow!("Header error: {:?}", e));
                    }
                    revm::context_interface::result::EVMError::Database(e) => {
                        return Err(anyhow!("Database error: {:?}", e));
                    }
                    revm::context_interface::result::EVMError::Custom(e) => {
                        return Err(anyhow!("Other error: {}", e));
                    }
                },
            };
            if replay_tx.include_trace {
                let trace = evm
                    .0
                    .inspector
                    .geth_builder()
                    .geth_call_traces(Default::default(), tx_execution.gas_used());
                call_traces.push(trace);
            }
            evm.0.inspector.fuse();
        }

        if block_output.is_some() && !invalid_transactions.is_empty() {
            for (idx, err) in invalid_transactions.iter().take(10) {
                log::info!(
                    "REVM rejected tx #{idx} (included in ZKsync OS block as failed): {err:?}"
                );
            }
        }

        let compare_report = if let Some(block_output) = block_output.as_ref() {
            Some(Self::build_compare_report(evm.0.db_mut(), block_output)?)
        } else {
            None
        };

        Ok((call_traces, invalid_transactions, compare_report))
    }

    fn build_revm_txs(
        transactions: &[ZKsyncTxEnvelope],
        block_output: Option<&BlockOutput>,
        block_gas_limit: u64,
        settlement_layer_chain_id: U256,
    ) -> anyhow::Result<Vec<ReplayTx>> {
        if let Some(block_output) = block_output {
            if transactions.len() != block_output.tx_results.len() {
                bail!(
                    "Transactions count ({}) does not match tx_results count ({})",
                    transactions.len(),
                    block_output.tx_results.len()
                );
            }

            let mut revm_txs = Vec::with_capacity(transactions.len());

            for (idx, (transaction, tx_output_raw)) in transactions
                .iter()
                .zip(&block_output.tx_results)
                .enumerate()
            {
                let Ok(tx_output) = tx_output_raw else {
                    log::debug!(
                        "Skipping tx #{idx} in REVM replay because ZKsync OS rejected it: {tx_output_raw:?}"
                    );
                    continue;
                };

                let tx_env = zk_tx_into_revm_tx(
                    transaction,
                    Some(tx_output.gas_used),
                    !tx_output.is_success(),
                    block_gas_limit,
                )
                .with_context(|| format!("Failed to convert tx #{idx} to REVM tx"))?;

                revm_txs.push(ReplayTx {
                    tx: tx_env,
                    include_trace: true,
                    original_tx_index: Some(idx),
                });
                Self::append_l1_post_processing_replay_txs(
                    &mut revm_txs,
                    transaction,
                    tx_output.gas_used,
                    tx_output.is_success(),
                    block_gas_limit,
                    settlement_layer_chain_id,
                )?;
            }

            Ok(revm_txs)
        } else {
            transactions
                .iter()
                .enumerate()
                .map(|(idx, transaction)| {
                    zk_tx_into_revm_tx(transaction, None, false, block_gas_limit)
                        .with_context(|| format!("Failed to convert tx #{idx} to REVM tx"))
                        .map(|tx| ReplayTx {
                            tx,
                            include_trace: true,
                            original_tx_index: Some(idx),
                        })
                })
                .collect()
        }
    }

    fn build_compare_report<DB>(
        cache_db: &mut CacheDB<DB>,
        block_output: &BlockOutput,
    ) -> anyhow::Result<CompareReport>
    where
        DB: DatabaseRef,
        DB::Error: std::error::Error + Send + Sync + 'static,
    {
        CompareReport::build(
            cache_db,
            &block_output.storage_writes,
            &block_output.account_diffs,
        )
    }

    fn read_settlement_layer_chain_id(mut state: State) -> anyhow::Result<U256> {
        let flat_key = zk_ee::common_structs::derive_flat_storage_key(
            &ruint::aliases::B160::from_limbs([0x800b, 0, 0]),
            &zk_ee::utils::Bytes32::ZERO,
        );
        Ok(U256::from_be_slice(
            state
                .read(flat_key.into_alloy())
                .unwrap_or_default()
                .as_slice(),
        ))
    }

    fn append_l1_post_processing_replay_txs(
        replay_txs: &mut Vec<ReplayTx>,
        transaction: &ZKsyncTxEnvelope,
        gas_used: u64,
        is_success: bool,
        block_gas_limit: u64,
        settlement_layer_chain_id: U256,
    ) -> anyhow::Result<()> {
        let ZKsyncTxEnvelope::ZKsync(ZKsyncSpecificTxEnvelope::L1(l1_tx)) = transaction else {
            return Ok(());
        };

        let gas_price = U256::from(l1_tx.max_fee_per_gas);
        let gas_limit = U256::from(l1_tx.gas_limit);
        let total_deposited = l1_tx.to_mint;
        let max_fee_commitment = gas_price
            .checked_mul(gas_limit)
            .ok_or_else(|| anyhow!("L1 max fee commitment overflow during REVM replay"))?;
        let to_transfer = total_deposited
            .checked_sub(max_fee_commitment)
            .ok_or_else(|| {
                anyhow!("L1 deposit smaller than max fee commitment during REVM replay")
            })?;
        let pay_to_operator = U256::from(gas_used)
            .checked_mul(gas_price)
            .ok_or_else(|| anyhow!("L1 operator payment overflow during REVM replay"))?;
        let refund = if is_success {
            max_fee_commitment
                .checked_sub(pay_to_operator)
                .ok_or_else(|| anyhow!("L1 successful refund underflow during REVM replay"))?
        } else {
            total_deposited
                .checked_sub(pay_to_operator)
                .ok_or_else(|| anyhow!("L1 reverted refund underflow during REVM replay"))?
        };

        if is_success && to_transfer > U256::ZERO {
            replay_txs.push(ReplayTx {
                tx: Self::build_asset_tracker_replay_tx(
                    settlement_layer_chain_id,
                    to_transfer,
                    block_gas_limit,
                )?,
                include_trace: false,
                original_tx_index: None,
            });
        }
        if pay_to_operator > U256::ZERO {
            replay_txs.push(ReplayTx {
                tx: Self::build_asset_tracker_replay_tx(
                    settlement_layer_chain_id,
                    pay_to_operator,
                    block_gas_limit,
                )?,
                include_trace: false,
                original_tx_index: None,
            });
        }
        if refund > U256::ZERO {
            replay_txs.push(ReplayTx {
                tx: Self::build_asset_tracker_replay_tx(
                    settlement_layer_chain_id,
                    refund,
                    block_gas_limit,
                )?,
                include_trace: false,
                original_tx_index: None,
            });
        }

        Ok(())
    }

    fn build_asset_tracker_replay_tx(
        settlement_layer_chain_id: U256,
        amount: U256,
        block_gas_limit: u64,
    ) -> anyhow::Result<ZKsyncTx<TxEnv>> {
        let mut calldata = [0u8; 68];
        calldata[..4].copy_from_slice(&HANDLE_FINALIZE_BASE_TOKEN_BRIDGING_ON_L2_SELECTOR);
        calldata[4..36].copy_from_slice(&settlement_layer_chain_id.to_be_bytes::<32>());
        calldata[36..68].copy_from_slice(&amount.to_be_bytes::<32>());

        internal_service_call_into_revm_tx(
            L2_BASE_TOKEN_ADDRESS,
            L2_ASSET_TRACKER_ADDRESS,
            Bytes::copy_from_slice(&calldata),
            block_gas_limit,
        )
    }
}
