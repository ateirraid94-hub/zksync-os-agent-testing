#![cfg(test)]

use rig::TestingFramework;
use rig::alloy::consensus::TxEip2930;
use rig::alloy::primitives::{Address, B256, TxKind, U256, address};
use rig::alloy::rpc::types::{AccessList, AccessListItem};
use rig::basic_system::system_implementation::flat_storage_model::{
    ExactIndexQuery, ExistingReadProof, PROOF_FOR_INDEX_QUERY_ID, PreviousIndexQuery,
    ValueAtIndexProof,
};
use rig::chain::TestingOracleFactory;
use rig::evm_bytecode;
use rig::forward_system::run::ReadStorageTree;
use rig::forward_system::run::convert_alloy::FromAlloy;
use rig::forward_system::run::query_processors::{
    BlockMetadataResponder, DACommitmentSchemeResponder, GenericPreimageResponder, TxDataResponder,
    ZKProofDataResponder,
};
use rig::forward_system::run::test_impl::{InMemoryPreimageSource, InMemoryTree};
use rig::oracle_provider::{MemorySource, OracleQueryProcessor, ZkEENonDeterminismSource};
use rig::ruint::aliases::B160;
use rig::testing_signer;
use rig::utils::{ACCOUNT_PROPERTIES_STORAGE_ADDRESS, address_into_special_storage_key};
use rig::zk_ee::common_structs::{
    ProofData, da_commitment_scheme::DACommitmentScheme, derive_flat_storage_key,
};
use rig::zk_ee::oracle::basic_queries::InitialStorageSlotQuery;
use rig::zk_ee::oracle::simple_oracle_query::SimpleOracleQuery;
use rig::zk_ee::oracle::usize_serialization::dyn_usize_iterator::DynUsizeIterator;
use rig::zk_ee::oracle::usize_serialization::{UsizeDeserializable, UsizeSerializable};
use rig::zk_ee::storage_types::{InitialStorageSlotData, StorageAddress};
use rig::zk_ee::system::metadata::zk_metadata::BlockMetadataFromOracle;
use rig::zk_ee::types_config::EthereumIOTypesConfig;
use rig::zk_ee::utils::Bytes32;
use rig::zksync_os_interface::traits::TxListSource;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;

#[derive(Clone, Debug)]
struct CountingStorageResponder<S: ReadStorageTree> {
    storage: S,
    counts: Arc<Mutex<HashMap<(B160, Bytes32), usize>>>,
}

impl<S: ReadStorageTree> CountingStorageResponder<S> {
    const SUPPORTED_QUERY_IDS: &[u32] = &[
        InitialStorageSlotQuery::<EthereumIOTypesConfig>::QUERY_ID,
        PreviousIndexQuery::QUERY_ID,
        ExactIndexQuery::QUERY_ID,
        PROOF_FOR_INDEX_QUERY_ID,
    ];
}

