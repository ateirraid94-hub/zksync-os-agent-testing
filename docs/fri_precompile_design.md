# FRI Precompile Design

## Overview

This document describes the design for gateway-only FRI proof submission transactions and the corresponding FRI verification precompile in zksync-os. The feature enables the gateway to accept ZK proof payloads as a dedicated transaction type, verify them at block start, and expose results to EVM contracts via a new precompile.

**Scope of this document**: zksync-os only. Changes to zksync-os-server, era-contracts, zksync-os-revm, and zksync-airbender are described at the interface level; their internal implementations are outside this document's scope.

---

## 1. Transaction Format and Encoding

### Type Byte

A new ZKsync-specific transaction type `0x7c` is introduced for FRI proof submission. The existing type space is:

| Type | Meaning |
|------|---------|
| `0x7f` | L1 priority transaction (`L1Tx`) |
| `0x7e` | Upgrade transaction |
| `0x7d` | Service transaction |
| `0x7c` | **FRI proof transaction (new)** |
| `0x01`–`0x04` | Standard EVM typed transactions (EIP-2930/1559/4844/7702) |
| legacy | Untyped (LegacyTx, LegacyWithEIP155) |

### Encoding

An FRI proof transaction is RLP-encoded with the following fields, analogous to the existing service transaction (`basic_bootloader/src/bootloader/transaction/rlp_encoded/transaction_types/service_tx.rs`):

- `chain_id: u64` — must match the expected chain ID (used for replay protection and gateway identification)
- `proof_payload: bytes` — the raw versioned proof payload

The proof payload itself has the following layout:

- Byte 0: `version: u8` — version tag for forward compatibility
- Bytes 1..N: `proof_data: bytes` — version-specific proof bytes

Version `0x01` is the first deployed encoding. Version `0x00` is reserved and must be rejected. Future versions increment the version byte; the verifier dispatch is keyed on this byte.

The `from` address is required to be `BOOTLOADER_FORMAL_ADDRESS` (the same constraint as service transactions). There is no signature: the transaction is operator-constructed and injected directly, similar to how service transactions are gated.

### Why Not Blobs

EIP-4844 blob transactions were considered but rejected: proof payloads would exceed a single blob's 128 KiB capacity and cannot be split across blobs without application-level reassembly. A dedicated type avoids this constraint.

---

## 2. Block Processing Flow

### Principle

All FRI proof transactions must be processed **before** any regular user transactions. This ensures that verified proof results are available in the block context when user transactions that invoke the FRI precompile execute.

The sequencer is responsible for ordering: FRI proof transactions must occupy contiguous positions at the beginning of the transaction stream.

### Pre-loop Phase

The existing block flow uses a trait-based composition (defined in `basic_bootloader/src/bootloader/block_flow/`):

```
MetadataInitOp → PostSystemInitOp → PreTxLoopOp → TxLoopOp → PostTxLoopOp
```

A new processing phase—the **FRI proof pre-loop**—is introduced between `PreTxLoopOp` and `TxLoopOp`. Concretely:

- The existing `ZKHeaderStructurePreTxOp` (`basic_bootloader/src/bootloader/block_flow/zk/pre_tx_loop.rs`) currently creates the `ZKBasicBlockDataKeeper` and returns immediately.
- It is extended to also consume all leading `0x7c` transactions from the oracle stream before returning control to the main loop.
- Each FRI proof transaction is decoded, its proof payload extracted, version byte checked, and the airbender verifier invoked.
- The result (success or failure, along with any public inputs on success) is appended to a new `fri_proof_context` field in `ZKBasicBlockDataKeeper`.

The pre-loop reads transactions from the same oracle stream as the regular loop (`system.try_begin_next_tx(...)`). It peeks at the type byte; if the first byte of the next transaction buffer is `0x7c` it processes it as an FRI proof transaction, otherwise it stops and the main loop takes over.

