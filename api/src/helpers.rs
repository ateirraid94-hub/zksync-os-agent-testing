use alloy::consensus::{EthereumTxEnvelope, SignableTransaction};
use alloy::consensus::{Signed, TxEnvelope, TypedTransaction};
use alloy::dyn_abi::DynSolValue;
use alloy::network::TxSignerSync;
use alloy::primitives::Address;
use alloy::primitives::Signature;
use alloy::primitives::B256;
use alloy::rlp::{encode, BufMut, Encodable};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::PrivateKeySigner;
use alloy_sol_types::sol;
use alloy_sol_types::SolCall;
use basic_bootloader::bootloader::constants::BOOTLOADER_FORMAL_ADDRESS;
use basic_bootloader::bootloader::transaction::rlp_encoded::transaction_types::service_tx::SERVICE_TX_TYPE;
use basic_system::system_implementation::flat_storage_model::bytecode_padding_len;
use basic_system::system_implementation::flat_storage_model::AccountProperties;
use forward_system::run::PreimageSource;
use ruint::aliases::U256;
use std::alloc::Global;
use zk_ee::common_structs::interop_root_storage::InteropRoot as StoredInteropRoot;
use zk_ee::execution_environment_type::ExecutionEnvironmentType;
use zk_ee::system::EIP7702_DELEGATION_MARKER;
use zk_ee::utils::Bytes32;
use zksync_os_interface::traits::EncodedTx;
use basic_bootloader::bootloader::constants::{
    L2_TX_INTRINSIC_NATIVE_COST, L2_TX_INTRINSIC_PUBDATA,
};
use basic_system::cost_constants::ECRECOVER_NATIVE_COST;
use basic_system::system_functions::keccak256::keccak256_native_cost_u64;
use basic_system::system_implementation::flat_storage_model::cost_constants::{
    blake2s_native_cost, COLD_EXISTING_STORAGE_READ_NATIVE_COST, PREIMAGE_CACHE_GET_NATIVE_COST,
    WARM_ACCOUNT_CACHE_ACCESS_NATIVE_COST, WARM_ACCOUNT_CACHE_WRITE_EXTRA_NATIVE_COST,
    WARM_STORAGE_READ_NATIVE_COST,
};

use basic_bootloader::bootloader::constants::{
    PER_ADDRESS_ACCESS_LIST_NATIVE_COST, PER_AUTH_NATIVE_COST,
    PER_SLOT_ACCESS_LIST_NATIVE_COST,
};
use zk_ee::system::MAX_NATIVE_COMPUTATIONAL;
// Getters

/// Retrieves balance from an account.
pub fn get_balance(account: &AccountProperties) -> U256 {
    account.balance
}

/// Retrieves nonce from an account.
pub fn get_nonce(account: &AccountProperties) -> u64 {
    account.nonce
}

/// Get unpadded code from full bytecode with artifacts.
pub fn get_unpadded_code<'a>(full_bytecode: &'a [u8], account: &AccountProperties) -> &'a [u8] {
    &full_bytecode[0..account.unpadded_code_len as usize]
}

/// Retrieves code for an account.
/// This function returns unpadded code, without artifacts.
pub fn get_code<P: PreimageSource>(
    preimage_source: &mut P,
    account: &AccountProperties,
) -> Vec<u8> {
    match preimage_source.get_preimage(account.bytecode_hash) {
        None => vec![],
        Some(full_bytecode) => get_unpadded_code(&full_bytecode, account).to_vec(),
    }
}

/// Computes the canonical ZKsync OS bytecode hash for EVM bytecode.
///
/// This follows the same path as account code installation, including
/// delegation marker handling and artifacts construction.
pub fn compute_evm_bytecode_hash(evm_code: &[u8]) -> B256 {
    let mut account = AccountProperties::default();
    let _ = set_properties_code(&mut account, evm_code);
    B256::from(account.bytecode_hash.as_u8_array())
}

/// Sets the balance for an account.
pub fn set_properties_balance(account: &mut AccountProperties, balance: U256) {
    account.balance = balance
}

/// Sets the nonce for an account.
pub fn set_properties_nonce(account: &mut AccountProperties, nonce: u64) {
    account.nonce = nonce
}

