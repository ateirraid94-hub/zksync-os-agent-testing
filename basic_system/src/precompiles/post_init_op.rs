//! Post-initialisation operations — registers precompiles after genesis.
//!
//! The FRI proof verifier at `0x12` is **gateway-only**: it is only registered
//! when `block_metadata.is_gateway == true`.  On non-gateway (standard L2)
//! nodes the address behaves as an empty account and calls return empty data.

use crate::precompiles::fri_proof_verifier::FRI_PROOF_VERIFIER_ADDRESS;
use zk_ee::system::metadata::BlockMetadataFromOracle;

/// Register all precompiles appropriate for the current node role.
pub fn register_precompiles(metadata: &BlockMetadataFromOracle) {
    register_standard_precompiles();

    if metadata.is_gateway {
        register_gateway_precompiles();
    }
}

fn register_standard_precompiles() {
    // sha256 @ 0x02, ecrecover @ 0x01, etc. — existing registrations unchanged.
}

fn register_gateway_precompiles() {
    // FRI proof verifier — only active on gateway sequencer nodes.
    register_precompile(FRI_PROOF_VERIFIER_ADDRESS);
}

fn register_precompile(_address: u32) {
    // Actual implementation hooks into the VM's precompile dispatch table.
    // Stubbed here to show the registration call site.
}
