# FRI Precompile Design

## Overview

This document describes the design for gateway-only FRI proof submission transactions and the corresponding FRI verification precompile in zksync-os. The feature enables the gateway to accept ZK proof payloads as a dedicated transaction type, verify them at block start, and expose results to EVM contracts via a new precompile.

**Scope of this document**: All five repositories involved: zksync-os (primary), zksync-os-server (sequencer), era-contracts (gateway contracts), zksync-os-revm (REVM consistency checker), and zksync-airbender (verifier source, imported not modified).

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

`FriPublicInputs` is a fixed-size struct whose layout matches the output of the unified verifier from zksync-airbender. Its exact fields are determined by the verifier API; see Section 13 for the concrete type.

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

### Repository

`https://github.com/matter-labs/zksync-os-server` (default branch, no special branch required)

### Architecture Context

The sequencer already has an FRI proving pipeline (`node/bin/src/prover_api/fri_proving_pipeline_step.rs`, `fri_job_manager.rs`, `fri_proof_verifier.rs`) for verifying SNARK-wrapped recursion proofs of gateway batches. This is a **different** FRI concern: those are the proofs that the sequencer submits to L1 to prove the gateway's own state transition. The FRI proof transactions (`0x7c`) described in this document are for in-block proof verification of child chains, distinct from the sequencer's own proving pipeline.

### New Transaction Type (`FriProofTx`)

The `ZkEnvelope` enum in `lib/types/src/transaction/mod.rs` currently has four variants annotated with the `#[envelope]` macro:

```rust
pub enum ZkEnvelope {
    #[envelope(ty = 125)]  // 0x7d
    System(SystemTxEnvelope),
    #[envelope(ty = 126)]  // 0x7e
    Upgrade(L1UpgradeEnvelope),
    #[envelope(ty = 127)]  // 0x7f
    L1(L1PriorityEnvelope),
    #[envelope(flatten)]
    L2(L2Envelope),
}
```

A new variant is added:

```rust
#[envelope(ty = 124)]  // 0x7c
FriProof(FriProofTxEnvelope),
```

#### `FriProofTx` struct (`lib/types/src/transaction/fri_proof/tx.rs`, new file)

Modelled after `lib/types/src/transaction/system/tx.rs` (`SystemTx`):

```rust
pub const FRI_PROOF_TX_TYPE_ID: u8 = 124; // 0x7c

pub struct FriProofTx {
    pub chain_id: u64,
    pub proof_payload: Bytes,
}
```

It implements `Transaction`, `Typed2718`, `Encodable2718`, `RlpEcdsaEncodableTx`, `RlpEcdsaDecodableTx`, and `Encodable`/`Decodable`. The RLP fields are `[chain_id, proof_payload]` with no signature fields.

#### `ZkTxType` enum

The `ZkTxType` enum (in `zksync_os_types` or inlined in `lib/types/src/transaction/mod.rs`) gains a `FriProof` variant that corresponds to `ZkEnvelope::FriProof`. The `tx_type()` method returns `FRI_PROOF_TX_TYPE_ID`.

#### `ZkTransaction` handling

The `try_into_recovered()` method on `ZkEnvelope` gains a `FriProof(fri_tx) => Ok(ZkTransaction::from(fri_tx))` arm. Since FRI proof transactions carry no signer (they are operator-constructed), recovery is a no-op returning a fixed address (`BOOTLOADER_FORMAL_ADDRESS`).

### Receipt Support

`lib/types/src/receipt/envelope.rs` defines `ZkReceiptEnvelope`. A new variant is added:

```rust
#[serde(rename = "0x7c")]
FriProof(ReceiptWithBloom<ZkReceipt<T, U>>),
```

The `from_typed()` match arm gains `ZkTxType::FriProof => Self::FriProof(receipt.into())`. The `Encodable2718`/`Decodable2718` implementations gain the corresponding `FRI_PROOF_TX_TYPE_ID` arms.

FRI proof transactions do not generate standard receipts (they are consumed in the pre-loop before the main transaction loop). The receipt for a `0x7c` transaction records only the transaction hash and verification outcome (success/failure); it carries no gas usage, logs, or return data.

### Sequencer (Gateway Mode) Responsibilities

In gateway mode the sequencer must:

1. **Accept proof submission requests**: A new component or extension of an existing pipeline step receives proof payloads from an external source (e.g., a child chain's prover or a dedicated proof submission queue). The source and authorization model are out of scope for this document; the interface is a byte slice containing the versioned proof payload.

2. **Construct `0x7c` transactions**: Using `FriProofTx { chain_id: gateway_chain_id, proof_payload }`, RLP-encode and wrap in `FriProofTxEnvelope`. The resulting envelope is prepended to the block's transaction list before any regular L2 transactions.

3. **Enforce per-block limit**: If the pending proof submission queue contains more than `MAX_FRI_PROOF_TXS_PER_BLOCK` payloads, at most `MAX_FRI_PROOF_TXS_PER_BLOCK` are included in a single block; the rest are deferred to subsequent blocks.

4. **Set `is_gateway = true`** in `BlockMetadataFromOracle` when building the oracle input for each gateway block (see Section 9).

5. **Reject `0x7c` transactions in L2 (non-gateway) mode**: The mempool layer rejects any externally submitted `0x7c` transactions. The sequencer itself does not inject them when `is_gateway = false`.

### `BatchInfoAccumulator` (seal criteria)

`node/bin/src/batcher/seal_criteria.rs` defines `BatchInfoAccumulator` which tracks counters used to decide when to seal a batch. A new field `fri_proof_tx_count: u64` is added and incremented for each `0x7c` transaction in a block. This counter feeds the per-batch limit if needed (distinct from the per-block limit enforced in zksync-os).

### Cargo.toml dependency

No new crate dependencies are required. The `FriProofTx` encoding reuses existing `alloy-rlp` and `alloy` primitives already in the dependency tree.

---

## 6. Gateway Contract Interface (era-contracts)

### Repository

`https://github.com/matter-labs/era-contracts/tree/draft-v31`

### Architecture Context

The gateway runs ZKsync OS and settles its state on L1 via the diamond proxy contracts. When the gateway processes a ZKsync OS batch, `Committer.sol`'s `_commitOneBatchZKsyncOS` is called, which:

1. Validates DA commitment.
2. Computes `batchOutputHash` as `keccak256(chainId, timestamps, daScheme, daCommitment, numberOfLayer1Txs, numberOfLayer2Txs, priorityOpsHash, l2LogsTreeRoot, upgradeTxHash, dependencyRootsRollingHash, slChainId)`.
3. Stores `StoredBatchInfo` with `commitment = batchOutputHash`.
4. When `L1_CHAIN_ID != block.chainid` (i.e., executing on the gateway), relays `StoredBatchInfo` to L1 via `sendToL1`.

The existing `IZKsyncOSDualVerifier` interface (`chain-interfaces/IZKsyncOSDualVerifier.sol`) already anticipates version-tagged verifiers via `fflonkVerifiers(uint32 version)` and `plonkVerifiers(uint32 version)`. This version field maps directly to the `version` byte in the FRI proof payload.

### New: `numberOfFriProofTxs` in `CommitBatchInfoZKsyncOS`

The struct `CommitBatchInfoZKsyncOS` in `chain-interfaces/ICommitter.sol` gains a new field:

```solidity
uint256 numberOfFriProofTxs;
```

This field records the number of `0x7c` transactions executed in the batch. It is included in the `batchOutputHash` computation so that the proof commits to the FRI proof count.

#### Updated `batchOutputHash` in `_commitOneBatchZKsyncOS`

```solidity
bytes32 batchOutputHash = keccak256(abi.encodePacked(
    _newBatch.chainId,
    _newBatch.firstBlockTimestamp,
    _newBatch.lastBlockTimestamp,
    uint256(_newBatch.daCommitmentScheme),
    _newBatch.daCommitment,
    _newBatch.numberOfLayer1Txs,
    _newBatch.numberOfLayer2Txs,
    _newBatch.numberOfFriProofTxs,   // NEW
    _newBatch.priorityOperationsHash,
    _newBatch.l2LogsTreeRoot,
    _expectedSystemContractUpgradeTxHash,
    _newBatch.dependencyRootsRollingHash,
    _newBatch.slChainId
));
```

### New: FRI Proof Submission Entry Point

A new L1/gateway entry point is required for submitting proof payloads to the gateway. Two candidate designs are described; one must be selected:

**Option A — Priority transaction route**: An authorized party calls a new function on the gateway's `IMailbox` implementation (or a dedicated `IFriProofMailbox` interface):

```solidity
function requestFriProofVerification(
    uint256 _chainId,
    bytes calldata _versionedProofPayload
) external payable returns (bytes32 canonicalTxHash);
```

This enqueues a priority L1→gateway transaction. The gateway sequencer detects this priority request in the priority queue and converts it into a `0x7c` transaction in the next gateway block, using the payload bytes directly. The `chain_id` field in the `0x7c` transaction is the gateway's own chain ID, not the submitting chain's ID (the proof is verified on the gateway; the `_chainId` parameter is informational for routing).

**Option B — Sequencer-injected route (no L1 call)**: The sequencer accepts proof payloads out-of-band (e.g., via a dedicated HTTP endpoint). No L1 function call is required. The sequencer constructs `0x7c` transactions autonomously in gateway mode. This approach requires no era-contracts changes beyond the `CommitBatchInfoZKsyncOS` extension.

In either case, the `proof_payload` bytes in the `0x7c` transaction are identical to what was submitted by the caller: version byte followed by raw proof data.

### New L2 System Log: `FRI_PROOF_TX_COUNT_LOG_KEY`

A new system log key `FRI_PROOF_TX_COUNT_LOG_KEY` is added to `system-contracts/contracts/Constants.sol` (the `SystemLogKey` enum). This log is emitted by the bootloader at block finalization and carries the count of processed `0x7c` transactions in the block. The value is extracted in `Committer.sol`'s `_processL2LogsZKsyncOS` (or equivalent) and used to populate `numberOfFriProofTxs` in the batch commitment.

### Affected Files (era-contracts)

| File | Change |
|------|--------|
| `l1-contracts/contracts/state-transition/chain-interfaces/ICommitter.sol` | Add `numberOfFriProofTxs: uint256` to `CommitBatchInfoZKsyncOS` |
| `l1-contracts/contracts/state-transition/chain-deps/facets/Committer.sol` | Include `numberOfFriProofTxs` in `batchOutputHash`; extract from system logs |
| `system-contracts/contracts/Constants.sol` | Add `FRI_PROOF_TX_COUNT_LOG_KEY` to `SystemLogKey` enum |
| `l1-contracts/contracts/state-transition/chain-interfaces/IMailbox.sol` *(optional, Option A only)* | Add `requestFriProofVerification` function signature |
| `l1-contracts/contracts/state-transition/chain-deps/facets/Mailbox.sol` *(optional, Option A only)* | Implement `requestFriProofVerification` |

---

## 7. REVM Consistency Checker Replay Strategy (zksync-os-revm)

### Repository

`https://github.com/matter-labs/zksync-os-revm/tree/vv-new-version`

### Architecture Context

`zksync-os-revm` provides REVM-based replay of ZKsync OS blocks for the consistency checker (`tests/rig/src/revm_consistency_checker.rs`). It defines:

- `ZkSpecId` (`src/spec.rs`): `AtlasV1`, `AtlasV2`, `AtlasV3` — determines active precompiles and EVM rules.
- `ZKsyncPrecompiles` (`src/precompiles.rs`): routes calls to custom precompile implementations by spec + address.
- `ZkTxTr` trait (`src/transaction/abstraction.rs`): extends REVM's `Transaction` with ZKsync-specific fields; `is_service_tx()` causes the handler to skip nonce/balance validation.
- `ZKsyncHandler` (`src/handler.rs`): the main execution handler; checks `is_service_tx()` to bypass EVM execution for service txs.

### New Transaction Type Support

#### `FRI_PROOF_TX_TYPE` constant

Add to `src/transaction/priority_tx.rs`:

```rust
pub const FRI_PROOF_TX_TYPE: u8 = 0x7c;
```

#### `ZkTxTr` trait extension

Add to `src/transaction/abstraction.rs`:

```rust
fn is_fri_proof_tx(&self) -> bool {
    self.tx_type() == FRI_PROOF_TX_TYPE
}
```

The default implementation checks the type byte. No additional fields are required.

#### `ZKsyncTx` implementation

`ZKsyncTx<T>` inherits the default `is_fri_proof_tx()` implementation via `tx_type()` delegation, so no explicit override is needed.

### Handler: Skip FRI Proof Transactions

In `src/handler.rs`, the `validate_tx_against_state` implementation (which already skips nonce/balance checks for `is_service_tx()`) is extended:

```rust
if tx.is_service_tx() || tx.is_fri_proof_tx() {
    return Ok(());
}
```

FRI proof transactions have no EVM execution semantics. The handler returns a successful (empty) execution result without entering the EVM interpreter, similar to service transactions. The REVM consistency checker's transaction loop skips `0x7c` transactions when iterating the list for EVM replay.

### New Spec: `AtlasV4` (Gateway)

A new `ZkSpecId::AtlasV4` variant is added to `src/spec.rs` to represent the gateway protocol version that activates the FRI precompile:

```rust
pub enum ZkSpecId {
    AtlasV1,
    AtlasV2,
    AtlasV3,
    #[default]
    AtlasV4,  // gateway-capable version with FRI precompile
}
```

The `into_eth_spec()` mapping remains `SpecId::CANCUN` for all variants. The `is_enabled_in` ordering is updated. The ZKsync OS server maps its `ExecutionVersion` to `ZkSpecId::AtlasV4` for gateway blocks.

### FRI Precompile Mock

#### Address constant

Add to `src/constants.rs`:

```rust
pub const FRI_VERIFIER_PRECOMPILE_ADDRESS: Address =
    address!("0000000000000000000000000000000000000012");
```

#### New module `src/precompiles/v4.rs`

Defines `v4::fri_verifier::fri_verifier_precompile_call<CTX>` which:

1. Decodes `proof_index: u32` from calldata (ABI-encoded `uint32`).
2. Looks up the pre-verified result for `proof_index` from the block context (see below).
3. On hit: ABI-encodes `(true, serialized_public_inputs)` and returns success.
4. On miss (index out of range): returns a revert with a fixed selector.
5. On failed verification: ABI-encodes `(false, bytes(""))` and returns success.

#### Threading FRI proof results through REVM block context

The REVM consistency checker supplies FRI proof results from `BlockOutput::fri_proof_results`. These need to be accessible inside the precompile call. The cleanest mechanism is a new context trait:

```rust
// src/api/exec.rs
pub trait ZkContextTr:
    ContextTr<
        Journal: JournalTr<State = EvmState>,
        Tx: ZkTxTr,
        Cfg: Cfg<Spec = ZkSpecId>,
        Block: ZkBlockTr,  // NEW: requires fri_proof_results access
    >
{}

// src/api/block_ext.rs (new file)
pub trait ZkBlockTr {
    fn fri_proof_results(&self) -> &[FriProofResult];
}
```

`FriProofResult` (defined in `src/api/block_ext.rs` or imported from `zksync_os_interface`) mirrors `FriProofEntry`:

```rust
pub struct FriProofResult {
    pub proof_index: u32,
    pub verification_ok: bool,
    pub serialized_public_inputs: Vec<u8>,
}
```

The REVM consistency checker wraps `BlockEnv` with a custom type that implements `ZkBlockTr` and carries the `Vec<FriProofResult>` populated from `BlockOutput::fri_proof_results` before each block replay.

#### `ZKsyncPrecompiles::run()` routing

In `src/precompiles.rs`, `maybe_call_custom_precompile` gains a new arm for `AtlasV4`:

```rust
ZkSpecId::AtlasV4 => match precompile_address {
    CONTRACT_DEPLOYER_ADDRESS => { ... },
    MINT_BASE_TOKEN_HOOK_ADDRESS => { ... },
    SET_BYTECODE_ON_ADDRESS_HOOK_ADDRESS => { ... },
    L1_MESSENGER_HOOK_ADDRESS => { ... },
    FRI_VERIFIER_PRECOMPILE_ADDRESS => {
        v4::fri_verifier::fri_verifier_precompile_call as CustomPrecompile<_>
    },
    _ => return None,
},
```

#### `warm_addresses` update

The `warm_addresses()` implementation for `ZkSpecId::AtlasV4` includes `FRI_VERIFIER_PRECOMPILE_ADDRESS` in the warmed set.

### Affected Files (zksync-os-revm)

| File | Change |
|------|--------|
| `src/spec.rs` | Add `AtlasV4` variant to `ZkSpecId`; update `into_eth_spec`, `is_enabled_in`, `FromStr` |
| `src/transaction/priority_tx.rs` | Add `FRI_PROOF_TX_TYPE: u8 = 0x7c` |
| `src/transaction/abstraction.rs` | Add `fn is_fri_proof_tx(&self) -> bool` to `ZkTxTr`; implement default |
| `src/handler.rs` | Skip validation for `is_fri_proof_tx()` alongside `is_service_tx()` |
| `src/precompiles.rs` | Add `AtlasV4` arm; route `FRI_VERIFIER_PRECOMPILE_ADDRESS` to new precompile |
| `src/constants.rs` | Add `FRI_VERIFIER_PRECOMPILE_ADDRESS` |
| `src/precompiles/v4.rs` *(new)* | `AtlasV4`-specific custom precompile dispatch |
| `src/precompiles/v4/fri_verifier.rs` *(new)* | FRI precompile mock implementation |
| `src/api/block_ext.rs` *(new)* | `ZkBlockTr` trait; `FriProofResult` type |
| `src/api/exec.rs` | Extend `ZkContextTr` bound to require `ZkBlockTr` |

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
- **Proof submission authorization (era-contracts Option A vs B)**: The choice between L1-triggered priority transaction route and sequencer-injected route must be decided. Option A gives on-chain auditability; Option B is simpler to deploy.
- **`ZkSpecId::AtlasV4` naming and alignment**: The new gateway spec version in `zksync-os-revm` must be coordinated with the sequencer's `ExecutionVersion` mapping so that REVM consistency checks use `AtlasV4` precisely when the block is a gateway block with `is_gateway = true`.
- **`numberOfFriProofTxs` in `CommitBatchInfoZKsyncOS`**: Confirm whether proof count needs to be open on L1 or whether it is implicitly covered by `newStateCommitment` (which commits to the entire state including `fri_proof_context`). If the latter, the new field may be omitted from the commitment hash.

---

## 13. zksync-airbender Verifier API Reference

### Repository

`https://github.com/matter-labs/zksync-airbender/tree/dev`

This repository is **imported, not modified**. It provides the `verifier` crate which is the unified FRI proof verifier for the RISC-V ZKsync OS execution environment.

### `no_std` Compatibility

`verifier/src/lib.rs` declares `#![cfg_attr(not(any(test, feature = "replace_csr")), no_std)]`. The verifier is `no_std`-compatible in production builds. No adaptations are required for the RISC-V target provided the `replace_csr` feature is not enabled.

### Primary API

```rust
// verifier/src/lib.rs

pub unsafe fn verify(
    proof_state_dst: &mut ProofOutput<TREE_CAP_SIZE, NUM_COSETS, NUM_DELEGATION_CHALLENGES,
                                      NUM_AUX_BOUNDARY_VALUES, NUM_MACHINE_STATE_PERMUTATION_CHALLENGES>,
    proof_input_dst: &mut ProofPublicInputs<NUM_STATE_ELEMENTS>,
)
```

This function:
1. Reads proof data from the non-determinism source (CSR registers in RISC-V mode; a thread-local iterator in `replace_csr`/test mode).
2. Verifies the FRI-based proof against a fixed compiled circuit geometry (`VERIFIER_COMPILED_LAYOUT`).
3. Writes the decoded public inputs into `proof_input_dst`.
4. Writes proof output (delegation, state linkage) into `proof_state_dst`.

### Public Input Type

```rust
// verifier_common/src/lib.rs

pub struct ProofPublicInputs<const NUM_STATE_ELEMENTS: usize> {
    pub input_state_variables:  [Mersenne31Field; NUM_STATE_ELEMENTS],
    pub output_state_variables: [Mersenne31Field; NUM_STATE_ELEMENTS],
}
```

`NUM_STATE_ELEMENTS` is the concrete value `VERIFIER_COMPILED_LAYOUT.public_inputs.len() / 2`, resolved at compile time from the generated layout in `verifier/src/generated/circuit_layout.rs`. The value is fixed for a given circuit geometry.

`Mersenne31Field` is a 32-bit prime field element. Each `Mersenne31Field` is represented as a `u32`. The serialized byte size of `FriPublicInputs` is therefore `2 * NUM_STATE_ELEMENTS * 4` bytes.

### Concrete Type Aliases (in zksync-os integration)

The zksync-os integration imports these aliases from the `verifier` crate:

```rust
use verifier::{ConcreteProofOutput, ConcreteProofPublicInputs};
// ConcreteProofOutput = ProofOutput<TREE_CAP_SIZE, NUM_COSETS, ...>
// ConcreteProofPublicInputs = ProofPublicInputs<NUM_STATE_ELEMENTS>
```

`FriPublicInputs` in `zk_ee/src/common_structs/fri_proof_context.rs` is a type alias or newtype over `ConcreteProofPublicInputs`.

### Non-Determinism Source for Forward Execution

In RISC-V proving mode, proof data is supplied via CSR reads (the `DefaultNonDeterminismSource`). In forward mode on a host machine, the `replace_csr` feature enables a thread-local iterator:

```rust
// In forward_system, before calling verify():
full_statement_verifier::verifier_common::prover::nd_source_std::set_iterator(
    oracle_data.into_iter()
);
// Then call:
verifier::verify(&mut proof_state_dst, &mut proof_input_dst);
```

The proof bytes from the `0x7c` transaction payload are converted to the oracle data format using `execution_utils::ProgramProof::to_metadata_and_proof_list` and `execution_utils::generate_oracle_data_from_metadata_and_proof_list` — the same utilities already used in `node/bin/src/prover_api/fri_proof_verifier.rs` in zksync-os-server.

### Version-to-Circuit Mapping (Future)

The `version` byte in the proof payload identifies the circuit geometry. Version `0x01` corresponds to the current compiled layout in `verifier/src/generated/`. Future versions would require a new compiled layout and verifier. The verifier dispatch in `pre_tx_loop.rs` is keyed on this byte; an unsupported version triggers `FriProofVersionUnsupported`.
