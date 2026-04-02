use alloy::consensus::{EthereumTxEnvelope, SignableTransaction};
use alloy::consensus::{Signed, TxEnvelope, TypedTransaction};
use alloy::dyn_abi::DynSolValue;
use alloy::network::TxSignerSync;
use alloy::primitives::Address;
use alloy::primitives::Signature;
use alloy::primitives::B256;
use alloy_rlp::{encode, BufMut, Encodable};
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
use zk_ee::utils::u256_try_to_u64;
use zk_ee::utils::Bytes32;
use zksync_os_interface::traits::EncodedTx;
use basic_bootloader::bootloader::constants::{
    L2_TX_INTRINSIC_NATIVE_COST, L2_TX_INTRINSIC_PUBDATA,
};
use basic_system::cost_constants::ECRECOVER_NATIVE_COST;
use zk_ee::common_structs::pubdata_compression::ValueDiffCompressionStrategy;
use basic_system::system_functions::keccak256::keccak256_native_cost_u64;
use basic_system::system_implementation::flat_storage_model::cost_constants::{
    blake2s_native_cost, COLD_EXISTING_STORAGE_READ_NATIVE_COST,
    COLD_NEW_STORAGE_READ_NATIVE_COST, PREIMAGE_CACHE_GET_NATIVE_COST,
    PREIMAGE_CACHE_SET_NATIVE_COST, WARM_ACCOUNT_CACHE_ACCESS_NATIVE_COST,
    WARM_ACCOUNT_CACHE_WRITE_EXTRA_NATIVE_COST, WARM_STORAGE_READ_NATIVE_COST,
};

use basic_bootloader::bootloader::constants::{
    PER_ADDRESS_ACCESS_LIST_NATIVE_COST, PER_AUTH_NATIVE_COST,
    PER_SLOT_ACCESS_LIST_NATIVE_COST,
};
use evm_interpreter::native_resource_constants::COPY_BYTE_NATIVE_COST;
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
pub struct ValidationNativeResourcesEstimate {
    /// Computational native resources consumed during validation.
    pub native_computational: u64,
    /// Pubdata bytes consumed during validation.
    pub pubdata: u64,
}

pub type ValidationNativeCostEstimate = ValidationNativeResourcesEstimate;

