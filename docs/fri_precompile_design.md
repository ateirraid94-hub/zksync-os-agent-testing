# FRI Precompile Design Document

## Overview

This document describes the design for a FRI (Fast Reed-Solomon Interactive Oracle Proofs of Proximity) precompile in ZKsync OS. The feature enables on-chain verification of FRI proofs via a new transaction type and a dedicated precompile contract, restricted to gateway nodes.

---

## 1. Transaction Format

### Type Byte

FRI proof transactions use type byte **`0x7c`** — the next available slot below:
- `0x7d` — service transactions
- `0x7e` — upgrade transactions
- `0x7f` — L1 transactions

### Encoding

RLP-encoded with the following fields:
```
fri_proof_tx := RLP([chain_id, version, proof_payload])
```

- `chain_id`: matches the current network chain ID
- `version`: one-byte version prefix
  - `0x00` — reserved
  - `0x01` — first deployed version
- `proof_payload`: opaque bytes containing the serialized FRI proof

### Sender Restriction

The `from` field **must** be `BOOTLOADER_FORMAL_ADDRESS`, identical to the constraint applied on service transactions. Any FRI proof transaction originating from a different address must be rejected.

---

## 2. Block Processing Flow

### Pre-Transaction Loop (FRI Proof Ingestion)

FRI proof transactions are consumed in an extended **`ZKHeaderStructurePreTxOp`** pre-loop, located in:

```
basic_bootloader/src/bootloader/block_flow/zk/pre_tx_loop.rs
```

This pre-loop runs **before** the main transaction loop. All `0x7c`-typed transactions in the block are extracted and validated here. The maximum number of FRI proof transactions per block is enforced via the constant:

```rust
MAX_FRI_PROOF_TXS_PER_BLOCK
```

### Main Transaction Loop (Ordering Enforcement)

The main loop (`tx_loop.rs`) enforces strict ordering: if a `0x7c` transaction appears **after** any regular (non-FRI-proof) transaction, the block is considered invalid:

```
InvalidTransaction::FriProofTxOutOfOrder
```

This guarantees that all FRI proof transactions are processed before user-facing transactions.

---

## 3. `fri_proof_context` Storage

### Design Pattern

The storage design follows the exact pattern of `new_settlement_layer_chain_id_storage` in:

```
basic_system/src/system_implementation/system/io_subsystem.rs
```

### New Types

A new `FriProofBlockContext` struct is introduced and embedded in `ZKBasicBlockDataKeeper`.

### IOSubsystem Trait Extensions

Two new methods are added to the `IOSubsystem` trait:

```rust
fn store_fri_proof_entry(index: u32, entry: FriProofBlockContext);
fn get_fri_proof_entry(index: u32) -> Option<FriProofBlockContext>;
```

The FRI precompile accesses stored proof data via `system.io.get_fri_proof_entry(index)`.

---

## 4. FRI Precompile

### Address

The precompile is registered at address **`0x12`**, which fits within the EVM precompile address range and has no conflicts with existing entries in `addresses_constants.rs`.

### Registration

The precompile is registered in `post_init_op.rs`, gated on `is_gateway`:

```rust
if metadata.is_gateway {
    register_precompile(FRI_PRECOMPILE_ADDRESS, fri_precompile_handler);
}
```

Only gateway nodes expose this precompile.

### Interface

**Input:**
```solidity
uint32 proof_index
```

**Output:**
```solidity
(bool success, bytes publicInputs)
```

- `success`: `true` if a proof entry exists at the given index and verification passed
- `publicInputs`: the serialized public inputs associated with the proof

---

## 5. Gateway Gating

### New Field in `BlockMetadataFromOracle`

A new boolean field is added to `BlockMetadataFromOracle` in:

```
zk_ee/src/system/metadata/zk_metadata.rs
```

```rust
pub is_gateway: bool,
```

### Serialization and Commitment

- The field is serialized as part of the `BLOCK_METADATA_QUERY_ID` blob.
- It is committed in the proof, meaning the value is cryptographically bound to each block.
- This prevents non-gateway nodes from forging gateway-mode behavior.

---

## 6. Precise File Changes (20+)

