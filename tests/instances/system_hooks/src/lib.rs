//!
//! These tests are focused on system hooks functionality.
//!
#![cfg(test)]

use alloy::primitives::TxKind;
use rig::alloy::primitives::address;
use rig::alloy::rpc::types::TransactionRequest;
use rig::ruint::aliases::B160;
use rig::ruint::aliases::U256;
use rig::testing_utils::call_address_and_measure_gas_cost;
use rig::testing_utils::install_system_contracts;
use rig::utils::{
    address_into_special_storage_key, AccountProperties, ACCOUNT_PROPERTIES_STORAGE_ADDRESS,
};
use rig::zk_ee::utils::Bytes32;
use rig::zksync_os_interface::types::ExecutionResult;
use rig::{alloy, Chain};

#[test]
fn test_set_bytecode_details_evm() {
    let mut chain = Chain::empty(None);

    let contract_deployer_address = address!("0000000000000000000000000000000000008006");
    let contract_deployer_hook_address = address!("0000000000000000000000000000000000007002");

    let bytecode = hex::decode("0123456789").unwrap();
    let code_hash = Bytes32::from_array(
        hex::decode("1c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7")
            .unwrap()
            .try_into()
            .unwrap(),
    );
    chain.set_preimage(code_hash, &bytecode);
    let calldata =
        hex::decode("00000000000000000000000000000000000000000000000000000000000100021c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7000000000000000000000000000000000000000000000000000000000000000579fad56e6cf52d0c8c2c033d568fc36856ba2b556774960968d79274b0e6b944")
            .unwrap();

    chain.set_balance(
        B160::from_be_bytes(contract_deployer_address.into_array()),
        U256::from(1_000_000_000_000_000_u64),
    );

    let encoded_tx = {
        let tx = TransactionRequest {
            chain_id: Some(37),
            from: Some(contract_deployer_address),
            to: Some(TxKind::Call(contract_deployer_hook_address)),
            input: calldata.into(),
            gas: Some(200_000),
            max_fee_per_gas: Some(1000),
            max_priority_fee_per_gas: Some(1000),
            value: Some(alloy::primitives::U256::from(0)),
            nonce: Some(0),
            ..TransactionRequest::default()
        };
        rig::utils::encode_l1_tx(tx)
    };
    let transactions = vec![encoded_tx];

    let output = chain.run_block(transactions, None, None, None);

    // Assert all txs succeeded
    assert!(output.tx_results.iter().cloned().enumerate().all(|(i, r)| {
        let success = r.clone().is_ok_and(|o| o.is_success());
        if !success {
            println!("Transaction {} failed with: {:?}", i, r)
        }
        success
    }));

    let mut account = AccountProperties::default();
    rig::zksync_os_api::helpers::set_properties_code(&mut account, &[0x01, 0x23, 0x45, 0x67, 0x89]);
    let expected_account_hash = account.compute_hash();

    let actual_hash = output
        .storage_writes
        .iter()
        .find(|write| {
            write.account.0 == ACCOUNT_PROPERTIES_STORAGE_ADDRESS.to_be_bytes()
                && write.account_key.0
                    == address_into_special_storage_key(&B160::from_limbs([0x10002, 0, 0]))
                        .as_u8_array()
        })
        .expect("Corresponding write for force deploy not found")
        .value;

    assert_eq!(expected_account_hash.as_u8_array(), actual_hash.0);
}

#[test]
fn test_set_deployed_bytecode_evm_unauthorized() {
    let mut chain = Chain::empty(None);
    install_system_contracts(&mut chain, false, false, true);

    let bytecode = hex::decode("0123456789").unwrap();
    let code_hash = Bytes32::from_array(
        hex::decode("1c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7")
            .unwrap()
            .try_into()
            .unwrap(),
    );
    chain.set_preimage(code_hash, &bytecode);

    let from = address!("000000000000000000000000000000000000800e");
    let contract_deployer_address = address!("0000000000000000000000000000000000008006");
    let calldata =
        hex::decode("231b395700000000000000000000000000000000000000000000000000000000000100021c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7000000000000000000000000000000000000000000000000000000000000000579fad56e6cf52d0c8c2c033d568fc36856ba2b556774960968d79274b0e6b9440000000000000000000000000000000000000000000000000000000000000005")
            .unwrap();

    let encoded_tx = {
        let tx = TransactionRequest {
            chain_id: Some(37),
            from: Some(from),
            to: Some(TxKind::Call(contract_deployer_address)),
            input: calldata.into(),
            gas: Some(200_000),
            max_fee_per_gas: Some(1000),
            max_priority_fee_per_gas: Some(1000),
            value: Some(alloy::primitives::U256::from(0)),
            nonce: Some(0),
            ..TransactionRequest::default()
        };
        rig::utils::encode_l1_tx(tx)
    };
    let transactions = vec![encoded_tx];

    let output = chain.run_block(transactions, None, None, None);

    let result = &output
        .tx_results
        .first()
        .unwrap()
        .as_ref()
        .unwrap()
        .execution_result;

    assert!(matches!(result, ExecutionResult::Revert(_)));
}

