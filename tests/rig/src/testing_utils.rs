use alloy::primitives::Address;
use forward_system::run::convert_alloy::FromAlloy;
use forward_system::system::tracers::call_tracer::CallTracer;
use ruint::aliases::B160;
use zk_ee::{system::validator, utils::Bytes32};
use zksync_os_tests_common::zksync_tx::encoding::ZKsyncOsEncodable;

use crate::{utils, Chain};

/// Recursive search for a required call
fn get_first_traced_subcall_to<'a>(
    address: &B160,
    call: &'a forward_system::system::tracers::call_tracer::Call,
) -> Option<&'a forward_system::system::tracers::call_tracer::Call> {
    if call.to == *address {
        return Some(call);
    }

    for subcall in &call.calls {
        let search_res = get_first_traced_subcall_to(address, subcall);
        if search_res.is_some() {
            return search_res;
        }
    }

    None
}

/// Find first call to the address in CallTracer results
pub fn get_first_traced_call_to(
    address: Address,
    tracer: &CallTracer,
) -> Option<&forward_system::system::tracers::call_tracer::Call> {
    let expected_to = B160::from_alloy(address);
    for tx in tracer.transactions.iter().flatten() {
        let search_res = get_first_traced_subcall_to(&expected_to, tx);
        if search_res.is_some() {
            return search_res;
        }
    }

    None
}

pub fn call_address_and_measure_gas_cost(
    address: Address,
    sender: Address,
    value: u64,
    calldata: Vec<u8>,
    additional_preimages: Vec<(Bytes32, Vec<u8>)>,
) -> u64 {
    let mut chain = Chain::empty(None);

    if value != 0 {
        let value_encoded = alloy::primitives::U256::from(value);
        chain.set_balance(B160::from_alloy(sender), value_encoded);
    }

    // Needed to test force deploys
    for (hash, preimage) in additional_preimages {
        chain.set_preimage(hash, &preimage);
    }

    let encoded_tx = {
        let tx = L1TxBuilder::new()
            .from(sender)
            .to(address)
            .gas_price(1000)
            .gas_limit(200_000)
            .value(alloy::primitives::U256::from(value))
            .input(calldata)
            .nonce(0)
            .build();

        tx.encode()
    };
    let transactions = vec![encoded_tx];

    let mut tracer = CallTracer::default();
    let mut validator = validator::NopTxValidator;

    let (output, _, _) = chain
        .run_block_with_extra_stats(transactions, None, None, None, &mut tracer, &mut validator)
        .expect("Should succeed");

    // Assert transaction succeeded
    assert!(output.tx_results[0].is_ok());
    let tx_result = output.tx_results[0].as_ref().unwrap();
    assert!(tx_result.is_success());

    let call_to_system_hook = get_first_traced_call_to(address, &tracer).expect("Should exist");
    call_to_system_hook.gas_used
}
