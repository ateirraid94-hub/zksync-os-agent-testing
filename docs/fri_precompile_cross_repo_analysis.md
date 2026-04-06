# FRI Precompile: Cross-Repo Analysis

This document maps every repository that must be touched to implement the FRI proof-verification precompile, explains what needs to change in each, and notes open questions per repo.

---

## Table of Contents

1. [zksync-airbender](#zksync-airbender)
2. [basic_bootloader](#basic_bootloader)
3. [basic_system](#basic_system)
4. [zk_ee](#zk_ee)
5. [zksync-os-revm](#zksync-os-revm)
6. [zksync-os-server](#zksync-os-server)
7. [era-contracts](#era-contracts)

---

## zksync-airbender

**Repo:** `matter-labs/zksync-airbender`

### Purpose in this feature
Airbender is the prover/verifier stack. The FRI precompile ultimately delegates proof verification work to a new AIR delegation inside this repo.

### Files changed

| File | Change |
|------|--------|
| `common_constants/src/delegation_types/fri_verifier.rs` | New delegation type constant for the FRI verifier circuit |
| `cs/src/csr_properties.rs` | Register the new delegation CSR properties |
| `cs/src/delegation/fri_verifier/air.rs` | AIR constraint definition for FRI verification |
| `cs/src/delegation/fri_verifier/mod.rs` | Module entry point wiring the AIR into the delegation framework |
| `full_statement_verifier/src/imports.rs` | Re-export / import the new delegation so the full statement verifier is aware of it |
| `riscv_transpiler/src/vm/delegations/fri_verifier.rs` | RISC-V transpiler support: emit the correct CSR write sequence to invoke the FRI verifier delegation |

### Key decisions
- The delegation type ID must be allocated in `delegation_types` before any other file can reference it — this is the root dependency in the airbender sub-tree.
- The AIR itself encodes the public inputs commitment; the precompile contract reads this back via the `get_fri_proof_entry` system call.

### Open questions
- [ ] Exact delegation type integer value — must not collide with existing types in `delegation_types/`.
- [ ] Whether the AIR needs to be registered in a global delegation registry or only in `full_statement_verifier`.

---

## basic_bootloader

**Repo:** `matter-labs/zksync-os` (`basic_bootloader` crate)

### Purpose in this feature
The bootloader orchestrates block processing. FRI proof transactions must be consumed in a dedicated pre-loop phase before the main transaction loop runs.

### Files changed

| File | Change |
|------|--------|
| `basic_bootloader/src/bootloader/block_flow/tx_loop.rs` | Enforce ordering: reject `0x7c` FRI proof txs that appear after any regular user tx with `InvalidTransaction::FriProofTxOutOfOrder` |
| `basic_bootloader/src/bootloader/block_flow/zk/pre_tx_loop.rs` | Extended `ZKHeaderStructurePreTxOp` pre-loop that drains and verifies all FRI proof txs up to `MAX_FRI_PROOF_TXS_PER_BLOCK` before the main loop starts |

### Key decisions
- Consuming FRI proof txs in the pre-loop (rather than inline) keeps the main loop logic clean and matches how other protocol-level operations (settlement layer chain ID) are handled.
- `MAX_FRI_PROOF_TXS_PER_BLOCK` is a compile-time constant; its value is TBD pending benchmarking.

### Open questions
- [ ] Final value of `MAX_FRI_PROOF_TXS_PER_BLOCK`.
- [ ] Whether the pre-loop should panic or return a block-level error when the limit is exceeded.

---

## basic_system

**Repo:** `matter-labs/zksync-os` (`basic_system` crate)

### Purpose in this feature
Hosts the precompile registry, the I/O subsystem, and the post-init operation that conditionally registers gateway-only precompiles.

### Files changed

| File | Change |
|------|--------|
| `basic_system/src/precompiles/fri_proof_verifier.rs` | New precompile at address `0x12`; input: `uint32 proof_index`; output: `(bool success, bytes publicInputs)` |
| `basic_system/src/precompiles/post_init_op.rs` | Gateway-only registration of the FRI precompile — guarded by `is_gateway` |
| `basic_system/src/system_implementation/system/io_subsystem.rs` | New `FriProofBlockContext` storage + `get_fri_proof_entry(index)` / `set_fri_proof_entry(index, data)` methods on `IOSubsystem`, following the `new_settlement_layer_chain_id_storage` pattern |

### Key decisions
- Address `0x12` was chosen as the next free slot in the EVM precompile range with no collision against `addresses_constants.rs`.
- Gateway-gating in `post_init_op.rs` means non-gateway nodes simply never see this precompile registered; no runtime overhead.

### Open questions
- [ ] Exact ABI encoding of `publicInputs` — raw bytes vs. ABI-encoded `uint256[]`.
- [ ] Whether `get_fri_proof_entry` should return an `Option` or panic on out-of-range index.

---

## zk_ee

**Repo:** `matter-labs/zksync-os` (`zk_ee` crate)

### Purpose in this feature
`zk_ee` defines the system-level traits, error types, and block metadata structures that everything else builds on.

### Files changed

| File | Change |
|------|--------|
| `zk_ee/src/system/errors.rs` | New `InvalidTransaction::FriProofTxOutOfOrder` variant |
| `zk_ee/src/system/metadata/zk_metadata.rs` | New `is_gateway: bool` field in `BlockMetadataFromOracle`; serialized as part of `BLOCK_METADATA_QUERY_ID` and committed in the proof |

### Key decisions
- Placing `is_gateway` in `BlockMetadataFromOracle` ensures it is proof-committed and cannot be forged by a malicious sequencer.
- The `FriProofTxOutOfOrder` error mirrors the style of existing `InvalidTransaction` variants — no tuple payload needed, the variant name is descriptive enough.

### Open questions
- [ ] Serialization position of `is_gateway` within `BLOCK_METADATA_QUERY_ID` — must not shift existing fields.

---

## zksync-os-revm

**Repo:** `matter-labs/zksync-os` (`zksync-os-revm` crate)

### Purpose in this feature
The REVM integration layer translates between the EVM transaction model and zkSync OS internals. It must be taught about the new `0x7c` transaction type.

### Files changed

| File | Change |
|------|--------|
| `zksync-os-revm/src/constants.rs` | Add `FRI_PROOF_TX_TYPE = 0x7c` constant |
| `zksync-os-revm/src/handler.rs` | Handle `0x7c` in the transaction dispatch path |
| `zksync-os-revm/src/transaction/abstraction.rs` | Extend the transaction abstraction to represent FRI proof txs |
| `zksync-os-revm/src/transaction/error.rs` | Surface `FriProofTxOutOfOrder` through the REVM error hierarchy |
| `zksync-os-revm/src/transaction/priority_tx.rs` | Ensure priority tx parsing does not mis-classify `0x7c` bytes |

### Key decisions
- Type byte `0x7c` is the next available slot below the existing service (`0x7d`), upgrade (`0x7e`), and L1 (`0x7f`) transaction types.

### Open questions
- [ ] Whether `0x7c` needs to be excluded from the standard EIP-2718 envelope path or is handled entirely out-of-band.

---

## zksync-os-server

**Repo:** `matter-labs/zksync-os-server`

### Purpose in this feature
The server is the off-chain node software — mempool, sequencer, transaction validators, and shared types. It needs to accept, validate, and sequence FRI proof transactions.

### Files changed

| File | Change |
|------|--------|
| `lib/types/src/transaction/mod.rs` | Register the `FriProof` transaction variant |
| `lib/types/src/transaction/system/fri_proof.rs` | New `FriProofTx` struct: RLP-encoded `chain_id` + `proof_payload`, `from` restricted to `BOOTLOADER_FORMAL_ADDRESS` |
| `lib/types/src/transaction/system/utils.rs` | Shared serialization helpers used by `FriProofTx` |
| `lib/types/src/transaction/encode.rs` | Encoding path for `0x7c` transactions |
| `lib/mempool/src/pool.rs` | Route `0x7c` transactions to the dedicated FRI proof subpool |
| `lib/mempool/src/subpools/fri_proof.rs` | New subpool: ordered queue of pending FRI proof txs, one per proof index |
| `lib/tx_validators/src/fri_proof_validator.rs` | Stateless + stateful validation: `from == BOOTLOADER_FORMAL_ADDRESS`, payload size bounds, proof version byte |
| `lib/sequencer/src/execution/execute_block_in_vm.rs` | Inject queued FRI proof txs at the head of the block (before user txs) when building a gateway block |

### Key decisions
- A dedicated subpool (mirroring the existing subpool architecture) keeps FRI proof txs isolated from user fee-market logic.
- Version byte prefix `0x00` is reserved; `0x01` is the first deployed version — allows future proof format upgrades without a type byte change.

### Open questions
- [ ] Maximum `proof_payload` byte length enforced by `fri_proof_validator`.
- [ ] Whether the sequencer should silently drop stale proof txs or propagate an error when a proof index is already committed.
- [ ] P2P propagation rules: should FRI proof txs be gossiped or only injected locally by the gateway sequencer?

---

## era-contracts

**Repo:** `matter-labs/era-contracts`

### Purpose in this feature
era-contracts contains the L1 and L2 smart contracts for the zkSync Era protocol, including the on-chain verifier, the diamond proxy / state-transition contracts, and settlement-layer infrastructure. For the FRI precompile, this repo is relevant in two ways:

1. **L1 verifier** — if FRI proof public inputs need to be anchored or verified on L1, the `Verifier.sol` / `PlonkVerifier.sol` stack may need to be extended or a new verifier contract deployed.
2. **Settlement layer contracts** — the gateway posts proof data to the settlement layer; any new transaction type or commitment format that flows from the gateway to L1 must be reflected in the corresponding L1 contracts.

### Potentially affected contracts

| Contract / File | Relevance |
|----------------|-----------|
| `l1-contracts/contracts/state-transition/Verifier.sol` (or equivalent) | May need to accept FRI proof public inputs as part of the batch commitment |
| `l1-contracts/contracts/state-transition/chain-deps/facets/Executor.sol` | `commitBatches` / `proveBatches` calldata format may change if FRI public inputs are appended to the batch commitment |
| `l1-contracts/contracts/state-transition/ValidatorTimelock.sol` | TBD — if FRI proof txs affect the validator flow |
| `l2-contracts/contracts/` | TBD — any L2 system contract changes driven by the precompile (unlikely, but audit required) |

### Key questions to resolve before implementation

- [ ] **Does the L1 verifier need to change?** If the FRI proof public inputs are purely intra-block data consumed by the precompile and never committed to L1, no L1 contract changes are needed. If they must be anchored on L1 (e.g., for cross-chain proofs), `Verifier.sol` and `Executor.sol` both need updates.
- [ ] **Batch commitment format** — determine whether `commitBatches` calldata in `Executor.sol` must be extended to carry FRI proof public inputs or a Merkle root thereof.
- [ ] **Gateway-specific facet** — the gateway's diamond may have a dedicated facet for settlement-layer operations. Confirm whether a new facet or an extension to an existing one is required.
- [ ] **ABI versioning** — any calldata format change must be coordinated with the upgrade process (`DiamondProxy` upgrade mechanism) and reflected in the era-contracts upgrade scripts.

### Status

Detailed file-level changes for era-contracts are **TBD** pending resolution of the open questions above. Once the L1 anchoring decision is made, this section should be expanded with the same level of detail as the other repos.
