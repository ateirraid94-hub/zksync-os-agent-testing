//!
//! These tests are focused on interop support in ZKsync OS
//!
#![cfg(test)]

use alloy::consensus::TxLegacy;
use alloy::signers::local::PrivateKeySigner;
use rig::alloy::primitives::Address;
use rig::alloy_sol_types::sol;
use rig::alloy_sol_types::SolCall;
use rig::crypto::MiniDigest;
use rig::ruint::aliases::{B160, U256};
use rig::system_hooks::addresses_constants::L2_INTEROP_ROOT_STORAGE_ADDRESS;
use rig::zk_ee::common_structs::interop_root_storage::InteropRoot as StoredInteropRoot;
use rig::zk_ee::system::tracer::NopTracer;
use rig::zk_ee::utils::Bytes32;
use rig::zksync_os_interface::types::ExecutionOutput;
use rig::zksync_os_interface::types::ExecutionResult;
use rig::{alloy, zksync_web3_rs, BlockContext, Chain};
use std::str::FromStr;
use std::vec;
use zksync_web3_rs::signers::{LocalWallet, Signer};

const L2_INTEROP_ROOT_STORAGE_BYTECODE : &str = "608060405234801561000f575f5ffd5b5060043610610055575f3560e01c8063140e31cf146100595780633b43dbde1461007757806377cfd17114610093578063cca2f7bc146100c3578063fb6200c6146100df575b5f5ffd5b6100616100fb565b60405161006e9190610595565b60405180910390f35b610091600480360381019061008c91906105d8565b610101565b005b6100ad60048036038101906100a89190610649565b6101b4565b6040516100ba919061069f565b60405180910390f35b6100dd60048036038101906100d89190610719565b6101d3565b005b6100f960048036038101906100f491906107b9565b610288565b005b60015481565b4173ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff1614610166576040517f4db373cd00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b43600154036101a1576040517f074bb98e00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b436001819055506101b181610341565b50565b5f602052815f5260405f20602052805f5260405f205f91509150505481565b4173ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff1614610238576040517f4db373cd00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b4360015403610273576040517f074bb98e00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b436001819055506102848282610365565b5050565b4173ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff16146102ed576040517f4db373cd00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b4360015403610328576040517f074bb98e00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b4360018190555061033b84848484610414565b50505050565b610362815f0135826020013583806040019061035d9190610836565b610414565b50565b5f8282905090505f5b8181101561040e5761040384848381811061038c5761038b610898565b5b905060200281019061039e91906108c5565b5f01358585848181106103b4576103b3610898565b5b90506020028101906103c691906108c5565b602001358686858181106103dd576103dc610898565b5b90506020028101906103ef91906108c5565b80604001906103fe9190610836565b610414565b80600101905061036e565b50505050565b60018282905014610451576040517f2f59bd0d00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f5f1b82825f81811061046757610466610898565b5b90506020020135036104a5576040517f9b5f85eb00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f5f1b5f5f8681526020019081526020015f205f8581526020019081526020015f2054146104ff576040517f2d48e8cf00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b81815f81811061051257610511610898565b5b905060200201355f5f8681526020019081526020015f205f8581526020019081526020015f208190555082847f6b451b8422636e45b93bf7f594fa2c1769d039766c4254a6e7f9c0ee1715cdb0848460405161056f929190610964565b60405180910390a350505050565b5f819050919050565b61058f8161057d565b82525050565b5f6020820190506105a85f830184610586565b92915050565b5f5ffd5b5f5ffd5b5f5ffd5b5f606082840312156105cf576105ce6105b6565b5b81905092915050565b5f602082840312156105ed576105ec6105ae565b5b5f82013567ffffffffffffffff81111561060a576106096105b2565b5b610616848285016105ba565b91505092915050565b6106288161057d565b8114610632575f5ffd5b50565b5f813590506106438161061f565b92915050565b5f5f6040838503121561065f5761065e6105ae565b5b5f61066c85828601610635565b925050602061067d85828601610635565b9150509250929050565b5f819050919050565b61069981610687565b82525050565b5f6020820190506106b25f830184610690565b92915050565b5f5ffd5b5f5ffd5b5f5ffd5b5f5f83601f8401126106d9576106d86106b8565b5b8235905067ffffffffffffffff8111156106f6576106f56106bc565b5b602083019150836020820283011115610712576107116106c0565b5b9250929050565b5f5f6020838503121561072f5761072e6105ae565b5b5f83013567ffffffffffffffff81111561074c5761074b6105b2565b5b610758858286016106c4565b92509250509250929050565b5f5f83601f840112610779576107786106b8565b5b8235905067ffffffffffffffff811115610796576107956106bc565b5b6020830191508360208202830111156107b2576107b16106c0565b5b9250929050565b5f5f5f5f606085870312156107d1576107d06105ae565b5b5f6107de87828801610635565b94505060206107ef87828801610635565b935050604085013567ffffffffffffffff8111156108105761080f6105b2565b5b61081c87828801610764565b925092505092959194509250565b5f5ffd5b5f5ffd5b5f5ffd5b5f5f833560016020038436030381126108525761085161082a565b5b80840192508235915067ffffffffffffffff8211156108745761087361082e565b5b6020830192506020820236038313156108905761088f610832565b5b509250929050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52603260045260245ffd5b5f823560016060038336030381126108e0576108df61082a565b5b80830191505092915050565b5f82825260208201905092915050565b5f5ffd5b82818337505050565b5f61091483856108ec565b93507f07ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff831115610947576109466108fc565b5b602083029250610958838584610900565b82840190509392505050565b5f6020820190508181035f83015261097d818486610909565b9050939250505056fea26469706673582212202200ec0ac558d2d6222c23f842f6b471dbf903eef21119e8c2039d001292a64664736f6c634300081c0033";

