//!
//! These tests are focused on interop support in ZKsync OS
//!
#![cfg(test)]

use alloy::signers::local::PrivateKeySigner;
use rig::crypto::MiniDigest;
use rig::ruint::aliases::{B160, U256};
use rig::system_hooks::addresses_constants::L2_INTEROP_ROOT_STORAGE_ADDRESS;
use rig::utils::encode_interop_root_import_calldata;
use rig::zk_ee::common_structs::interop_root_storage::InteropRoot as StoredInteropRoot;
use rig::zk_ee::system::tracer::NopTracer;
use rig::zk_ee::utils::Bytes32;
use rig::zksync_os_interface::types::ExecutionOutput;
use rig::zksync_os_interface::types::ExecutionResult;
use rig::{alloy, zksync_web3_rs, BlockContext, Chain};
use std::str::FromStr;
use std::vec;
use zksync_web3_rs::signers::{LocalWallet, Signer};

const L2_INTEROP_ROOT_STORAGE_BYTECODE : &str = "608060405234801561000f575f5ffd5b506004361061004a575f3560e01c80633b43dbde1461004e57806377cfd1711461006a578063cca2f7bc1461009a578063fb6200c6146100b6575b5f5ffd5b610068600480360381019061006391906104c2565b6100d2565b005b610084600480360381019061007f919061053c565b610169565b6040516100919190610592565b60405180910390f35b6100b460048036038101906100af919061060c565b610188565b005b6100d060048036038101906100cb91906106ac565b6102aa565b005b60016180006100e19190610769565b73ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff1614610145576040517fefce78c700000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b610166815f0135826020013583806040019061016191906107bc565b61032f565b50565b5f602052815f5260405f20602052805f5260405f205f91509150505481565b60016180006101979190610769565b73ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff16146101fb576040517fefce78c700000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f8282905090505f5b818110156102a4576102998484838181106102225761022161081e565b5b9050602002810190610234919061084b565b5f013585858481811061024a5761024961081e565b5b905060200281019061025c919061084b565b602001358686858181106102735761027261081e565b5b9050602002810190610285919061084b565b806040019061029491906107bc565b61032f565b806001019050610204565b50505050565b60016180006102b99190610769565b73ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff161461031d576040517fefce78c700000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b6103298484848461032f565b50505050565b6001828290501461036c576040517f2f59bd0d00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f5f1b82825f8181106103825761038161081e565b5b90506020020135036103c0576040517f9b5f85eb00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f5f1b5f5f8681526020019081526020015f205f8581526020019081526020015f20541461041a576040517f2d48e8cf00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b81815f81811061042d5761042c61081e565b5b905060200201355f5f8681526020019081526020015f205f8581526020019081526020015f208190555082847f6b451b8422636e45b93bf7f594fa2c1769d039766c4254a6e7f9c0ee1715cdb0848460405161048a9291906108ea565b60405180910390a350505050565b5f5ffd5b5f5ffd5b5f5ffd5b5f606082840312156104b9576104b86104a0565b5b81905092915050565b5f602082840312156104d7576104d6610498565b5b5f82013567ffffffffffffffff8111156104f4576104f361049c565b5b610500848285016104a4565b91505092915050565b5f819050919050565b61051b81610509565b8114610525575f5ffd5b50565b5f8135905061053681610512565b92915050565b5f5f6040838503121561055257610551610498565b5b5f61055f85828601610528565b925050602061057085828601610528565b9150509250929050565b5f819050919050565b61058c8161057a565b82525050565b5f6020820190506105a55f830184610583565b92915050565b5f5ffd5b5f5ffd5b5f5ffd5b5f5f83601f8401126105cc576105cb6105ab565b5b8235905067ffffffffffffffff8111156105e9576105e86105af565b5b602083019150836020820283011115610605576106046105b3565b5b9250929050565b5f5f6020838503121561062257610621610498565b5b5f83013567ffffffffffffffff81111561063f5761063e61049c565b5b61064b858286016105b7565b92509250509250929050565b5f5f83601f84011261066c5761066b6105ab565b5b8235905067ffffffffffffffff811115610689576106886105af565b5b6020830191508360208202830111156106a5576106a46105b3565b5b9250929050565b5f5f5f5f606085870312156106c4576106c3610498565b5b5f6106d187828801610528565b94505060206106e287828801610528565b935050604085013567ffffffffffffffff8111156107035761070261049c565b5b61070f87828801610657565b925092505092959194509250565b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f6107738261071d565b915061077e8361071d565b9250828201905073ffffffffffffffffffffffffffffffffffffffff8111156107aa576107a961073c565b5b92915050565b5f5ffd5b5f5ffd5b5f5ffd5b5f5f833560016020038436030381126107d8576107d76107b0565b5b80840192508235915067ffffffffffffffff8211156107fa576107f96107b4565b5b602083019250602082023603831315610816576108156107b8565b5b509250929050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52603260045260245ffd5b5f82356001606003833603038112610866576108656107b0565b5b80830191505092915050565b5f82825260208201905092915050565b5f5ffd5b82818337505050565b5f61089a8385610872565b93507f07ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff8311156108cd576108cc610882565b5b6020830292506108de838584610886565b82840190509392505050565b5f6020820190508181035f83015261090381848661088f565b9050939250505056fea26469706673582212207d5b11ffa3ae60b0ae174df6b0fe6b1c7f2bcadbc1934064a28c7723c5d4e74d64736f6c634300081c0033";

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

fn run_test(interop_roots: Vec<StoredInteropRoot>) {
    run_test_inner(interop_roots)
}

/// Executes a transaction with specified interop roots and verifies success
fn run_test_inner(interop_roots: Vec<StoredInteropRoot>) {
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

    // Compute expected rolling hash
    let expected_rolling_hash = rig::basic_system::system_implementation::system::interop_roots::calculate_interop_roots_rolling_hash(
        Bytes32::ZERO,
        interop_roots.iter(),
        &mut rig::crypto::sha3::Keccak256::new(),
    );

    // Construct calldata
    let n_interop_roots = interop_roots.len();
    let calldata = encode_interop_root_import_calldata(interop_roots);

    let tx = rig::utils::encode_service_tx(
        0,
        50_000_000,
        &L2_INTEROP_ROOT_STORAGE_ADDRESS.to_be_bytes::<20>(),
        &calldata,
    );

    let mut tracer = NopTracer::default();

    let block_context = BlockContext {
        eip1559_basefee: U256::ZERO,
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
