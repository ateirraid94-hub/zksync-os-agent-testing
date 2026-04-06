# FRI Precompile — Cross-Repo Analysis

> **Scope of this document:** Analysis of changes required across all repositories to support the FRI proof precompile. Code implementation is tracked separately in `fri_precompile_implementation_plan.md`.

## Affected Repositories

| Repository | Role |
|---|---|
| [zksync-os](https://github.com/matter-labs/zksync-os) | Core implementation: FRI precompile system call, bootloader integration, metadata |
| [zksync-os-server](https://github.com/matter-labs/zksync-os-server) | Sequencer support for accepting, validating, and including the new tx type in blocks (gateway mode only) |
| [zksync-os-revm](https://github.com/matter-labs/zksync-os-revm/tree/vv-new-version) | REVM consistency checker support for replaying the new tx type |
| [zksync-airbender](https://github.com/matter-labs/zksync-airbender/tree/dev) | ZK circuit delegation handler and AIR for in-circuit FRI verification |

---

## 1. zksync-os (This Repository)

The core precompile lives here. Changes required:

- **Precompile dispatcher** (`basic_system/src/precompiles/`): register the FRI verifier precompile address and route calls to the verification logic.
- **Bootloader tx loop** (`basic_bootloader/src/bootloader/block_flow/tx_loop.rs`, `pre_tx_loop.rs`): recognise the new system transaction type and route it through the precompile path rather than the normal EVM execution path.
- **IO subsystem** (`basic_system/src/system_implementation/system/io_subsystem.rs`): expose the delegation CSR write so the precompile can invoke the airbender delegation.
- **Metadata** (`zk_ee/src/system/metadata/zk_metadata.rs`): any per-block or per-tx metadata fields needed to record proof submission.

---

## 2. zksync-os-server

The sequencer (gateway mode only) must be able to accept, validate, queue, and include `FriProof` system transactions in blocks.

### New types required

- **`FriProofTx` / `FriProofTxEnvelope`** (`lib/types/src/transaction/system/fri_proof.rs`, `lib/types/src/transaction/mod.rs`): wire format for the new transaction type. Must carry the raw FRI proof blob and any auxiliary fields (e.g. circuit ID, claimed public inputs).
- **`ZkEnvelope::FriProof` variant** (`lib/types/src/transaction/encode.rs`): the envelope discriminant that lets the rest of the server distinguish this tx from priority and L2 transactions.

### Mempool / subpool

- **`FriProofSubpool`** (`lib/mempool/src/subpools/fri_proof.rs`): a dedicated gateway-only FIFO subpool. Proposed initial capacity: 8 pending proof transactions. Capacity must stay in sync with the bootloader constant that limits proof txs per block.
- **`pool.rs`** (`lib/mempool/src/pool.rs`): register the new subpool and plumb it into the main mempool routing logic.

### Validation

- **`FriProofValidationError`** (`lib/tx_validators/src/fri_proof_validator.rs`): at minimum three invariants must be checked before a proof tx is admitted to the subpool:
  1. Caller is the authorised proof submitter address (TBD — see open questions).
  2. Proof blob is non-empty and its length is a multiple of the expected field-element size.
  3. The transaction is only accepted when the node is operating in gateway mode (`is_gateway == true`).

### Sequencer block building

- **`execute_block_in_vm.rs`** (`lib/sequencer/src/execution/execute_block_in_vm.rs`): drain up to N proof transactions from `FriProofSubpool` per block and inject them ahead of normal L2 transactions, matching the order expected by the bootloader tx loop.

### Open questions

| # | Question | Owner |
|---|---|---|
| 1 | What is the ingress path for proof submissions — direct RPC, L1 log, or internal sequencer trigger? | TBD |
| 2 | How is `is_gateway` configuration surfaced to the validator — node config flag, chain config, or runtime detection? | TBD |
| 3 | Should the subpool capacity of 8 be derived from the same constant used in the bootloader, or are they intentionally independent? | TBD |

---

## 3. zksync-os-revm

REVM is used as a consistency checker to replay transactions executed by the VM. It must be able to handle the new `FriProof` transaction type without treating it as a normal EVM transaction.

### New constant and predicate

- **`FRI_PROOF_TRANSACTION_TYPE`** (`src/constants.rs`): the numeric EIP-2718 type byte reserved for FRI proof transactions. Must match the value used by `zksync-os-server`.
- **`is_fri_proof_tx()`** (`src/transaction/abstraction.rs`): predicate that identifies an incoming transaction as a FRI proof tx based on its type byte, used to gate the bypass logic below.

### Handler bypass

FRI proof transactions bypass normal EVM execution. The following four handler methods in `src/handler.rs` must short-circuit when `is_fri_proof_tx()` is true:

1. **Nonce validation** — proof txs are not subject to account nonce ordering.
2. **Balance / gas pre-check** — proof txs do not deduct gas from a sender balance.
3. **EVM call dispatch** — no EVM frame is created; execution is delegated to the precompile via the delegation CSR.
4. **Refund / post-execution accounting** — no gas refund is issued.

### Caller restriction

- **`BOOTLOADER_FORMAL_ADDRESS`** (`src/constants.rs`): the only permitted `msg.sender` for a FRI proof tx when replayed through REVM. Any other caller must produce **`InvalidFriProofTxCaller`** (`src/transaction/error.rs`).

### Gap / pending work

- The precompile mock used during REVM replay is not yet implemented. Until the airbender circuit is wired up, REVM must either skip proof verification entirely (returning a fixed success result) or call a stub that validates only the structural invariants of the proof blob. The chosen approach must be documented before the branch is merged.

---

## 4. zksync-airbender

The FRI verifier runs inside a ZK delegation circuit. The RISC-V VM delegates proof verification to a specialised coprocessor via a CSR write; airbender provides the circuit that proves the coprocessor executed correctly.

### New delegation CSR

- **`FRI_VERIFIER_DELEGATION_CSR = 0x7CC`** (`common_constants/src/delegation_types/fri_verifier.rs`, `cs/src/csr_properties.rs`): the CSR address the RISC-V core writes to in order to trigger the FRI verifier delegation. Must be added to the CSR property table so the transpiler and constraint system recognise it.
- **Delegation type ID `1996`**: the numeric identifier for this delegation in the type registry (`common_constants/src/delegation_types/fri_verifier.rs`).

### RAM ABI

The proof and its metadata are passed through a shared RAM region. Required ABI offsets (exact layout TBD — mark as TODO until the proof format is finalised):

| Offset | Field | Notes |
|---|---|---|
| 0x00 | `proof_len` | u32, length of the proof blob in field elements |
| 0x04 | `public_inputs_ptr` | pointer to claimed public inputs |
| 0x08 | `proof_ptr` | pointer to the raw proof blob |
| TBD | additional fields | TBD pending proof format decision |

### Handler and circuit

- **`FriVerifierDelegationHandler`** (`riscv_transpiler/src/vm/delegations/fri_verifier.rs`): RISC-V transpiler hook that emits the delegation witness when the transpiler encounters a CSR write to `0x7CC`.
- **`FriVerifierDelegationCircuit` AIR** (`cs/src/delegation/fri_verifier/air.rs`, `cs/src/delegation/fri_verifier/mod.rs`): the algebraic intermediate representation for the FRI verifier coprocessor. Constraints must cover:
  - Correct parsing of the proof blob from the RAM ABI.
  - All rounds of the FRI query phase.
  - Merkle authentication paths for each queried leaf.
  - Final public-input consistency check.
- **`full_statement_verifier` imports** (`full_statement_verifier/src/imports.rs`): register the new circuit so the top-level statement verifier includes the FRI delegation proof.

### Open questions

| # | Question | Owner |
|---|---|---|
| 1 | What is the exact wire format of the FRI proof blob (field element size, endianness, query structure)? | TBD |
| 2 | What recursion depth / number of FRI rounds is targeted, and is this fixed per circuit or parameterised? | TBD |
| 3 | Has capacity profiling been done for the delegation circuit — does it fit within the row budget of a single delegation segment? | TBD |
| 4 | Is a trusted setup required for the inner FRI circuit, or is it purely transparent? | TBD |