impl<S: ReadStorageTree, M: MemorySource> OracleQueryProcessor<M> for CountingStorageResponder<S> {
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
        _memory: &M,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static + Send + Sync> {
        match query_id {
            InitialStorageSlotQuery::<EthereumIOTypesConfig>::QUERY_ID => {
                let StorageAddress { address, key } = <InitialStorageSlotQuery<
                    EthereumIOTypesConfig,
                > as SimpleOracleQuery>::Input::from_iter(
                    &mut query.into_iter()
                )
                .expect("must deserialize storage address");

                if let Some(counter) = self.counts.lock().unwrap().get_mut(&(address, key)) {
                    *counter += 1;
                }

                let flat_key = derive_flat_storage_key(&address, &key);
                let slot_data: InitialStorageSlotData<EthereumIOTypesConfig> =
                    if let Some(value) = self.storage.read(flat_key) {
                        InitialStorageSlotData {
                            initial_value: value,
                            is_new_storage_slot: false,
                        }
                    } else {
                        InitialStorageSlotData {
                            initial_value: Bytes32::ZERO,
                            is_new_storage_slot: true,
                        }
                    };

                DynUsizeIterator::from_constructor(slot_data, UsizeSerializable::iter)
            }
            PreviousIndexQuery::QUERY_ID => {
                let key = <PreviousIndexQuery as SimpleOracleQuery>::Input::from_iter(
                    &mut query.into_iter(),
                )
                .expect("must deserialize key");
                let prev_index = self.storage.prev_tree_index(key);

                DynUsizeIterator::from_constructor(prev_index, UsizeSerializable::iter)
            }
            ExactIndexQuery::QUERY_ID => {
                let key = <ExactIndexQuery as SimpleOracleQuery>::Input::from_iter(
                    &mut query.into_iter(),
                )
                .expect("must deserialize key");
                let existing = self
                    .storage
                    .tree_index(key)
                    .expect("Reading index for key that is not in the tree");

                DynUsizeIterator::from_constructor(existing, UsizeSerializable::iter)
            }
            PROOF_FOR_INDEX_QUERY_ID => {
                let index = u64::from_iter(&mut query.into_iter()).expect("must deserialize index");
                let existing = self.storage.merkle_proof(index);
                let proof = ValueAtIndexProof {
                    proof: ExistingReadProof { existing },
                };

                DynUsizeIterator::from_constructor(proof, UsizeSerializable::iter)
            }
            _ => unreachable!(),
        }
    }
}

struct CountingOracleFactory {
    counts: Arc<Mutex<HashMap<(B160, Bytes32), usize>>>,
}

impl CountingOracleFactory {
    fn new(targets: Vec<(B160, Bytes32)>) -> (Self, Arc<Mutex<HashMap<(B160, Bytes32), usize>>>) {
        let counts = Arc::new(Mutex::new(
            targets.into_iter().map(|key| (key, 0usize)).collect(),
        ));

        (
            Self {
                counts: counts.clone(),
            },
            counts,
        )
    }

    fn build_oracle<M: MemorySource + 'static>(
        &self,
        block_metadata: BlockMetadataFromOracle,
        state_tree: InMemoryTree<false>,
        preimage_source: InMemoryPreimageSource,
        tx_source: TxListSource,
        proof_data: Option<
            ProofData<
                rig::basic_system::system_implementation::flat_storage_model::FlatStorageCommitment<
                    { rig::basic_system::system_implementation::flat_storage_model::TREE_HEIGHT },
                >,
            >,
        >,
        da_commitment_scheme: Option<DACommitmentScheme>,
        use_native_callable_oracles: bool,
    ) -> ZkEENonDeterminismSource<M> {
        let block_metadata_responder = BlockMetadataResponder { block_metadata };
        let tx_data_responder = TxDataResponder {
            tx_source,
            next_tx: None,
            next_tx_format: None,
            next_tx_from: None,
        };
        let preimage_responder = GenericPreimageResponder { preimage_source };
        let storage_responder = CountingStorageResponder {
            storage: state_tree,
            counts: self.counts.clone(),
        };
        let zk_proof_data_responder = ZKProofDataResponder { data: proof_data };
        let da_commitment_scheme_responder = DACommitmentSchemeResponder {
            da_commitment_scheme,
        };

        let mut oracle = ZkEENonDeterminismSource::default();
        oracle.add_external_processor(block_metadata_responder);
        oracle.add_external_processor(tx_data_responder);
        oracle.add_external_processor(preimage_responder);
        oracle.add_external_processor(storage_responder);
        oracle.add_external_processor(zk_proof_data_responder);
        oracle.add_external_processor(da_commitment_scheme_responder);
        // Keep custom test oracles aligned with the default rig oracle setup in proof mode.
        if use_native_callable_oracles {
            oracle.add_external_processor(
                rig::callable_oracles::arithmetic::NativeArithmeticQuery::default(),
            );
            oracle.add_external_processor(
                rig::callable_oracles::blob_kzg_commitment::NativeBlobCommitmentAndProofQuery::default(),
            );
            oracle.add_external_processor(
                rig::callable_oracles::field_hints::NativeFieldOpsQuery::default(),
            );
        } else {
            oracle.add_external_processor(
                rig::callable_oracles::arithmetic::ArithmeticQuery::default(),
            );
            oracle.add_external_processor(
                rig::callable_oracles::blob_kzg_commitment::BlobCommitmentAndProofQuery::default(),
            );
            oracle.add_external_processor(
                rig::callable_oracles::field_hints::FieldOpsQuery::default(),
            );
        }

        oracle
    }
}