#[test]
fn run_processes_one_interop_root() {
    // Create some dummy interop root
    let interop_roots = vec![StoredInteropRoot {
        root: Bytes32::from_u256_be(&U256::ONE),
        block_or_batch_number: U256::from(42),
        chain_id: U256::ONE,
    }];

    run_test(interop_roots);
}

#[test]
#[should_panic(expected = "Transaction should be successful")]
fn run_fails_if_interop_root_is_incorrect() {
    // Create some dummy interop root
    let interop_roots = vec![StoredInteropRoot {
        root: Bytes32::zero(), // Root can't be zero
        block_or_batch_number: U256::from(42),
        chain_id: U256::ONE,
    }];

    run_test(interop_roots);
}

#[test]
fn run_processes_several_interop_roots() {
    let mut interop_roots = Vec::new();
    // Create several interop roots to test batch processing and resource costs
    for i in 1..=20 {
        interop_roots.push(StoredInteropRoot {
            root: Bytes32::from_u256_be(&U256::from(0x1000 + i)),
            block_or_batch_number: U256::from(100 + i),
            chain_id: U256::from(i), // Use different chain IDs
        });
    }

    run_test(interop_roots);
}

#[test]
fn run_processes_empty_interop_roots() {
    run_test(vec![]);
}

#[test]
fn run_processes_interop_roots_max_amount() {
    let interop_roots = vec![
        // Edge case: Maximum values
        StoredInteropRoot {
            root: Bytes32::from_u256_be(&U256::MAX),
            block_or_batch_number: U256::MAX,
            chain_id: U256::MAX,
        },
        // Edge case: Minimum valid values (chain_id = 1, block = 0)
        StoredInteropRoot {
            root: Bytes32::from_u256_be(&U256::from(1)),
            block_or_batch_number: U256::ZERO,
            chain_id: U256::ONE,
        }, // Edge case: Large root hash with small numbers
        StoredInteropRoot {
            root: Bytes32::from_u256_be(&(U256::MAX - U256::from(1))),
            block_or_batch_number: U256::ONE,
            chain_id: U256::ONE,
        },
    ];

    run_test(interop_roots);
}