/// Sets a given [evm_code] for an [account].
/// Computes artifacts for [evm_code] and returns the extended
/// bytecode (code + artifacts).
pub fn set_properties_code(account: &mut AccountProperties, evm_code: &[u8]) -> Vec<u8> {
    use crypto::blake2s::Blake2s256;
    use crypto::sha3::Keccak256;
    use crypto::MiniDigest;

    let is_delegation = evm_code.len() >= 3 && evm_code[0..3] == EIP7702_DELEGATION_MARKER;

    let unpadded_code_len = evm_code.len();

    let observable_bytecode_hash = Bytes32::from_array(Keccak256::digest(evm_code));

    let (bytecode_hash, artifacts_len, full_bytecode) = if is_delegation {
        let artifacts_len = 0;
        let padding_len = bytecode_padding_len(unpadded_code_len);
        let full_len = unpadded_code_len + padding_len + artifacts_len;
        let mut padded_bytecode: Vec<u8> = vec![0u8; full_len];
        padded_bytecode[..unpadded_code_len].copy_from_slice(evm_code);
        let bytecode_hash = Bytes32::from_array(Blake2s256::digest(&padded_bytecode));

        account.versioning_data.set_as_delegated();

        (bytecode_hash, artifacts_len, padded_bytecode)
    } else {
        let artifacts =
            evm_interpreter::BytecodePreprocessingData::create_artifacts_inner(Global, evm_code);
        let artifacts = artifacts.as_slice();
        let artifacts_len = artifacts.len();
        let padding_len = bytecode_padding_len(unpadded_code_len);
        let full_len = unpadded_code_len + padding_len + artifacts_len;
        let mut bytecode_and_artifacts: Vec<u8> = vec![0u8; full_len];
        bytecode_and_artifacts[..unpadded_code_len].copy_from_slice(evm_code);
        let bitmap_offset = unpadded_code_len + padding_len;
        bytecode_and_artifacts[bitmap_offset..].copy_from_slice(artifacts);

        let bytecode_hash = Bytes32::from_array(Blake2s256::digest(&bytecode_and_artifacts));

        account
            .versioning_data
            .set_code_version(evm_interpreter::ARTIFACTS_CACHING_CODE_VERSION_BYTE);
        account.versioning_data.set_as_deployed();

        (bytecode_hash, artifacts_len, bytecode_and_artifacts)
    };

    account.observable_bytecode_hash = observable_bytecode_hash;
    account.bytecode_hash = bytecode_hash;
    account
        .versioning_data
        .set_ee_version(ExecutionEnvironmentType::EVM as u8);
    account.unpadded_code_len = unpadded_code_len as u32;
    account.artifacts_len = artifacts_len as u32;
    account.observable_bytecode_len = unpadded_code_len as u32;
    full_bytecode
}

///
/// Internal tx encoding method.
///
/// TODO: cleanup
///
#[allow(clippy::too_many_arguments)]
pub fn encode_tx(
    tx_type: u8,
    from: [u8; 20],
    to: Option<[u8; 20]>,
    gas_limit: u128,
    gas_per_pubdata_byte_limit: Option<u128>,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: Option<u128>,
    paymaster: Option<[u8; 20]>,
    nonce: u128,
    value: [u8; 32],
    data: Vec<u8>,
    signature: Vec<u8>,
    paymaster_input: Option<Vec<u8>>,
    reserved_dynamic: Option<Vec<u8>>,
    is_eip155: bool,
) -> EncodedTx {
    fn address_to_value(address: &[u8; 20]) -> DynSolValue {
        let mut padded = [0u8; 32];
        padded[12..].copy_from_slice(address.as_slice());
        U256::from_be_bytes(padded).into()
    }

    let bytes = DynSolValue::Tuple(vec![
        U256::from(tx_type).into(),
        address_to_value(&from),
        address_to_value(&to.unwrap_or_default()),
        U256::from(gas_limit).into(),
        gas_per_pubdata_byte_limit.unwrap_or_default().into(),
        max_fee_per_gas.into(),
        max_priority_fee_per_gas.unwrap_or(max_fee_per_gas).into(),
        address_to_value(&paymaster.unwrap_or_default()),
        U256::from(nonce).into(),
        U256::from_be_bytes(value).into(),
        DynSolValue::FixedArray(vec![
            (if tx_type == 0 {
                if is_eip155 {
                    U256::ONE
                } else {
                    U256::ZERO
                }
            } else if tx_type == 0x7f {
                U256::from(gas_limit * max_fee_per_gas)
            } else {
                U256::ZERO
            })
            .into(),
            (if to.is_none() { U256::ONE } else { U256::ZERO }).into(),
            U256::ZERO.into(),
            U256::ZERO.into(),
        ]),
        DynSolValue::Bytes(data),
        DynSolValue::Bytes(signature),
        // factory deps not supported for now
        DynSolValue::Array(vec![]),
        DynSolValue::Bytes(paymaster_input.unwrap_or_default()),
        DynSolValue::Bytes(reserved_dynamic.unwrap_or_default()),
    ])
    .abi_encode_params();
    EncodedTx::Abi(bytes)
}

