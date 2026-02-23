//!
//! These tests are focused on different tx types.
//!
#![cfg(test)]

use alloy::primitives::TxKind;
use rig::alloy::primitives::address;
use rig::alloy::rpc::types::TransactionRequest;
use rig::ruint::aliases::B160;
use rig::testing_utils::call_address_and_measure_gas_cost;
use rig::utils::{
    address_into_special_storage_key, AccountProperties, ACCOUNT_PROPERTIES_STORAGE_ADDRESS,
};
use rig::zk_ee::utils::Bytes32;
use rig::zksync_os_interface::types::ExecutionResult;
use rig::{alloy, Chain};

#[test]
fn test_set_bytecode_details_evm() {
    let mut chain = Chain::empty(None);

    let complex_upgrader_address = address!("000000000000000000000000000000000000800f");
    let contract_deployer_address = address!("0000000000000000000000000000000000008006");
    // setBytecodeDetailsEVM(address,bytes32,uint32,bytes32) - f6eca0b0
    let bytecode = hex::decode("0123456789").unwrap();
    let code_hash = Bytes32::from_array(
        hex::decode("1c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7")
            .unwrap()
            .try_into()
            .unwrap(),
    );
    chain.set_preimage(code_hash, &bytecode);
    let calldata =
        hex::decode("f6eca0b000000000000000000000000000000000000000000000000000000000000100021c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7000000000000000000000000000000000000000000000000000000000000000579fad56e6cf52d0c8c2c033d568fc36856ba2b556774960968d79274b0e6b944")
            .unwrap();

    let encoded_tx = {
        let tx = TransactionRequest {
            chain_id: Some(37),
            from: Some(complex_upgrader_address),
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

    let from = address!("000000000000000000000000000000000000800e");
    let contract_deployer_address = address!("0000000000000000000000000000000000008006");
    let calldata =
        hex::decode("f6eca0b000000000000000000000000000000000000000000000000000000000000100021c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7000000000000000000000000000000000000000000000000000000000000000579fad56e6cf52d0c8c2c033d568fc36856ba2b556774960968d79274b0e6b944")
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

    if let ExecutionResult::Success(_) = output
        .tx_results
        .first()
        .unwrap()
        .as_ref()
        .unwrap()
        .execution_result
    {
        panic!("force deploy from unauthorized sender haven't failed")
    }
}

#[test]
fn test_contract_deployer_gas_charging() {
    let complex_upgrader_address = address!("000000000000000000000000000000000000800f");
    let contract_deployer_address = address!("0000000000000000000000000000000000008006");

    // setBytecodeDetailsEVM(address,bytes32,uint32,bytes32) - f6eca0b0
    let bytecode = hex::decode("0123456789").unwrap();
    let code_hash = Bytes32::from_array(
        hex::decode("1c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7")
            .unwrap()
            .try_into()
            .unwrap(),
    );
    let calldata =
        hex::decode("f6eca0b000000000000000000000000000000000000000000000000000000000000100021c4be3dec3ba88b69a8d3cd5cedd2b22f3da89b1ff9c8fd453c5a6e10c23d6f7000000000000000000000000000000000000000000000000000000000000000579fad56e6cf52d0c8c2c033d568fc36856ba2b556774960968d79274b0e6b944")
            .unwrap();

    let gas_used = call_address_and_measure_gas_cost(
        contract_deployer_address,
        complex_upgrader_address,
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
    // The hook should charge HOOK_BASE_ERGS_COST (100 gas) + keccak256 costs + LOG costs
    assert_eq!(gas_used, 3133);
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