### Main Loop Enforcement

The main `TxLoopOp` (`basic_bootloader/src/bootloader/block_flow/zk/tx_loop.rs`) already enforces service block invariants via `check_for_service_block_invariants`. A similar check must be added for FRI proof transactions: if a `0x7c` transaction appears after any non-FRI transaction in the main loop, this is a protocol violation and returns an `InvalidTransaction::FriProofTxOutOfOrder` error, causing the block to be invalid.

This ordering invariant is enforced on both forward and proving paths.

### Per-block Verification Budget

A constant `MAX_FRI_PROOF_TXS_PER_BLOCK` is added to `basic_bootloader/src/bootloader/constants.rs` to bound the number of proof transactions per block. If this limit is exceeded, the excess transactions are treated as invalid. The initial value is subject to benchmarking; a placeholder value is defined for testing.

---

## 3. FRI Proof Block Context (`fri_proof_context`)

### Structure

A new struct `FriProofBlockContext` is defined in `basic_bootloader/src/bootloader/block_flow/zk/block_data.rs` (alongside `ZKBasicBlockDataKeeper`). It holds the results of all verified FRI proof transactions for the current block:

- An `ArrayVec<FriProofEntry, MAX_FRI_PROOF_TXS_PER_BLOCK>` where each entry contains:
  - `proof_index: u32` — position in the block's FRI proof sequence (0-based)
  - `verification_ok: bool` — whether the verifier accepted the proof
  - `public_inputs: FriPublicInputs` — the public inputs extracted from a successful verification; zero-valued on failure
  - `version: u8` — proof encoding version used

`FriPublicInputs` is a fixed-size struct whose layout matches the output of the unified verifier from zksync-airbender. Its exact fields depend on the verifier API; the design assumes it is a fixed-size value type.

### Storage Location

`FriProofBlockContext` is a field added directly to `ZKBasicBlockDataKeeper`:

```
ZKBasicBlockDataKeeper {
    ...existing fields...
    pub fri_proof_context: FriProofBlockContext,  // new
}
```

### Precompile Access Path

The FRI precompile is a call hook registered in `system_hooks`. Call hooks receive `system: &mut System<S>`. To give the precompile access to `FriProofBlockContext`, it must be made reachable through `System<S>`.

The chosen mechanism follows the same pattern as `new_settlement_layer_chain_id_storage` in `basic_system/src/system_implementation/system/io_subsystem.rs`: the block context is stored in the IO subsystem.

Specifically:
- A new field `fri_proof_context: FriProofBlockContext` is added to the concrete `FullIO` struct in `basic_system/src/system_implementation/system/io_subsystem.rs`.
- Two new methods are added to the `IOSubsystem` trait in `zk_ee/src/system/io.rs`:
  - `fn set_fri_proof_entry(&mut self, entry: FriProofEntry) -> Result<(), InternalError>` — adds a verified entry; fails if `proof_index` is out of bounds or already set.
  - `fn get_fri_proof_entry(&self, proof_index: u32) -> Option<&FriProofEntry>` — retrieves entry by index.
- The pre-loop writes entries via `system.io.set_fri_proof_entry(...)` after verifying each proof.
- The FRI precompile reads via `system.io.get_fri_proof_entry(index)`.

`FriProofBlockContext` is reset at block start (as part of `ZKBasicBlockDataKeeper::new()`). It does not persist across blocks and is not written to the state tree.

---

## 4. FRI Precompile Interface

### Address

A new EVM-callable precompile is registered at address `0x12` (decimal 18). The existing precompile addresses in `system_hooks/src/addresses_constants.rs` end at `0x0a` (point evaluation), with `0x100` used for P256. Address `0x12` fits within the EVM precompile range and does not conflict with any currently defined address.

A new constant `FRI_VERIFIER_PRECOMPILE_ADDRESS_LOW: u8 = 0x12` is added to `system_hooks/src/addresses_constants.rs`.