///
/// Sign and encode alloy transaction using provided `wallet`.
///
pub fn sign_and_encode_alloy_tx<T>(mut tx: T, wallet: &PrivateKeySigner) -> EncodedTx
where
    T: SignableTransaction<Signature>,
    Signed<T>: Into<TxEnvelope>,
{
    let sig: Signature = wallet
        .sign_transaction_sync(&mut tx)
        .expect("transaction signing failed");
    let signed: Signed<T> = tx.into_signed(sig);
    let env: TxEnvelope = signed.into();
    let bytes = encode_envelope_2718(&env);
    EncodedTx::Rlp(bytes, wallet.address())
}

pub fn encode_envelope_2718(env: &TxEnvelope) -> Vec<u8> {
    let mut out = Vec::new();
    match env {
        EthereumTxEnvelope::Legacy(signed) => {
            signed.rlp_encode(&mut out);
        }
        EthereumTxEnvelope::Eip2930(signed) => {
            out.push(0x01);
            signed.rlp_encode(&mut out);
        }
        EthereumTxEnvelope::Eip1559(signed) => {
            out.push(0x02);
            signed.rlp_encode(&mut out);
        }
        EthereumTxEnvelope::Eip4844(signed) => {
            out.push(0x03);
            signed.rlp_encode(&mut out);
        }
        EthereumTxEnvelope::Eip7702(signed) => {
            out.push(0x04);
            signed.rlp_encode(&mut out);
        }
    }
    out
}

///
/// Sign and encode alloy transaction request using provided `wallet`.
///
pub fn sign_and_encode_transaction_request(
    req: TransactionRequest,
    wallet: &PrivateKeySigner,
) -> EncodedTx {
    let typed_tx = if req.blob_versioned_hashes.is_some() {
        req.build_4844_without_sidecar()
            .expect("Failed to build 4844 tx")
            .into()
    } else {
        req.build_typed_tx().expect("Failed to build typed tx")
    };
    match typed_tx {
        TypedTransaction::Legacy(tx) => sign_and_encode_alloy_tx(tx, wallet),
        TypedTransaction::Eip1559(tx) => sign_and_encode_alloy_tx(tx, wallet),
        TypedTransaction::Eip7702(tx) => sign_and_encode_alloy_tx(tx, wallet),
        TypedTransaction::Eip2930(tx) => sign_and_encode_alloy_tx(tx, wallet),
        TypedTransaction::Eip4844(tx) => sign_and_encode_alloy_tx(tx, wallet),
    }
}

/// Helper wrapper representing the RLP *body* of a service tx:
/// [to, data, salt]
struct ServiceTxBody<'a> {
    to: &'a [u8; 20],
    data: &'a [u8],
    salt: u64,
}

enum ServiceTxField<'b> {
    Bytes(&'b [u8]),
    U64(u64),
}

impl<'b> Encodable for ServiceTxField<'b> {
    fn encode(&self, out: &mut dyn BufMut) {
        match self {
            ServiceTxField::Bytes(b) => (*b).encode(out),
            ServiceTxField::U64(n) => n.encode(out),
        }
    }
}

impl<'a> Encodable for ServiceTxBody<'a> {
    fn encode(&self, out: &mut dyn BufMut) {
        let fields = vec![
            ServiceTxField::Bytes(self.to.as_slice()),
            ServiceTxField::Bytes(self.data),
            ServiceTxField::U64(self.salt),
        ];

        fields.encode(out);
    }
}

///
/// Encode a service transaction
///
pub fn encode_service_tx(to: &[u8; 20], data: &[u8], salt: u64) -> EncodedTx {
    let body = ServiceTxBody { to, data, salt };
    let rlp_body = encode(&body);
    let mut out = Vec::with_capacity(1 + rlp_body.len());
    out.push(SERVICE_TX_TYPE);
    out.extend_from_slice(&rlp_body);
    let from = Address::from_slice(&BOOTLOADER_FORMAL_ADDRESS.to_be_bytes::<20>());
    EncodedTx::Rlp(out, from)
}