#[test]
#[should_panic(expected = "Transaction should be successful")]
fn run_fails_if_sender_is_not_coinbase() {
    // Create some dummy interop root
    let interop_roots = vec![StoredInteropRoot {
        root: Bytes32::zero(), // Root can't be zero
        block_or_batch_number: U256::from(42),
        chain_id: U256::ONE,
    }];

    run_test_inner(interop_roots, false);
}

fn run_test(interop_roots: Vec<StoredInteropRoot>) {
    run_test_inner(interop_roots, true)
}

/// Executes a transaction with specified interop roots and verifies success
fn run_test_inner(interop_roots: Vec<StoredInteropRoot>, sender_is_coinbase: bool) {
    let mut chain = Chain::empty(None);
    // We can't set interop roots for block 0
    chain.set_last_block_number(0);
    let wallet = PrivateKeySigner::from_str(
        "dcf2cbdd171a21c480aa7f53d77f31bb102282b3ff099c78e3118b37348c72f7",
    )
    .unwrap();
    let wallet_ethers = LocalWallet::from_bytes(wallet.to_bytes().as_slice()).unwrap();

    let from = wallet_ethers.address();

    chain.set_balance(
        B160::from_be_bytes(from.0),
        U256::from(1_000_000_000_000_000_u64),
    );

    let bytecode = hex::decode(L2_INTEROP_ROOT_STORAGE_BYTECODE).unwrap();
    chain.set_evm_bytecode(L2_INTEROP_ROOT_STORAGE_ADDRESS, &bytecode);

    // Declare sol interface
    sol! {
      struct InteropRoot {
          uint256 chainId;
          uint256 blockOrBatchNumber;
          bytes32[] sides;
      }

      function addInteropRootsInBatch(InteropRoot[] calldata interopRootsInput);
    }

    // Compute expected rolling hash
    let expected_rolling_hash = rig::basic_system::system_implementation::system::interop_roots::calculate_interop_roots_rolling_hash(
        Bytes32::ZERO,
        interop_roots.iter(),
        &mut rig::crypto::sha3::Keccak256::new(),
    );

    // Construct calldata
    let n_interop_roots = interop_roots.len();
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
    let calldata: Vec<u8> = addInteropRootsInBatchCall {
        interopRootsInput: interop_roots,
    }
    .abi_encode();

    let tx = rig::utils::sign_and_encode_alloy_tx(
        TxLegacy {
            chain_id: 37u64.into(),
            nonce: 0,
            gas_price: 1000,
            gas_limit: 50_000_000,
            to: rig::alloy::primitives::TxKind::Call(Address::from_slice(
                &L2_INTEROP_ROOT_STORAGE_ADDRESS.to_be_bytes::<20>(),
            )),
            value: Default::default(),
            input: calldata.into(),
        },
        &wallet,
    );

    let mut tracer = NopTracer::default();
    let coinbase = if sender_is_coinbase {
        // Make sure coinbase = from if the test is valid
        B160::from_be_bytes(from.0)
    } else {
        B160::ZERO
    };
    let block_context = BlockContext {
        coinbase,
        ..Default::default()
    };

    let (output, pi_batch_output) = chain
        .run_block_pi_output(vec![tx], Some(block_context), None, None, &mut tracer)
        .expect("Block should run successfully");

    // Verify the transaction succeeded
    assert_eq!(output.tx_results.len(), 1);
    assert!(output.tx_results[0].is_ok(), "Transaction should succeed");
    let tx_result = output.tx_results[0].as_ref().unwrap();
    assert!(tx_result.is_success(), "Transaction should be successful");
    // check the output to ensure the contract was called
    match &tx_result.execution_result {
        ExecutionResult::Success(ExecutionOutput::Call(_)) => (),
        _ => panic!("Execution result must be a successful call"),
    }
    // Check there's an event for every root
    assert!(tx_result.logs.len() == n_interop_roots);
    // Check the rolling hash in public input is the expected one
    assert_eq!(
        expected_rolling_hash, pi_batch_output.interop_roots_rolling_hash,
        "Mismatch in interop_roots_rolling_hash"
    );
}
