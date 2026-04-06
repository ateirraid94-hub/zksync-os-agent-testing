//! CSR property table — maps CSR addresses to their delegation types.
//!
//! `FRI_VERIFIER_DELEGATION_CSR` (`0x7CC`) is added to the supported list.
//! The table is consulted by the transpiler and the circuit machine
//! configuration to determine which CSRRW instructions are delegation
//! invocations vs ordinary CSR reads.

use common_constants::delegation_types::fri_verifier::FRI_VERIFIER_DELEGATION_CSR;

/// All CSR addresses that map to a delegation circuit.
///
/// Extend this when adding new delegations.
pub const DELEGATION_CSR_ADDRESSES: &[u32] = &[
    0x7C7, // Blake2s
    0x7CA, // BigInt
    0x7CB, // Keccak
    FRI_VERIFIER_DELEGATION_CSR, // FRI Verifier (0x7CC)
];

/// Returns `true` if the given CSR address corresponds to a delegation circuit.
pub fn is_delegation_csr(csr: u32) -> bool {
    DELEGATION_CSR_ADDRESSES.contains(&csr)
}

/// Build the special CSR properties table used by the machine configuration.
///
/// Called during machine setup — see `machine_configurations/…/mod.rs`.
pub fn create_special_csr_properties_table() -> CsrPropertiesTable {
    let mut table = CsrPropertiesTable::default();
    for &csr in DELEGATION_CSR_ADDRESSES {
        table.mark_delegation(csr);
    }
    table
}

// Placeholder — replace with the actual table type from the codebase.
#[derive(Default)]
pub struct CsrPropertiesTable {
    delegation_csrs: std::collections::HashSet<u32>,
}

impl CsrPropertiesTable {
    pub fn mark_delegation(&mut self, csr: u32) {
        self.delegation_csrs.insert(csr);
    }

    pub fn is_delegation(&self, csr: u32) -> bool {
        self.delegation_csrs.contains(&csr)
    }
}