///
/// Calldata used by service transactions that import interop roots.
///
/// Constructs the calldata for:
///
/// function addInteropRootsInBatch(InteropRoot[] calldata interopRootsInput);
///
/// where
///
/// struct InteropRoot {
///     uint256 chainId;
///     uint256 blockOrBatchNumber;
///     bytes32[] sides;
/// }
///
pub fn encode_interop_root_import_calldata(interop_roots: Vec<StoredInteropRoot>) -> Vec<u8> {
    // Declare sol interface
    sol! {
      struct InteropRoot {
          uint256 chainId;
          uint256 blockOrBatchNumber;
          bytes32[] sides;
      }

      function addInteropRootsInBatch(InteropRoot[] calldata interopRootsInput);
    }

    // Construct calldata
    let interop_roots: Vec<InteropRoot> = interop_roots
        .into_iter()
        .map(|r: StoredInteropRoot| {
            let root_b256 = alloy::primitives::B256::from_slice(r.root.as_u8_ref());
            InteropRoot {
                chainId: r.chain_id,
                blockOrBatchNumber: r.block_or_batch_number,
                sides: vec![root_b256],
            }
        })
        .collect();
    addInteropRootsInBatchCall {
        interopRootsInput: interop_roots,
    }
    .abi_encode()
}

///
/// Calldata used by service transactions that update the settlement layer chain id.
///
/// Constructs the calldata for:
///
/// function setSettlementLayerChainId(uint256 _newSettlementLayerChainId);
///
pub fn encode_set_settlement_layer_chain_id_calldata(new_sl_chain_id: U256) -> Vec<u8> {
    // Declare sol interface
    sol! {
       function setSettlementLayerChainId(uint256);
    }

    // Construct calldata
    setSettlementLayerChainIdCall(new_sl_chain_id).abi_encode()
}

/// Estimated native resource consumption during L2 transaction validation.
///
/// The bootloader splits the native budget into two pools:
/// - **main resources** (`≤ MAX_NATIVE_COMPUTATIONAL`): covers computational native costs.
/// - **withheld resources** (excess above `MAX_NATIVE_COMPUTATIONAL`): can only pay for pubdata.
///
/// Validation pubdata is charged from withheld first, then from main if withheld is exhausted.
/// Computational native is always charged from main only.
pub struct ValidationNativeCostEstimate {
    /// Computational native resources consumed during validation (excluding pubdata cost).
    /// Charged exclusively from main resources.
    pub native_computational: u64,
    /// Pubdata bytes consumed during validation.
    /// The corresponding native cost (`pubdata × (pubdata_price / native_price)`) is charged
    /// from withheld resources first, then from main resources if withheld is insufficient.
    pub pubdata: u64,
}

impl ValidationNativeCostEstimate {
    /// Returns the native cost for the pubdata portion.
    pub fn pubdata_native_cost(&self, pubdata_price: U256, native_price: U256) -> Option<U256> {
        if native_price.is_zero() {
            return None;
        }
        let native_per_pubdata = pubdata_price / native_price;
        native_per_pubdata.checked_mul(U256::from(self.pubdata))
    }
}