#[test]
fn test_l1_messenger_hook_succeeds() {
    let mut chain = Chain::empty(None);
    // making sure hooks are installed
    install_system_contracts(&mut chain, false, false, true);

    let l1_messenger_contract = address!("0000000000000000000000000000000000008008");

    let l1_messenger_hook = address!("0000000000000000000000000000000000007001");

    // Calldata that the hook *expects*:
    // abi.encode(msg.sender, _message)
    let hook_calldata = hex::decode(
        "000000000000000000000000111111111111111111111111111111111111111100000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000020000000000000000000000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    .unwrap();

    let tx = TransactionRequest {
        chain_id: Some(37),
        from: Some(l1_messenger_contract),
        to: Some(TxKind::Call(l1_messenger_hook)),
        input: hook_calldata.into(),
        gas: Some(200_000),
        max_fee_per_gas: Some(1000),
        max_priority_fee_per_gas: Some(1000),
        value: Some(alloy::primitives::U256::from(0)),
        nonce: Some(0),
        ..TransactionRequest::default()
    };

    let encoded_tx = rig::utils::encode_l1_tx(tx);
    let transactions = vec![encoded_tx];

    let output = chain.run_block(transactions, None, None, None);

    let tx_result = &output
        .tx_results
        .first()
        .unwrap()
        .as_ref()
        .unwrap()
        .execution_result;

    match tx_result {
        ExecutionResult::Success(_) => {
            // ok
        }
        _ => {
            panic!("L1 messenger hook call from authorized sender did not succeed: {tx_result:?}");
        }
    }
}

#[test]
fn test_l1_messenger_hook_fails_with_invalid_calldata() {
    let mut chain = Chain::empty(None);
    // making sure hooks are installed
    install_system_contracts(&mut chain, false, false, true);

    let l1_messenger_contract = address!("0000000000000000000000000000000000008008");

    let l1_messenger_hook = address!("0000000000000000000000000000000000007001");

    // Invalid calldata
    let hook_calldata = hex::decode("00000000000000000000000011111111").unwrap();

    let tx = TransactionRequest {
        chain_id: Some(37),
        from: Some(l1_messenger_contract),
        to: Some(TxKind::Call(l1_messenger_hook)),
        input: hook_calldata.into(),
        gas: Some(200_000),
        max_fee_per_gas: Some(1000),
        max_priority_fee_per_gas: Some(1000),
        value: Some(alloy::primitives::U256::from(0)),
        nonce: Some(0),
        ..TransactionRequest::default()
    };

    let encoded_tx = rig::utils::encode_l1_tx(tx);
    let transactions = vec![encoded_tx];

    let output = chain.run_block(transactions, None, None, None);

    let tx_result = &output
        .tx_results
        .first()
        .unwrap()
        .as_ref()
        .unwrap()
        .execution_result;

    assert!(matches!(tx_result, ExecutionResult::Revert { .. }));
}

#[test]
fn test_l1_messenger_hook_unauthorized_sender_ignored() {
    let mut chain = Chain::empty(None);
    // making sure hooks are installed
    install_system_contracts(&mut chain, false, false, true);

    // ❌ this should NOT be the L1Messenger system contract address
    let unauthorized_from = address!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    let l1_messenger_hook = address!("0000000000000000000000000000000000007001");

    // Calldata that the hook *expects*:
    // abi.encode(msg.sender, _message)
    // For the unauthorized test we don't care about the message contents,
    // we just want msg.sender (on the hook side) to be wrong (EOA instead of system contract).
    let hook_calldata = hex::decode(
    "000000000000000000000000111111111111111111111111111111111111111100000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000020000000000000000000000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    )
    .unwrap();

    let tx = TransactionRequest {
        chain_id: Some(37),
        from: Some(unauthorized_from),
        to: Some(TxKind::Call(l1_messenger_hook)),
        input: hook_calldata.into(),
        gas: Some(200_000),
        max_fee_per_gas: Some(1000),
        max_priority_fee_per_gas: Some(1000),
        value: Some(alloy::primitives::U256::from(0)),
        nonce: Some(0),
        ..TransactionRequest::default()
    };

    let encoded_tx = rig::utils::encode_l1_tx(tx);
    let transactions = vec![encoded_tx];

    let output = chain.run_block(transactions, None, None, None);

    let tx_result = &output
        .tx_results
        .first()
        .unwrap()
        .as_ref()
        .unwrap()
        .execution_result;

    assert!(matches!(tx_result, ExecutionResult::Success { .. }));
}

#[test]
fn test_contract_deployer_gas_charging() {
    let contract_deployer_address = address!("0000000000000000000000000000000000008006");
    let contract_deployer_hook_address = address!("0000000000000000000000000000000000007002");

    let bytecode = hex::decode("0123456789").unwrap();
    let code_hash = Bytes32::from_array(
        hex::decode("1c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7")
            .unwrap()
            .try_into()
            .unwrap(),
    );
    let calldata =
        hex::decode("00000000000000000000000000000000000000000000000000000000000100021c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7000000000000000000000000000000000000000000000000000000000000000579fad56e6cf52d0c8c2c033d568fc36856ba2b556774960968d79274b0e6b944")
            .unwrap();

    let gas_used = call_address_and_measure_gas_cost(
        contract_deployer_hook_address,
        contract_deployer_address,
        0,
        calldata,
        vec![(code_hash, bytecode)],
    );

    // The hook should charge HOOK_BASE_ERGS_COST (100 gas) + extra for bytecode length
    assert_eq!(gas_used, 2950);
}

#[test]
fn test_l1_messenger_gas_charging() {
    let l1_messenger_address = address!("0000000000000000000000000000000000008008");
    let sender = address!("1234567890123456789012345678901234567890");

    // sendToL1(bytes) - 62f84b24
    let message = b"test message to L1";
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&hex::decode("62f84b24").unwrap()); // sendToL1 selector
    calldata.extend_from_slice(&[0u8; 31]); // offset padding
    calldata.push(0x20); // offset to data (32 bytes)
    calldata.extend_from_slice(&[0u8; 31]); // length padding
    calldata.push(message.len() as u8); // message length
    calldata.extend_from_slice(message); // message data
                                         // Pad to 32 byte boundary
    let padding_needed = 32 - (message.len() % 32);
    if padding_needed != 32 {
        calldata.extend_from_slice(&vec![0u8; padding_needed]);
    }

    let gas_used =
        call_address_and_measure_gas_cost(l1_messenger_address, sender, 0, calldata, vec![]);

    // Verify that gas was charged - this should include the hook gas cost + keccak + LOG costs
    // The hook should charge keccak256 costs + LOG costs
    assert_eq!(gas_used, 9238);
}

#[test]
fn test_mint_base_token_hook() {
    let mut chain = Chain::empty(None);

    chain.mint_tokens_to_treasury(); // to properly reflect the initial balance

    // L2 base token address is the only address allowed to call the mint hook
    let l2_base_token_address = address!("000000000000000000000000000000000000800a");
    // Mint hook address (0x7100)
    let mint_hook_address = address!("0000000000000000000000000000000000007100");
    let mint_amount = alloy::primitives::U256::from(3000000000000000000u64); // 3 ETH

    // Check initial balance of L2_BASE_TOKEN_ADDRESS is zero
    let initial_balance = chain
        .get_account_properties(&B160::from_be_bytes(l2_base_token_address.into_array()))
        .balance;

    // Prepare calldata: 32 bytes containing the mint amount as U256 big-endian
    let calldata = mint_amount.to_be_bytes::<32>().to_vec();

    // Create transaction from L2_BASE_TOKEN_ADDRESS to MINT_HOOK_ADDRESS
    let tx = TransactionRequest {
        chain_id: Some(37),
        from: Some(l2_base_token_address),
        to: Some(TxKind::Call(mint_hook_address)),
        input: calldata.into(),
        value: Some(alloy::primitives::U256::ZERO), // No ETH value needed for mint
        gas: Some(200_000),
        max_fee_per_gas: Some(1000),
        max_priority_fee_per_gas: Some(1000),
        nonce: Some(0),
        ..TransactionRequest::default()
    };

    let encoded_tx = rig::utils::encode_l1_tx(tx);
    let transactions = vec![encoded_tx];

    let output = chain.run_block(transactions, None, None, None);

    // Assert transaction succeeded
    assert!(output.tx_results.iter().cloned().enumerate().all(|(i, r)| {
        let success = r.clone().is_ok_and(|o| o.is_success());
        if !success {
            println!("Transaction {} failed with: {:?}", i, r)
        }
        success
    }));

    // Check that the caller's (L2_BASE_TOKEN_ADDRESS) balance was increased by the mint amount
    let final_balance = chain
        .get_account_properties(&B160::from_be_bytes(l2_base_token_address.into_array()))
        .balance;

    let actually_minted_amount = final_balance
        .checked_sub(initial_balance)
        .expect("Some tokens should be minted");
    assert_eq!(
        actually_minted_amount, mint_amount,
        "Minted amount should match the requested mint amount"
    );
}