impl TestingOracleFactory<false> for CountingOracleFactory {
    fn create_forward_oracle(
        &self,
        block_metadata: BlockMetadataFromOracle,
        state_tree: InMemoryTree<false>,
        preimage_source: InMemoryPreimageSource,
        tx_source: TxListSource,
        proof_data: Option<
            ProofData<
                rig::basic_system::system_implementation::flat_storage_model::FlatStorageCommitment<
                    { rig::basic_system::system_implementation::flat_storage_model::TREE_HEIGHT },
                >,
            >,
        >,
        da_commitment_scheme: Option<DACommitmentScheme>,
        _add_uart: bool,
        use_native_callable_oracles: bool,
    ) -> ZkEENonDeterminismSource<rig::oracle_provider::DummyMemorySource> {
        self.build_oracle(
            block_metadata,
            state_tree,
            preimage_source,
            tx_source,
            proof_data,
            da_commitment_scheme,
            use_native_callable_oracles,
        )
    }

    fn create_proof_oracle(
        &self,
        block_metadata: BlockMetadataFromOracle,
        state_tree: InMemoryTree<false>,
        preimage_source: InMemoryPreimageSource,
        tx_source: TxListSource,
        proof_data: Option<
            ProofData<
                rig::basic_system::system_implementation::flat_storage_model::FlatStorageCommitment<
                    { rig::basic_system::system_implementation::flat_storage_model::TREE_HEIGHT },
                >,
            >,
        >,
        da_commitment_scheme: Option<DACommitmentScheme>,
        _add_uart: bool,
        use_native_callable_oracles: bool,
    ) -> ZkEENonDeterminismSource<rig::risc_v_simulator::abstractions::memory::VectorMemoryImpl>
    {
        self.build_oracle(
            block_metadata,
            state_tree,
            preimage_source,
            tx_source,
            proof_data,
            da_commitment_scheme,
            use_native_callable_oracles,
        )
    }
}

fn storage_access_list_item(address: Address, slot: Bytes32) -> AccessList {
    AccessList::from(vec![AccessListItem {
        address,
        storage_keys: vec![B256::from(slot.as_u8_array())],
    }])
}

fn account_access_list_item(address: Address) -> AccessList {
    AccessList::from(vec![AccessListItem {
        address,
        storage_keys: vec![],
    }])
}

fn eip2930_call(
    signer: rig::alloy::signers::local::PrivateKeySigner,
    nonce: u64,
    to: Address,
    gas_limit: u64,
    access_list: AccessList,
) -> ZKsyncTxEnvelope {
    let tx = TxEip2930 {
        chain_id: 37u64,
        nonce,
        gas_price: 1000,
        gas_limit,
        to: TxKind::Call(to),
        value: U256::ZERO,
        input: Vec::new().into(),
        access_list,
    };
    ZKsyncTxEnvelope::from_eth_tx(tx, signer)
}