// TODO: may be unified with bootloader
/// Estimates the native resources consumed during L2 transaction validation, without executing
/// the actual validation logic.
///
/// Returns [`ValidationNativeCostEstimate`] with separate `native_computational` and `pubdata`
/// fields so callers can properly account for withheld resources (see module-level doc).
pub fn validation_native_cost(
    max_fee_per_gas: U256,
    max_priority_fee_per_gas: U256,
    gas_limit: U256,
    pubdata_price: U256,
    base_fee: U256,
    native_price: U256,
    calldata_len: u64,
    tx_len: u64,
    num_access_list_addresses: u64,
    num_access_list_slots: u64,
    num_authorization_list_entries: u64,
) -> Result<ValidationNativeCostEstimate, ()> {
    if native_price.is_zero() {
        return Err(());
    }
    let priority_fee =
        std::cmp::min(max_priority_fee_per_gas, max_fee_per_gas.saturating_sub(base_fee));
    let gas_price = base_fee + priority_fee;

    let native_per_pubdata = pubdata_price / native_price;
    let native_limit = gas_price * gas_limit / native_price;
    let mut withheld = 0;
    if native_limit > MAX_NATIVE_COMPUTATIONAL {
        withheld = native_limit - MAX_NATIVE_COMPUTATIONAL;
        native_limit = MAX_NATIVE_COMPUTATIONAL;
    }

    // Cost of a cold account read: warm access + cold extra + account data preimage decommitment.
    // Used for the originator, each access list address, and each auth entry's authority.
    let account_decommitment_cost = PREIMAGE_CACHE_GET_NATIVE_COST
        .saturating_add(blake2s_native_cost(AccountProperties::ENCODED_SIZE));
    let cold_account_read_cost = WARM_ACCOUNT_CACHE_ACCESS_NATIVE_COST
        .saturating_add(COLD_EXISTING_STORAGE_READ_NATIVE_COST)
        .saturating_add(account_decommitment_cost);

    // Cost of a warm account write (nonce increment, balance update, delegation): warm access + write extra.
    let warm_account_write_cost = WARM_ACCOUNT_CACHE_ACCESS_NATIVE_COST
        .saturating_add(WARM_ACCOUNT_CACHE_WRITE_EXTRA_NATIVE_COST);

    // Calldata: 1 native per byte (COPY_BYTE_NATIVE_COST = 1)
    let mut native_computational: u64 = calldata_len;
    // L2 intrinsic native cost (fee transfer to coinbase, gas refund transfer, tx hash rolling hash)
    native_computational = native_computational.saturating_add(L2_TX_INTRINSIC_NATIVE_COST);
    // Keccak for signing hash (overestimated using total tx length)
    native_computational =
        native_computational.saturating_add(keccak256_native_cost_u64(tx_len as usize));
    // ECRecover for originator signature verification
    native_computational = native_computational.saturating_add(ECRECOVER_NATIVE_COST);
    // Keccak for full tx hash
    native_computational =
        native_computational.saturating_add(keccak256_native_cost_u64(tx_len as usize));
    // Cold originator account read
    native_computational = native_computational.saturating_add(cold_account_read_cost);
    // Originator nonce increment (account is warm after previous read)
    native_computational = native_computational.saturating_add(warm_account_write_cost);

    // EIP-2930 access list:
    // Per address: base charge + cold account touch (materialize_element, same cost as a cold read).
    let access_list_address_cost =
        PER_ADDRESS_ACCESS_LIST_NATIVE_COST.saturating_add(cold_account_read_cost);
    native_computational = native_computational
        .saturating_add(access_list_address_cost.saturating_mul(num_access_list_addresses));
    // Per slot: base charge + cold storage slot touch.
    let access_list_slot_cost = PER_SLOT_ACCESS_LIST_NATIVE_COST
        .saturating_add(WARM_STORAGE_READ_NATIVE_COST)
        .saturating_add(COLD_EXISTING_STORAGE_READ_NATIVE_COST);
    native_computational = native_computational
        .saturating_add(access_list_slot_cost.saturating_mul(num_access_list_slots));

    // EIP-7702 authorization list:
    // Per entry: base charge + keccak of auth message + ecrecover + cold authority account read
    //            + nonce increment + delegation code write (both warm writes on authority account).
    // Auth message is at most ~70 bytes (magic + rlp([chain_id, address, nonce])), fits in one keccak round.
    let auth_message_keccak_cost = keccak256_native_cost_u64(70);
    let per_auth_cost = PER_AUTH_NATIVE_COST
        .saturating_add(auth_message_keccak_cost)
        .saturating_add(ECRECOVER_NATIVE_COST)
        .saturating_add(cold_account_read_cost) // authority account read
        .saturating_add(warm_account_write_cost) // authority nonce increment
        .saturating_add(warm_account_write_cost); // delegation code write
    native_computational = native_computational
        .saturating_add(per_auth_cost.saturating_mul(num_authorization_list_entries));

    // Fee prepayment balance deduction (originator account is warm at this point)
    native_computational = native_computational.saturating_add(warm_account_write_cost);

    // Pubdata estimation (worst case: key 32 bytes + compressed value diff 34 bytes per state change):
    // - Intrinsic: coinbase balance change
    // - Sender nonce increment
    // - Sender balance deduction (fee prepayment)
    // - Per auth entry: authority nonce increment + authority delegation code write
    let mut pubdata: u64 = L2_TX_INTRINSIC_PUBDATA
        .saturating_add(32 + 34) // sender nonce change
        .saturating_add(32 + 34); // sender balance change (fee prepayment)
    pubdata = pubdata.saturating_add(
        (32 + 34 + 32 + 34).saturating_mul(num_authorization_list_entries), // nonce + delegation per auth
    );

    Ok(ValidationNativeCostEstimate {
        native_computational,
        pubdata,
    })
}