/// Estimates the native resources consumed during L2 transaction validation without executing
/// the actual validation logic.
///
/// `access_list_address_exists[i]` — whether the i-th access list address is already in the
/// storage tree (`true` = COLD_EXISTING read cost; `false` = COLD_NEW read cost).
/// `access_list_slot_exists[i]` — same semantics for storage slots.
/// `authorization_authority_exists[i]` — whether the i-th EIP-7702 authority account exists
/// in the tree (`true` = COLD_EXISTING + decommitment; `false` = COLD_NEW, no decommitment).
/// `fee_to_prepay` — the fee the sender will pay upfront (`gas_price × gas_limit`), used to
/// compute the exact balance-diff pubdata via compression estimation.
pub fn validation_native_resources(
    calldata_len: u64,
    tx_len: u64,
    access_list_address_exists: &[bool],
    access_list_slot_exists: &[bool],
    authorization_authority_exists: &[bool],
    sender_balance: U256,
    fee_to_prepay: U256,
) -> Result<ValidationNativeResourcesEstimate, ()> {
    // Cost of a cold existing account read: warm cache access + warm storage + cold existing extra
    //   + account data preimage decommitment.
    let account_decommitment_cost = PREIMAGE_CACHE_GET_NATIVE_COST
        .saturating_add(blake2s_native_cost(AccountProperties::ENCODED_SIZE));
    let cold_existing_account_read_cost = WARM_ACCOUNT_CACHE_ACCESS_NATIVE_COST
        .saturating_add(WARM_STORAGE_READ_NATIVE_COST)
        .saturating_add(COLD_EXISTING_STORAGE_READ_NATIVE_COST)
        .saturating_add(account_decommitment_cost);

    // Cost of a cold new (empty) account read: warm cache access + warm storage + cold new extra.
    // No decommitment since the account has no stored preimage.
    let cold_new_account_read_cost = WARM_ACCOUNT_CACHE_ACCESS_NATIVE_COST
        .saturating_add(WARM_STORAGE_READ_NATIVE_COST)
        .saturating_add(COLD_NEW_STORAGE_READ_NATIVE_COST);

    // Cost of a warm account write (nonce increment, balance update, delegation): warm access + write extra.
    let warm_account_write_cost = WARM_ACCOUNT_CACHE_ACCESS_NATIVE_COST
        .saturating_add(WARM_ACCOUNT_CACHE_WRITE_EXTRA_NATIVE_COST);

    // 1. Calldata: COPY_BYTE_NATIVE_COST per byte
    let mut native_computational: u64 = calldata_len.saturating_mul(COPY_BYTE_NATIVE_COST);
    // 2. L2 intrinsic native cost (fee transfer to coinbase, gas refund transfer, tx hash rolling hash)
    native_computational = native_computational.saturating_add(L2_TX_INTRINSIC_NATIVE_COST);
    // 3. Keccak for signing hash (overestimated using total tx length)
    native_computational =
        native_computational.saturating_add(keccak256_native_cost_u64(tx_len as usize));
    // 4. ECRecover for originator signature verification
    native_computational = native_computational.saturating_add(ECRECOVER_NATIVE_COST);
    // 5. Keccak for full tx hash
    native_computational =
        native_computational.saturating_add(keccak256_native_cost_u64(tx_len as usize));
    // 6. Cold originator account read (originator always exists — it holds the fee balance)
    native_computational =
        native_computational.saturating_add(cold_existing_account_read_cost);
    // 7. Originator nonce increment (account is warm after previous read)
    native_computational = native_computational.saturating_add(warm_account_write_cost);

    // 8. EIP-2930 access list:
    // Per address: base charge + cold account touch (materialize_element).
    for &exists in access_list_address_exists {
        let touch_cost = if exists {
            cold_existing_account_read_cost
        } else {
            cold_new_account_read_cost
        };
        native_computational = native_computational
            .saturating_add(PER_ADDRESS_ACCESS_LIST_NATIVE_COST)
            .saturating_add(touch_cost);
    }
    // Per slot: base charge + cold storage slot touch.
    for &exists in access_list_slot_exists {
        let slot_cost = WARM_STORAGE_READ_NATIVE_COST.saturating_add(if exists {
            COLD_EXISTING_STORAGE_READ_NATIVE_COST
        } else {
            COLD_NEW_STORAGE_READ_NATIVE_COST
        });
        native_computational = native_computational
            .saturating_add(PER_SLOT_ACCESS_LIST_NATIVE_COST)
            .saturating_add(slot_cost);
    }

    // 9. EIP-7702 authorization list.
    // Auth message is at most ~70 bytes (magic + rlp([chain_id, address, nonce])), fits in one keccak round.
    let auth_message_keccak_cost = keccak256_native_cost_u64(70);
    // Delegation code write: warm account access + keccak of 23-byte delegation code
    // + blake2s of padded code + preimage cache set + write extra.
    let delegation_code_len: usize = 23; // 0xef0100 || 20-byte address
    let delegation_padded_len = delegation_code_len + bytecode_padding_len(delegation_code_len);
    let delegation_write_cost = WARM_ACCOUNT_CACHE_ACCESS_NATIVE_COST
        .saturating_add(keccak256_native_cost_u64(delegation_code_len))
        .saturating_add(blake2s_native_cost(delegation_padded_len))
        .saturating_add(PREIMAGE_CACHE_SET_NATIVE_COST)
        .saturating_add(WARM_ACCOUNT_CACHE_WRITE_EXTRA_NATIVE_COST);
    for &exists in authorization_authority_exists {
        // Per entry: base charge + keccak of auth message + ecrecover + cold authority account
        //            read + nonce increment + delegation code write.
        let authority_read_cost = if exists {
            cold_existing_account_read_cost
        } else {
            cold_new_account_read_cost
        };
        native_computational = native_computational
            .saturating_add(PER_AUTH_NATIVE_COST)
            .saturating_add(auth_message_keccak_cost)
            .saturating_add(ECRECOVER_NATIVE_COST)
            .saturating_add(authority_read_cost)
            .saturating_add(warm_account_write_cost) // nonce increment
            .saturating_add(delegation_write_cost); // delegation code write
    }

    // 10. Fee prepayment balance deduction (originator account is warm at this point)
    native_computational = native_computational.saturating_add(warm_account_write_cost);

    // Pubdata: precise computation based on account properties diff compression.
    //
    // During validation, the sender's nonce increments by 1 (Add strategy → 2 bytes)
    // and balance decreases by fee_to_prepay (compressed with the optimal strategy).
    let new_balance = sender_balance.checked_sub(fee_to_prepay).expect("Balance < fee");
    // Sender account diff: key (32) + account metadata (1) + nonce diff (2) + optional balance diff
    let mut pubdata: u64 = 32 + 1 + 2;
    if sender_balance != new_balance {
        pubdata = pubdata.saturating_add(
            ValueDiffCompressionStrategy::optimal_compression_length_u256(
                sender_balance,
                new_balance,
            ) as u64,
        );
    }
    // Intrinsic pubdata (coinbase balance change) — pre-paid in create_resources_for_tx
    pubdata = pubdata.saturating_add(L2_TX_INTRINSIC_PUBDATA);
    // Per auth entry: full account diff (versioning_data + nonce + code fields + bytecode with padding).
    let auth_pubdata_per_entry: u64 = 32 /*key*/
        + 1 /*account diff metadata*/
        + 8 /*versioning_data*/
        + 2 /*nonce*/
        + 1 /*balance*/
        + 4 /*unpadded_code_len*/
        + 4 /*artifacts_len*/
        + delegation_padded_len as u64
        + 4 /*observable_len*/;
    pubdata = pubdata.saturating_add(
        auth_pubdata_per_entry
            .saturating_mul(authorization_authority_exists.len() as u64),
    );

    Ok(ValidationNativeResourcesEstimate {
        native_computational,
        pubdata,
    })
}