### Registration

The precompile is registered in `system_hooks/src/lib.rs` inside `add_precompiles()`. Unlike other precompiles, it is registered **only when the instance is running as the gateway**. The gating condition is described in Section 9. On non-gateway instances, calls to address `0x12` behave as calls to an empty account (return empty, no revert), consistent with standard EVM semantics for unregistered precompiles.

### Input Encoding

The calldata to the FRI precompile is ABI-encoded with a single `uint32` field:

- `proof_index: uint32` — the zero-based index of the proof in the block's FRI proof sequence.

This is consistent with the `solidity` ABI encoding conventions used elsewhere (the FRI precompile is callable from Solidity without special tooling).

### Output Encoding

The return data is ABI-encoded as `(bool success, bytes publicInputs)`:

- `success: bool` — `true` if the proof at `proof_index` was verified successfully.
- `publicInputs: bytes` — the serialized `FriPublicInputs` from the verified proof on success; empty bytes on failure.

If `proof_index` is out of range (no such proof in the block), the precompile reverts with a fixed error selector.

### Gas Cost

A constant `FRI_PRECOMPILE_BASE_COST_ERGS` is defined in `basic_bootloader/src/bootloader/constants.rs`. The cost covers the lookup and ABI encoding. It does not include re-verification cost because verification was already performed at block start. The value is determined by benchmarking native cycle counts.

---

## 5. Sequencer Changes (zksync-os-server)

The sequencer changes are outside zksync-os but their interface with zksync-os determines what zksync-os must expose.

### What zksync-os must expose

The `zksync-os-server` sequencer needs:

1. A way to construct and encode `0x7c` FRI proof transactions. The encoding is described in Section 1; the sequencer assembles the RLP-encoded byte vector using the same utilities used for service transactions.

2. The `BlockMetadataFromOracle` structure (via `zksync_os_interface` or `forward_system` API) must carry the `is_gateway` flag so that the sequencer can set it correctly when building oracle inputs.

3. The oracle processor for the tx stream must accept `0x7c` transactions in the transaction list source (`TxListSource` in `zksync_os_interface/src/traits.rs`). The transaction bytes are passed through as-is; no special handling at the `TxListSource` level is needed.

### Sequencer responsibilities (documented for clarity)

- In gateway mode: accept proof submission requests, construct `0x7c` transactions, and prepend them to the transaction list before regular transactions in each block.
- In L2 mode: reject `0x7c` transactions at the mempool level (do not include in blocks).
- Enforce the per-block limit `MAX_FRI_PROOF_TXS_PER_BLOCK`.

---

## 6. Gateway Contract Interface (era-contracts)

The gateway contract changes are outside zksync-os but their interaction with the settlement layer determines proof submission flow.

At the contract level, a new L1 entry point is needed to accept proof payloads and include them in gateway blocks. From the zksync-os perspective, the only interface requirement is:

- The proof payload bytes supplied by the L1 contract are identical to the `proof_payload` field that appears in the `0x7c` transaction (version byte followed by raw proof data). No additional wrapping is performed by zksync-os.
- The `chain_id` in the `0x7c` transaction is the gateway chain ID.

No changes to zksync-os are driven by the contract interface beyond what is described in other sections.

---

## 7. REVM Consistency Checker Replay Strategy

The REVM consistency checker (`tests/rig/src/revm_consistency_checker.rs`, `tests/revm_runner/`) replays blocks using REVM to check output consistency with zksync-os forward execution. It must be updated to handle `0x7c` transactions.

### What changes in zksync-os

The `BlockContext` structure in `zksync_os_interface/src/types.rs` (currently exposed as `BlockContextInterface` in the rig) needs a new field that carries the pre-verified FRI proof results so that the REVM runner can mock the FRI precompile.

