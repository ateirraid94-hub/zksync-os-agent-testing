//! Well-known ZKsync OS addresses used by the REVM consistency checker.

use revm_primitives::Address;

/// The formal address assigned to the bootloader.
///
/// This address is the implicit sender for all system-level transactions:
/// service txs (`0x7d`), FRI proof txs (`0x7c`), and upgrade txs (`0x7e`).
/// Verify the exact value against `zksync-os/basic_system/src/addresses_constants.rs`.
pub const BOOTLOADER_FORMAL_ADDRESS: Address =
    Address::new(revm_primitives::hex_literal::hex!(
        "000000000000000000000000000000000000800b"
    ));
