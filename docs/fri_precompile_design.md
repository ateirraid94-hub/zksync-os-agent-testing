# FRI Precompile Design Document

## Overview

This document describes the design for integrating FRI (Fast Reed-Solomon Interactive) proof transactions and a corresponding precompile into the zkEVM bootloader and system.

---

## Transaction Format

### Type Byte

- **`0x7c`** — next available slot below:
  - Service tx: `0x7d`
  - Upgrade tx: `0x7e`
  - L1 tx: `0x7f`

### Encoding

- RLP-encoded with fields: `chain_id` + `proof_payload`
- `from` **must** be `BOOTLOADER_FORMAL_ADDRESS` (same constraint as service transactions)

### Version Byte Prefix

| Value | Meaning |
|-------|---------|
| `0x00` | Reserved |
| `0x01` | First deployed version |

---

## Block Processing Flow

### Pre-Transaction Loop

FRI proof transactions are consumed in an extended **`ZKHeaderStructurePreTxOp` pre-loop** before the main transaction loop.

- Location: `basic_bootloader/src/bootloader/block_flow/zk/pre_tx_loop.rs`
- Verified against constant `MAX_FRI_PROOF_TXS_PER_BLOCK`
- All `0x7c` transactions must appear before any regular transactions in the block

### Main Transaction Loop

- Location: `basic_bootloader/src/bootloader/block_flow/zk/tx_loop.rs`
- Enforces ordering: if a `0x7c` tx appears after any regular tx → `InvalidTransaction::FriProofTxOutOfOrder`

---

## `fri_proof_context` Storage

### Pattern

Follows the exact pattern of `new_settlement_layer_chain_id_storage` in:

```
basic_system/src/system_implementation/system/io_subsystem.rs
```

### New Structures

- `FriProofBlockContext` — new context struct added to `ZKBasicBlockDataKeeper`
- Two new methods added to the `IOSubsystem` trait:
  - (getter) `get_fri_proof_entry(index: u32) -> FriProofBlockContext`
  - (setter) `set_fri_proof_entry(index: u32, ctx: FriProofBlockContext)`

### Precompile Access

The FRI precompile reads proof entries via:

```rust
system.io.get_fri_proof_entry(index)
```

---

## FRI Precompile

### Address

- **`0x12`** — fits within the EVM precompile address range; no conflicts with existing entries in `addresses_constants.rs`

### Registration

- Gateway-only: registered in `post_init_op.rs` **conditional on `is_gateway`**
- Non-gateway deployments do not expose this precompile

### ABI

**Input:**

```solidity
uint32 proof_index
```

**Output:**

```solidity
(bool success, bytes publicInputs)
```

---

## Gateway Gating

### New Field

```rust
// zk_ee/src/system/metadata/zk_metadata.rs
pub struct BlockMetadataFromOracle {
    // ... existing fields ...
    pub is_gateway: bool,
}
```

- Serialized as part of `BLOCK_METADATA_QUERY_ID`
- Committed in the proof, so it cannot be spoofed by the sequencer

---

## File Change Index (20+ locations)

| File | Change |
|------|--------|
| `basic_bootloader/src/bootloader/block_flow/zk/pre_tx_loop.rs` | Extended pre-loop consuming `0x7c` txs; enforce `MAX_FRI_PROOF_TXS_PER_BLOCK` |
| `basic_bootloader/src/bootloader/block_flow/zk/tx_loop.rs` | Reject `0x7c` after regular tx with `FriProofTxOutOfOrder` |
| `basic_bootloader/src/bootloader/tx_parsing/mod.rs` | Decode type `0x7c` RLP structure |
| `basic_bootloader/src/bootloader/tx_parsing/fri_proof_tx.rs` | New file: `FriProofTransaction` struct + decode logic |
| `basic_bootloader/src/bootloader/constants.rs` | Add `MAX_FRI_PROOF_TXS_PER_BLOCK` |
| `basic_system/src/system_implementation/system/io_subsystem.rs` | Add `FriProofBlockContext`; extend `IOSubsystem` trait with `get/set_fri_proof_entry` |
| `basic_system/src/system_implementation/system/zk_basic_block_data_keeper.rs` | Add `fri_proof_entries` field to `ZKBasicBlockDataKeeper` |
| `basic_system/src/system_implementation/precompiles/mod.rs` | Register FRI precompile at `0x12` |
| `basic_system/src/system_implementation/precompiles/fri_proof.rs` | New file: FRI precompile implementation |
| `basic_system/src/system_implementation/post_init_op.rs` | Conditional registration of FRI precompile on `is_gateway` |
| `zk_ee/src/system/metadata/zk_metadata.rs` | Add `is_gateway: bool` to `BlockMetadataFromOracle` |
| `zk_ee/src/system/metadata/mod.rs` | Serialize/deserialize `is_gateway` in `BLOCK_METADATA_QUERY_ID` |
| `zk_ee/src/system/errors.rs` | Add `InvalidTransaction::FriProofTxOutOfOrder` variant |
| `zk_ee/src/system/errors.rs` | Add `InvalidTransaction::FriProofTxNotFromBootloader` variant |
| `addresses_constants.rs` (system crate) | Add `FRI_PRECOMPILE_ADDRESS = 0x12` |
| `basic_bootloader/src/bootloader/tx_validation/mod.rs` | Validate `from == BOOTLOADER_FORMAL_ADDRESS` for `0x7c` |
| `basic_bootloader/src/bootloader/tx_validation/fri_proof_tx_validation.rs` | New file: validation logic for FRI proof txs |
| `basic_bootloader/src/bootloader/block_flow/zk/mod.rs` | Wire new pre-loop op into block flow |
| `basic_system/src/system_implementation/system/mod.rs` | Export new types |
| `zk_ee/src/lib.rs` | Re-export metadata changes |

---

## Unresolved Questions

1. What is the exact serialization format for `proof_payload` (length-prefixed bytes vs. fixed-size fields)?
2. Should `MAX_FRI_PROOF_TXS_PER_BLOCK` be a compile-time constant or a runtime oracle value?
3. How are `publicInputs` encoded in the precompile output — ABI-encoded or raw bytes?
4. Does the proof verifier run inside the precompile call or is it pre-verified in the pre-tx loop?
5. What happens if `proof_index` is out of range — revert or return `(false, "")`?
6. Is `is_gateway` set by the oracle or derived from chain configuration?
7. Should non-gateway chains silently ignore `0x7c` txs or hard-reject them?
8. What gas cost should be assigned to the FRI precompile call?
9. Are FRI proof txs included in the block's transaction count for the executor?
10. How does the L1 verifier validate the `is_gateway` commitment?
11. Should `FriProofBlockContext` store the raw payload or parsed fields?
12. What is the upgrade path if the `0x01` version format needs to change?