#[test]
fn access_list_touch_only_does_not_query_slot_or_change_pubdata() {
    let contract = address!("1000000000000000000000000000000000000001");
    let signer = testing_signer(0);
    let slot = Bytes32::ZERO;
    let target = (B160::from_alloy(contract), slot);
    let (oracle_factory, counts) = CountingOracleFactory::new(vec![target]);

    let tx_with_access_list = eip2930_call(
        signer.clone(),
        0,
        contract,
        30_000,
        storage_access_list_item(contract, slot),
    );
    let tx_without_access_list =
        eip2930_call(signer.clone(), 0, contract, 30_000, AccessList::default());

    let mut tester = TestingFramework::new()
        .with_prefunded_account(signer.address())
        .with_evm_contract(contract, &evm_bytecode::return_empty())
        .with_custom_oracle_factory(oracle_factory);
    let access_list_output = tester.execute_block(vec![tx_with_access_list]);
    tester.assert_all_txs_succeeded(&access_list_output);

    let mut control = TestingFramework::new()
        .with_prefunded_account(signer.address())
        .with_evm_contract(contract, &evm_bytecode::return_empty());
    let control_output = control.execute_block(vec![tx_without_access_list]);
    control.assert_all_txs_succeeded(&control_output);

    assert_eq!(counts.lock().unwrap()[&target], 0);
    assert_eq!(
        access_list_output.tx_results[0]
            .as_ref()
            .unwrap()
            .pubdata_used,
        control_output.tx_results[0].as_ref().unwrap().pubdata_used
    );
}

#[test]
fn access_list_account_touch_only_does_not_materialize_account_data() {
    let contract = address!("1000000000000000000000000000000000000002");
    let warmed_account = address!("2000000000000000000000000000000000000001");
    let signer = testing_signer(0);
    let target = (
        ACCOUNT_PROPERTIES_STORAGE_ADDRESS,
        address_into_special_storage_key(&B160::from_alloy(warmed_account)),
    );
    let (oracle_factory, counts) = CountingOracleFactory::new(vec![target]);

    let tx_with_access_list = eip2930_call(
        signer.clone(),
        0,
        contract,
        30_000,
        account_access_list_item(warmed_account),
    );
    let tx_without_access_list =
        eip2930_call(signer.clone(), 0, contract, 30_000, AccessList::default());

    let mut tester = TestingFramework::new()
        .with_prefunded_account(signer.address())
        .with_evm_contract(contract, &evm_bytecode::return_empty())
        .with_custom_oracle_factory(oracle_factory);
    let access_list_output = tester.execute_block(vec![tx_with_access_list]);
    tester.assert_all_txs_succeeded(&access_list_output);

    let mut control = TestingFramework::new()
        .with_prefunded_account(signer.address())
        .with_evm_contract(contract, &evm_bytecode::return_empty());
    let control_output = control.execute_block(vec![tx_without_access_list]);
    control.assert_all_txs_succeeded(&control_output);

    assert_eq!(counts.lock().unwrap()[&target], 0);
    assert_eq!(
        access_list_output.tx_results[0].as_ref().unwrap().gas_used
            - control_output.tx_results[0].as_ref().unwrap().gas_used,
        2_400
    );
}

#[test]
fn deploy_touch_and_deconstruct_does_not_materialize_touched_slot() {
    let signer = testing_signer(0);
    let created_address = signer.address().create(0);
    let slot = Bytes32::ZERO;
    let target = (B160::from_alloy(created_address), slot);
    let (oracle_factory, counts) = CountingOracleFactory::new(vec![target]);

    let init_code = {
        let beneficiary = address!("0000000000000000000000000000000000000001");
        evm_bytecode::selfdestruct(beneficiary)
    };

    let tx = {
        let tx = TxEip2930 {
            chain_id: 37u64,
            nonce: 0,
            gas_price: 1000,
            gas_limit: 120_000,
            to: TxKind::Create,
            value: U256::ZERO,
            input: init_code.into(),
            access_list: storage_access_list_item(created_address, slot),
        };
        ZKsyncTxEnvelope::from_eth_tx(tx, signer.clone())
    };

    let mut tester = TestingFramework::new()
        .with_prefunded_account(signer.address())
        .with_custom_oracle_factory(oracle_factory);
    let output = tester.execute_block(vec![tx]);
    tester.assert_all_txs_succeeded(&output);

    assert_eq!(counts.lock().unwrap()[&target], 0);
    let created = tester.get_account_properties(&created_address);
    assert_eq!(created.balance, U256::ZERO);
    assert_eq!(created.bytecode_hash, Bytes32::ZERO);
    assert_eq!(created.nonce, 0);
}