/// Computes the effective gas price for an L2 transaction, matching bootloader's `get_gas_price`.
/// When `base_fee` is zero, returns zero (no gas fees charged).
pub fn compute_l2_tx_gas_price(
    max_fee_per_gas: U256,
    max_priority_fee_per_gas: U256,
    base_fee: U256,
) -> U256 {
    if base_fee.is_zero() {
        U256::ZERO
    } else {
        let priority_fee =
            max_priority_fee_per_gas.min(max_fee_per_gas.saturating_sub(base_fee));
        base_fee + priority_fee
    }
}

/// Computes the L2 transaction validation native resources estimate and verifies it fits within the
/// available native resource budget derived from the transaction's gas parameters.
///
/// For the description of the `access_list_*` and `authorization_authority_exists` parameters,
/// see [`validation_native_cost`].
pub fn validate_l2_tx_native_resources(
    max_fee_per_gas: U256,
    max_priority_fee_per_gas: U256,
    gas_limit: u64,
    pubdata_price: U256,
    base_fee: U256,
    native_price: U256,
    calldata_len: u64,
    tx_len: u64,
    access_list_address_exists: &[bool],
    access_list_slot_exists: &[bool],
    authorization_authority_exists: &[bool],
    sender_balance: U256,
) -> Result<(), ()> {
    if native_price.is_zero() {
        return Err(());
    }

    let gas_price = compute_l2_tx_gas_price(max_fee_per_gas, max_priority_fee_per_gas, base_fee);

    let fee_to_prepay = gas_price
        .checked_mul(U256::from(gas_limit))
        .ok_or(())?;

    let estimate = validation_native_resources(
        calldata_len,
        tx_len,
        access_list_address_exists,
        access_list_slot_exists,
        authorization_authority_exists,
        sender_balance,
        fee_to_prepay,
    )?;

    // Match bootloader arithmetic exactly (see validate_and_compute_fee_for_transaction and
    // create_resources_for_tx):
    //   native_per_gas      = ceil(gas_price / native_price)  — saturating u64 multiply with gas_limit
    //   native_per_pubdata  = floor(pubdata_price / native_price)
    //   native_prepaid      = native_per_gas * gas_limit       — u64 saturating multiply
    let native_per_gas: u64 =
        u256_try_to_u64(&gas_price.div_ceil(native_price)).ok_or(())?;
    let native_per_pubdata: u64 =
        u256_try_to_u64(&(pubdata_price / native_price)).ok_or(())?;
    let native_prepaid_from_gas: u64 = native_per_gas.saturating_mul(gas_limit);

    // Subtract intrinsic pubdata overhead before the withheld split.
    let intrinsic_pubdata_overhead = native_per_pubdata.saturating_mul(L2_TX_INTRINSIC_PUBDATA);
    let native_after_intrinsic_pubdata = native_prepaid_from_gas
        .checked_sub(intrinsic_pubdata_overhead)
        .ok_or(())?;

    // Split at MAX_NATIVE_COMPUTATIONAL into main (computational) and withheld (pubdata-only) pools.
    let main = native_after_intrinsic_pubdata.min(MAX_NATIVE_COMPUTATIONAL);
    let withheld = native_after_intrinsic_pubdata - main;

    // Check 1: computational native must fit within main resources.
    if estimate.native_computational > main {
        return Err(());
    }

    // Check 2: validation pubdata cost must fit in withheld plus remaining main.
    let pubdata_native_cost = native_per_pubdata
        .checked_mul(estimate.pubdata)
        .ok_or(())?;
    let remaining_main = main - estimate.native_computational;
    if pubdata_native_cost > withheld.saturating_add(remaining_main) {
        return Err(());
    }

    Ok(())
}