| File | Change |
|------|--------|
| `basic_bootloader/src/bootloader/block_flow/zk/pre_tx_loop.rs` | Extend pre-loop to consume and validate `0x7c` transactions |
| `basic_bootloader/src/bootloader/block_flow/zk/tx_loop.rs` | Add `FriProofTxOutOfOrder` ordering enforcement |
| `basic_bootloader/src/bootloader/transactions/mod.rs` | Register `0x7c` as a valid transaction type |
| `basic_bootloader/src/bootloader/transactions/fri_proof_tx.rs` | New: FRI proof transaction parsing and validation logic |
| `basic_system/src/system_implementation/system/io_subsystem.rs` | Add `store_fri_proof_entry` / `get_fri_proof_entry` methods |
| `basic_system/src/system_implementation/system/zk_basic_block_data_keeper.rs` | Add `FriProofBlockContext` field |
| `basic_system/src/system_implementation/system/fri_proof_context.rs` | New: `FriProofBlockContext` type definition |
| `basic_system/src/system_implementation/precompiles/fri.rs` | New: FRI precompile handler implementation |
| `basic_system/src/system_implementation/precompiles/mod.rs` | Export `fri` module |
| `basic_system/src/system_implementation/post_init_op.rs` | Register FRI precompile at `0x12` when `is_gateway` |
| `zk_ee/src/system/metadata/zk_metadata.rs` | Add `is_gateway: bool` to `BlockMetadataFromOracle` |
| `zk_ee/src/system/metadata/mod.rs` | Update serialization for `is_gateway` in `BLOCK_METADATA_QUERY_ID` |
| `addresses_constants.rs` (or equivalent) | Add `FRI_PRECOMPILE_ADDRESS = 0x12` constant |
| `basic_bootloader/src/bootloader/constants.rs` | Add `MAX_FRI_PROOF_TXS_PER_BLOCK` constant |
| `basic_bootloader/src/bootloader/errors.rs` | Add `FriProofTxOutOfOrder` variant to `InvalidTransaction` |
| `basic_bootloader/src/bootloader/block_flow/zk/mod.rs` | Re-export new pre-loop changes |
| `basic_system/src/lib.rs` | Export `fri_proof_context` module |
| Integration test: `tests/fri_proof_tx_integration.rs` | New: end-to-end test for FRI proof tx ingestion and precompile call |
| `Cargo.toml` (workspace or crate-level) | Add any new dependency for FRI proof deserialization if needed |
| `docs/fri_precompile_design.md` | This document |

---

## 7. Unresolved Questions

1. **Proof payload format**: What is the exact binary encoding of `proof_payload`? Is it a custom format or standard FRI wire format?
2. **Verification logic**: Where does actual FRI proof verification happen — in the precompile, in the pre-loop, or both?
3. **Public inputs binding**: How are public inputs committed relative to the block hash?
4. **Error behavior**: Should an invalid FRI proof abort the block, or only mark that proof index as failed?
5. **`MAX_FRI_PROOF_TXS_PER_BLOCK` value**: What is the concrete limit? Impacts block size and prover cost.
6. **Precompile gas cost**: What is the gas schedule for calling `0x12`? Fixed cost or proof-size-dependent?
7. **Re-entrancy / composability**: Can regular user transactions call the FRI precompile in the same block they were verified in?
8. **Proof index namespace**: Are proof indices global per block or per-chain?
9. **Serialization versioning**: How is the `version` byte in the transaction format used for forward compatibility?
10. **Gateway detection finality**: Can `is_gateway` change mid-upgrade? What happens to in-flight blocks?
11. **Testing strategy**: Can FRI proofs be mocked for unit tests, or does the test environment require a full prover?
12. **Consensus on address `0x12`**: Has `0x12` been formally reserved, or is this tentative pending cross-team review?

---

## 8. References

- Existing service transaction implementation: `basic_bootloader/src/bootloader/transactions/`
- Settlement layer storage pattern: `basic_system/src/system_implementation/system/io_subsystem.rs`
- Block metadata: `zk_ee/src/system/metadata/zk_metadata.rs`
- Existing precompile registrations: `basic_system/src/system_implementation/post_init_op.rs`
- Address constants: `addresses_constants.rs`