Specifically, a `fri_proof_results: Vec<FriProofResult>` field is added to the `BlockOutput` or `BlockContext` type that is returned by the forward system after block execution. The REVM runner reads from this field when the FRI precompile is invoked during REVM re-execution, returning the pre-computed result rather than re-running the verifier.

The `FriProofResult` type in the interface mirrors `FriProofEntry` (proof_index, verification_ok, serialized public_inputs) and is kept in `zksync_os_interface`.

### Sequencing within the checker

`0x7c` transactions are consumed from the transaction list in the forward system run (the REVM checker sees them as "consumed before REVM execution"). The REVM runner does not attempt to execute `0x7c` transactions in REVM because they have no EVM semantics. The checker's transaction iteration loop skips `0x7c` entries and instead populates the FRI precompile mock state from `BlockOutput::fri_proof_results`.

---

## 8. Forward/Proving Mode Considerations

### Forward Mode

In forward mode (`forward_system/src/run/`), FRI proof transactions flow through the standard oracle machinery:

- `TxDataResponder` in `forward_system/src/run/query_processors/` already serves transaction data via `TX_DATA_WORDS_QUERY_ID` and `TX_ENCODING_FORMAT_QUERY_ID`. No new oracle query IDs are required for proof payload delivery; the `0x7c` transaction bytes are served through the same transaction oracle path.
- A new `FriProofVerifier` is added to the forward system. During the pre-loop phase, after decoding the `0x7c` transaction, the forward system calls the airbender verifier directly (the `prover` crate from `zksync-airbender` is already a workspace dependency).
- In `call_simulation` mode (`BasicBootloaderCallSimulationConfig`), FRI proof verification may be skipped (analogous to how simulation skips signature validation), returning a synthetic "success" result. This is controlled by a new `SKIP_FRI_PROOF_VERIFICATION: bool` const parameter on the config trait.

### Proving Mode (RISC-V)

In proving mode (`proof_running_system/src/`):

- The same code path runs on RISC-V. The airbender verifier is the same Rust library (no `std` or global allocator usage in the verifier path; this must be verified when the verifier crate is integrated).
- Memory for proof payload buffers is allocated from `S::Allocator` (the `TalcAllocator` in proving mode), not from a global allocator. The buffer for the proof payload is allocated via `UsizeAlignedByteBox::preallocated_in(tx_length_in_bytes, allocator)`, which is the same mechanism used for all other transactions in the main loop.
- The RISC-V build (`zksync_os/dump_bin.sh`) does not require changes beyond ensuring the new code compiles for the `riscv32i-unknown-none-elf` target and does not introduce `std` dependencies.

### Cross-mode Correctness

The forward and proving modes must produce identical `FriProofBlockContext` results for a given set of `0x7c` transactions. Any divergence would constitute a proving bug. Specifically:

- Both modes must run the same verifier with the same inputs.
- Both must apply the same version byte dispatch.
- Both must apply the same `MAX_FRI_PROOF_TXS_PER_BLOCK` limit.
- Integration tests in `tests/instances/` must include at least one test that runs both forward and proving paths (via `execute_block` in the test rig) with a valid FRI proof transaction and confirms consistent results.

---

## 9. Gateway vs L2 Separation

### Gating Mechanism

A new `is_gateway: bool` field is added to `BlockMetadataFromOracle` in `zk_ee/src/system/metadata/zk_metadata.rs`.

This field is set by the sequencer in the oracle block metadata response and is passed into the bootloader via the existing `BLOCK_METADATA_QUERY_ID` oracle query. The `UsizeSerializable` and `UsizeDeserializable` implementations for `BlockMetadataFromOracle` are updated to include this field (one additional `usize` word, serialized as 0 or 1).

### Enforcement in zksync-os

The `is_gateway` field is checked in two places:

**1. FRI proof pre-loop (in `pre_tx_loop.rs`):**
Before consuming any `0x7c` transactions, the pre-loop checks `system.get_metadata().is_gateway()`. If false, and a `0x7c` transaction is present in the stream, it is rejected with `InvalidTransaction::FriProofTxOnNonGateway`. The block is not invalidated as a whole; the individual transaction is marked invalid and the main loop continues. (This mirrors how individual transaction encoding errors are handled in the existing loop—logged and recorded as invalid, not fatal to the block.)

**2. FRI precompile registration (in `post_init_op.rs`):**
In `ZKHeaderPostInitOp` (`basic_bootloader/src/bootloader/block_flow/zk/post_init_op.rs`), the call to `system_hooks::add_precompiles(...)` is extended to conditionally register the FRI precompile only when `is_gateway` is true. This means the precompile address `0x12` is only an active hook on the gateway; on L2 instances it is a no-op address.

### Why a New Field Rather Than Chain ID Comparison

Chain ID alone is insufficient for gating because:
- The gateway chain ID is not a compile-time constant; it is configurable.
- A comparison would require encoding the gateway chain ID in block metadata or a separate oracle query.
- An explicit `is_gateway` flag is simpler, more auditable, and removes ambiguity.

The sequencer is trusted to set this flag correctly; it is checked in the block proof (since `BlockMetadataFromOracle` is part of the prover's public inputs via the block metadata oracle query, the flag is committed in the proof).

---

## 10. Affected Files in zksync-os

The following files require changes. This list is precise; no file is listed unless a concrete change to it was identified in this analysis.

### `zk_ee/src/system/metadata/zk_metadata.rs`
- Add `is_gateway: bool` field to `BlockMetadataFromOracle`.
- Update `UsizeSerializable::USIZE_LEN` and `iter()` to include this field.
- Update `UsizeDeserializable::USIZE_LEN` and `from_iter()` to deserialize this field.
- Add accessor `is_gateway(&self) -> bool` to the `BasicBlockMetadata` trait implementation.

### `zk_ee/src/system/metadata/basic_metadata.rs`
- Add `fn is_gateway(&self) -> bool` to the `BasicBlockMetadata` trait.

### `zk_ee/src/system/io.rs`
- Add `fn set_fri_proof_entry(&mut self, entry: FriProofEntry) -> Result<(), InternalError>` to `IOSubsystem` trait.
- Add `fn get_fri_proof_entry(&self, proof_index: u32) -> Option<&FriProofEntry>` to `IOSubsystem` trait.

### `zk_ee/src/common_structs/mod.rs`
- Add `pub mod fri_proof_context;` to the module list.

### `zk_ee/src/common_structs/fri_proof_context.rs` *(new file)*
- Define `FriPublicInputs` (fixed-size struct matching airbender verifier output).
- Define `FriProofEntry` (proof_index, verification_ok, version, public_inputs).
- Define `FriProofBlockContext` (ArrayVec of FriProofEntry, bounded by `MAX_FRI_PROOF_TXS_PER_BLOCK`).

### `basic_bootloader/src/bootloader/constants.rs`
- Add `MAX_FRI_PROOF_TXS_PER_BLOCK: usize`.
- Add `FRI_PRECOMPILE_BASE_COST_ERGS: u64`.

### `basic_bootloader/src/bootloader/errors.rs`
- Add `FriProofTxOnNonGateway` variant to `InvalidTransaction`.
- Add `FriProofTxOutOfOrder` variant to `InvalidTransaction`.
- Add `FriProofVersionUnsupported` variant to `InvalidTransaction`.
- Add `FriProofLimitExceeded` variant to `InvalidTransaction`.

### `basic_bootloader/src/bootloader/transaction/rlp_encoded/transaction_types/mod.rs`
- Add `pub mod fri_proof_tx;`.

### `basic_bootloader/src/bootloader/transaction/rlp_encoded/transaction_types/fri_proof_tx.rs` *(new file)*
- Define `FriProofTx` struct with fields `chain_id: u64` and `proof_payload: &'a [u8]`.
- Implement `EthereumTxType` with `TX_TYPE = 0x7c`.
- Implement RLP decoding from raw bytes.
- Validate that `from == BOOTLOADER_FORMAL_ADDRESS`.

### `basic_bootloader/src/bootloader/transaction/rlp_encoded/mod.rs`
- Add `FriProofTx(FriProofTx<'a>)` variant to `RlpEncodedTxInner`.
- Add `FriProofTx::TX_TYPE => { ... }` arm to the `parse_and_compute_signed_hash` dispatch match.

### `basic_bootloader/src/bootloader/transaction/mod.rs`
- Add `is_fri_proof()` method to the `Transaction<A>` facade.
- Ensure `tx_type()` returns `0x7c` for FRI proof transactions.

### `basic_bootloader/src/bootloader/transaction_flow/process_transaction.rs`
- In `process_transaction()`, add routing for `is_fri_proof()` transactions: these must not reach this function at all (they are consumed in the pre-loop); add an `internal_error!` assert if one is encountered here.

### `basic_bootloader/src/bootloader/block_flow/zk/block_data.rs`
- Import `FriProofBlockContext` from `zk_ee::common_structs::fri_proof_context`.
- Add `pub fri_proof_context: FriProofBlockContext` field to `ZKBasicBlockDataKeeper`.
- Initialize it in `ZKBasicBlockDataKeeper::new()`.

### `basic_bootloader/src/bootloader/block_flow/zk/pre_tx_loop.rs`
- Extend `ZKHeaderStructurePreTxOp::pre_op()` to peek at the oracle stream and consume all leading `0x7c` transactions.
- For each `0x7c` transaction: decode it, check version byte, check `is_gateway`, invoke verifier, write entry to `ZKBasicBlockDataKeeper::fri_proof_context` via `system.io.set_fri_proof_entry(...)`.
- Handle `MAX_FRI_PROOF_TXS_PER_BLOCK` limit.
- Return the populated `ZKBasicBlockDataKeeper`.

### `basic_bootloader/src/bootloader/block_flow/zk/tx_loop.rs`
- In the main transaction loop, after determining transaction type, add check: if `is_fri_proof()` is true, record as `InvalidTransaction::FriProofTxOutOfOrder` and continue (mirroring the existing `InvalidEncoding` path).

### `basic_system/src/system_implementation/system/io_subsystem.rs`
- Add `fri_proof_context: FriProofBlockContext` field to `FullIO`.
- Implement `set_fri_proof_entry` and `get_fri_proof_entry` for `FullIO`.
- Initialize `fri_proof_context` in `FullIO` constructor.
- `FriProofBlockContext` does not participate in the IO frame/rollback mechanism (proof context is set once before any user transactions and never rolled back).

### `system_hooks/src/addresses_constants.rs`
- Add `pub const FRI_VERIFIER_PRECOMPILE_ADDRESS_LOW: u8 = 0x12;`
- Add `pub const FRI_VERIFIER_PRECOMPILE_ADDRESS: B160 = B160::from_limbs([0x12, 0, 0]);`

### `system_hooks/src/lib.rs`
- Add a new function `add_fri_precompile<S, A>(hooks, system)` that registers the FRI verification precompile at `FRI_VERIFIER_PRECOMPILE_ADDRESS`.
- Call this function from `add_precompiles()` conditionally: only if `system.get_metadata().is_gateway()` is true.

### `system_hooks/src/call_hooks/precompiles/fri_verifier.rs` *(new file)*
- Implement the FRI precompile call hook.
- ABI-decode `proof_index: u32` from calldata.
- Call `system.io.get_fri_proof_entry(proof_index)`.
- ABI-encode and return `(bool, bytes)`.
- Charge `FRI_PRECOMPILE_BASE_COST_ERGS`.

### `forward_system/src/run/mod.rs` / `forward_system/src/system/bootloader.rs`
- In forward execution, the FRI verifier is invoked during the pre-loop. The `prover` crate from `zksync-airbender` is called. The `forward_system/Cargo.toml` must add `prover` as a dependency if it is not already present.
- For call simulation, add `SKIP_FRI_PROOF_VERIFICATION` logic to `BasicBootloaderCallSimulationConfig` (in `basic_bootloader/src/bootloader/config.rs`).

### `zksync_os_interface/src/types.rs`
- Add `fri_proof_results: Vec<FriProofResult>` to `BlockOutput` (or to a new `BlockFriContext` field).
- Define `FriProofResult` (proof_index, verification_ok, serialized_public_inputs) for consumption by the REVM consistency checker.

### `tests/rig/src/chain.rs`
- Thread `fri_proof_results` from `BlockOutput` into the REVM consistency checker block context.

### `tests/instances/TESTING.md`
- Document how to write tests involving FRI proof transactions (gateway-mode block context, proof construction).

---

## 11. Testing Plan

Integration tests are added under `tests/instances/`. Per `tests/instances/TESTING.md`, tests use `TestingFramework` and `execute_block`.

Required test cases:

1. **Happy path**: A block on a gateway instance with one valid `0x7c` transaction. A subsequent EVM transaction calls the FRI precompile at index 0. Verify the precompile returns `(true, <public_inputs>)`.

2. **Multiple proofs**: A block with two valid `0x7c` transactions. Verify precompile calls at index 0 and index 1 return independent correct results.

3. **Failed verification**: A block with a `0x7c` transaction containing an invalid proof. Verify the precompile returns `(false, "")` for that index.

4. **Non-gateway rejection**: A block on an L2 instance (is_gateway = false) with a `0x7c` transaction. Verify it is recorded as `InvalidTransaction::FriProofTxOnNonGateway` and does not populate the FRI context.

5. **Out-of-order rejection**: A block where a `0x7c` transaction appears after a regular L2 transaction. Verify `InvalidTransaction::FriProofTxOutOfOrder` is returned.

6. **Limit exceeded**: A block with `MAX_FRI_PROOF_TXS_PER_BLOCK + 1` FRI proof transactions. Verify the excess transaction is rejected.

7. **Version byte**: A `0x7c` transaction with version byte `0x00` (reserved). Verify `InvalidTransaction::FriProofVersionUnsupported`.

8. **Forward + proving consistency**: Tests 1 and 3 are run with `ZKSYNC_RISC_V_RUN=true` to confirm identical results in proving mode.

---

## 12. Unresolved Questions

The following items require resolution before implementation begins:

- **Verifier API surface**: The exact Rust function signature for the unified verifier in `zksync-airbender` must be confirmed. In particular: is it `no_std`-compatible as-is, or does it require adaptation for the RISC-V target?
- **`FriPublicInputs` layout**: The exact fields and size of the public inputs struct must be agreed with the airbender team and stabilized before the precompile ABI is finalized.
- **Proof payload size bound**: A maximum size for proof payloads must be defined so that the allocator buffer pre-allocation in the pre-loop does not overflow. This is a constant in `basic_bootloader/src/bootloader/constants.rs`.
- **Gas/native cost**: The `FRI_PRECOMPILE_BASE_COST_ERGS` and the native cost of running the verifier in the pre-loop must be benchmarked (see `docs/benchmarking.md`).
- **`BlockMetadataFromOracle` serialization size**: Adding `is_gateway` changes `USIZE_LEN` for `BlockMetadataFromOracle`. All callers that compare or assert on serialized metadata size must be updated.
- **Rollback semantics for `fri_proof_context`**: The decision that `FriProofBlockContext` does not participate in IO frame rollback must be explicitly confirmed. If a panic occurs mid-block after FRI proof processing, the context is discarded with the block. This is the correct behavior (the context is per-block, not per-tx) but must be documented in the IO subsystem implementation.
