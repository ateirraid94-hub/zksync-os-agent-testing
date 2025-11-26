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

const L2_INTEROP_ROOT_STORAGE_BYTECODE : &str = "608060405234801561000f575f5ffd5b5060043610610055575f3560e01c8063140e31cf146100595780633b43dbde1461007757806377cfd17114610093578063cca2f7bc146100c3578063fb6200c6146100df575b5f5ffd5b6100616100fb565b60405161006e9190610688565b60405180910390f35b610091600480360381019061008c91906106cb565b610101565b005b6100ad60048036038101906100a8919061073c565b6101b4565b6040516100ba9190610792565b60405180910390f35b6100dd60048036038101906100d8919061080c565b6101d3565b005b6100f960048036038101906100f491906108ac565b610288565b005b60015481565b4173ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff1614610166576040517f4db373cd00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b43600154036101a1576040517f074bb98e00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b436001819055506101b181610341565b50565b5f602052815f5260405f20602052805f5260405f205f91509150505481565b4173ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff1614610238576040517f4db373cd00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b4360015403610273576040517f074bb98e00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b436001819055506102848282610365565b5050565b4173ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff16146102ed576040517f4db373cd00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b4360015403610328576040517f074bb98e00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b4360018190555061033b84848484610414565b50505050565b610362815f0135826020013583806040019061035d9190610929565b610414565b50565b5f8282905090505f5b8181101561040e5761040384848381811061038c5761038b61098b565b5b905060200281019061039e91906109b8565b5f01358585848181106103b4576103b361098b565b5b90506020028101906103c691906109b8565b602001358686858181106103dd576103dc61098b565b5b90506020028101906103ef91906109b8565b80604001906103fe9190610929565b610414565b80600101905061036e565b50505050565b60018282905014610451576040517f2f59bd0d00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f5f1b82825f8181106104675761046661098b565b5b90506020020135036104a5576040517f9b5f85eb00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f5f1b5f5f8681526020019081526020015f205f8581526020019081526020015f2054146104ff576040517f2d48e8cf00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b81815f8181106105125761051161098b565b5b905060200201355f5f8681526020019081526020015f205f8581526020019081526020015f2081905550610560848484845f8181106105545761055361098b565b5b905060200201356105a1565b82847f6b451b8422636e45b93bf7f594fa2c1769d039766c4254a6e7f9c0ee1715cdb08484604051610593929190610a57565b60405180910390a350505050565b5f8383836040516020016105b793929190610ab9565b60405160208183030381529060405290505f61700373ffffffffffffffffffffffffffffffffffffffff16826040516105f09190610b47565b5f604051808303815f865af19150503d805f8114610629576040519150601f19603f3d011682016040523d82523d5f602084013e61062e565b606091505b5050905080610669576040517f2d53be6900000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5050505050565b5f819050919050565b61068281610670565b82525050565b5f60208201905061069b5f830184610679565b92915050565b5f5ffd5b5f5ffd5b5f5ffd5b5f606082840312156106c2576106c16106a9565b5b81905092915050565b5f602082840312156106e0576106df6106a1565b5b5f82013567ffffffffffffffff8111156106fd576106fc6106a5565b5b610709848285016106ad565b91505092915050565b61071b81610670565b8114610725575f5ffd5b50565b5f8135905061073681610712565b92915050565b5f5f60408385031215610752576107516106a1565b5b5f61075f85828601610728565b925050602061077085828601610728565b9150509250929050565b5f819050919050565b61078c8161077a565b82525050565b5f6020820190506107a55f830184610783565b92915050565b5f5ffd5b5f5ffd5b5f5ffd5b5f5f83601f8401126107cc576107cb6107ab565b5b8235905067ffffffffffffffff8111156107e9576107e86107af565b5b602083019150836020820283011115610805576108046107b3565b5b9250929050565b5f5f60208385031215610822576108216106a1565b5b5f83013567ffffffffffffffff81111561083f5761083e6106a5565b5b61084b858286016107b7565b92509250509250929050565b5f5f83601f84011261086c5761086b6107ab565b5b8235905067ffffffffffffffff811115610889576108886107af565b5b6020830191508360208202830111156108a5576108a46107b3565b5b9250929050565b5f5f5f5f606085870312156108c4576108c36106a1565b5b5f6108d187828801610728565b94505060206108e287828801610728565b935050604085013567ffffffffffffffff811115610903576109026106a5565b5b61090f87828801610857565b925092505092959194509250565b5f5ffd5b5f5ffd5b5f5ffd5b5f5f833560016020038436030381126109455761094461091d565b5b80840192508235915067ffffffffffffffff82111561096757610966610921565b5b60208301925060208202360383131561098357610982610925565b5b509250929050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52603260045260245ffd5b5f823560016060038336030381126109d3576109d261091d565b5b80830191505092915050565b5f82825260208201905092915050565b5f5ffd5b82818337505050565b5f610a0783856109df565b93507f07ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff831115610a3a57610a396109ef565b5b602083029250610a4b8385846109f3565b82840190509392505050565b5f6020820190508181035f830152610a708184866109fc565b90509392505050565b5f819050919050565b610a93610a8e82610670565b610a79565b82525050565b5f819050919050565b610ab3610aae8261077a565b610a99565b82525050565b5f610ac48286610a82565b602082019150610ad48285610a82565b602082019150610ae48284610aa2565b602082019150819050949350505050565b5f81519050919050565b5f81905092915050565b8281835e5f83830152505050565b5f610b2182610af5565b610b2b8185610aff565b9350610b3b818560208601610b09565b80840191505092915050565b5f610b528284610b17565b91508190509291505056fea264697066735822122011b80f186b12ab22e73667bd52ba374749a115c52b055346bcdec387cd9842e464736f6c634300081c0033";

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
