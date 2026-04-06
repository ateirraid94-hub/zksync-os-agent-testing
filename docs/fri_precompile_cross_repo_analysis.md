# FRI Precompile: Cross-Repository Design Analysis

> Deep analysis notes covering all repositories impacted by the FRI proof precompile feature.
> Five sections: current repo (`zksync-os`) + 4 external repositories.

---

## Table of Contents

1. [zksync-os (current repo) — Full Change Inventory](#1-zksync-os-current-repo)
2. [zksync-protocol — Protocol-Level Considerations](#2-zksync-protocol)
3. [era-contracts — L1/L2 Contract Considerations](#3-era-contracts)
4. [zkevm_circuits / zksync-crypto — Proof System Considerations](#4-zkevm_circuits--zksync-crypto)
5. [zk_evm — VM Execution Considerations](#5-zk_evm)

---

## 1. `zksync-os` (current repo)

### Summary
All in-repo changes needed to introduce the FRI proof transaction type (`0x7c`), the `fri_proof_context` storage system, the FRI precompile at address `0x12`, and gateway-only gating.

### Transaction Layer

| File | Change | Notes |
|------|--------|-------|
| `basic_bootloader/src/bootloader/transaction/mod.rs` | Add `TransactionType::FriProof = 0x7c` variant | Must sit below service `0x7d`, upgrade `0x7e`, L1 `0x7f` |
| `basic_bootloader/src/bootloader/transaction/fri_proof_tx.rs` | **New file** — RLP decoder for `(chain_id, proof_payload)` + `from == BOOTLOADER_FORMAL_ADDRESS` enforcement | Mirror `service_tx.rs` structure |
| `basic_bootloader/src/bootloader/block_flow/zk/pre_tx_loop.rs` | Consume all `0x7c` txs before main loop; enforce `MAX_FRI_PROOF_TXS_PER_BLOCK` | New constant, suggest `8` — validate with prover team |
| `basic_bootloader/src/bootloader/block_flow/zk/tx_loop.rs` | Reject `0x7c` if encountered after regular txs → `InvalidTransaction::FriProofTxOutOfOrder` | Ordering invariant |
| `basic_bootloader/src/bootloader/errors.rs` | Add `FriProofTxOutOfOrder`, `TooManyFriProofTxs`, `FriProofInvalidSender` | |

### Storage / Context Layer

| File | Change | Notes |
|------|--------|-------|
| `basic_system/src/system_implementation/system/io_subsystem.rs` | Add `get_fri_proof_entry(index: u32) -> Option<FriProofEntry>` + `set_fri_proof_entry(...)` to `IOSubsystem` trait | Exact pattern of `new_settlement_layer_chain_id_storage` |
| `basic_system/src/system_implementation/system/block_data_keeper.rs` | Add `FriProofBlockContext` struct + integrate into `ZKBasicBlockDataKeeper` | Holds `Vec<FriProofEntry>` bounded by `MAX_FRI_PROOF_TXS_PER_BLOCK` |
| `zk_ee/src/system/fri_proof_entry.rs` | **New file** — `FriProofEntry { proof_index: u32, public_inputs: Vec<u8>, verified: bool }` | Shared type across subsystems |
| `zk_ee/src/system/mod.rs` | Re-export `FriProofEntry` | |

### Precompile

| File | Change | Notes |
|------|--------|-------|
| `basic_system/src/system_implementation/precompiles/addresses_constants.rs` | Add `FRI_VERIFIER_PRECOMPILE_ADDRESS: Address = 0x12` | Confirmed no conflict with existing range |
| `basic_system/src/system_implementation/precompiles/fri_verifier.rs` | **New file** — precompile impl: reads `proof_index: u32` from calldata, calls `system.io.get_fri_proof_entry(proof_index)`, ABI-encodes `(bool success, bytes publicInputs)` | |
| `basic_system/src/system_implementation/precompiles/mod.rs` | Register `FriVerifierPrecompile` | |
| `basic_system/src/system_implementation/post_init_op.rs` | Gate registration on `block_metadata.is_gateway` | Non-gateway nodes must not expose the precompile |

### Gateway Gating

| File | Change | Notes |
|------|--------|-------|
| `zk_ee/src/system/metadata/zk_metadata.rs` | Add `is_gateway: bool` to `BlockMetadataFromOracle` | Serialized as part of `BLOCK_METADATA_QUERY_ID`; committed into the proof |
| `zk_ee/src/system/metadata/mod.rs` | Update serialization round-trip tests | |

### Unresolved Questions (zksync-os)

1. **`MAX_FRI_PROOF_TXS_PER_BLOCK` value** — needs prover team input; currently proposed `8`.
2. **`proof_payload` max size** — no size cap defined yet; needed to bound witness generation cost.
3. **Gas accounting for `0x7c` txs** — do they consume block gas or are they free like service txs?
4. **`version` byte in `proof_payload`** — `0x00` reserved, `0x01` first version; upgrade path not defined.
5. **`FriProofBlockContext` persistence** — cleared per-block; confirm no cross-block accumulation needed.
6. **Precompile return encoding for failure** — return `(false, "")` or revert? ABI consumer compatibility unclear.

---

## 2. `zksync-protocol`

> Repository: `matter-labs/zksync-protocol`
> Role: Canonical protocol specification, transaction encoding schemas, and versioning.

### Design Considerations

#### Transaction Type Registry
- **`0x7c` must be formally registered** in the protocol transaction type table. The existing registry (see `crates/zksync_protocol/src/transaction/mod.rs`) lists types `0x00`–`0x7f`. The FRI proof tx occupies the last unallocated slot below the system-reserved range (`0x7d`–`0x7f`). This must be locked in a protocol version bump to prevent future collision.
- The protocol version bump also governs the minimum node version that must understand `0x7c`. Older nodes receiving a block containing `0x7c` txs must hard-reject it, not silently ignore it.

#### Encoding Schema
- The RLP schema `[chain_id, version, proof_payload]` must be canonically specified here so that all client implementations (Go, Rust, TypeScript SDKs) encode identically.
- `chain_id` inclusion prevents cross-chain proof replay. This mirrors how standard EIP-155 txs work, but the protocol spec must explicitly call out that `v/r/s` fields are **absent** — no ECDSA signature, sender authenticity is enforced purely by `from == BOOTLOADER_FORMAL_ADDRESS`.
- The `version` byte namespace (`0x00` = reserved/test, `0x01` = first production) must be defined in the protocol spec as a forward-compatibility hook for proof format upgrades.

#### Block Header Changes
- If `is_gateway` is committed into the proof via `BLOCK_METADATA_QUERY_ID`, the protocol spec for block metadata serialization must be updated. Specifically, the bit-packing or field ordering in `BlockMetadataFromOracle` determines the proof's public input layout. **Any reordering breaks verifier contracts on L1.**
- Recommendation: add `is_gateway` as the last field to avoid shifting existing field offsets.

#### Protocol Version Gating
- The FRI precompile must be **feature-gated behind a protocol version number**. The protocol repo defines the mapping from version numbers to feature sets. A `fri_precompile_v1` feature flag should be introduced so that:
  - Nodes below the activation version do not expose address `0x12`.
  - The bootloader's pre-tx loop is a no-op if the protocol version predates the feature.

#### Unresolved Questions (zksync-protocol)

7. **Protocol version number** for `0x7c` activation — which version slot is available?
8. **`proof_payload` schema versioning** — is the `version` byte sufficient, or does the protocol need a separate proof format registry?
9. **Cross-chain replay protection** — is `chain_id` alone sufficient, or is a `block_number` nonce needed in `proof_payload`?

---

## 3. `era-contracts`

> Repository: `matter-labs/era-contracts`
> Role: L1 smart contracts (diamond proxy, verifier, governance) and L2 system contracts.

### Design Considerations

#### L1 Verifier Contract
- The existing `Verifier.sol` (or `PlonkVerifier.sol`) verifies ZK proofs submitted to L1. If the FRI precompile produces public inputs that must later be verified on L1, the **L1 verifier must be updated or a new `FRIVerifier.sol` deployed** to handle the FRI-specific public input layout.
- Key question: is the FRI precompile result *consumed only on L2* (by smart contracts calling `0x12`), or does the public input need to be relayed to L1 for settlement? If the latter, a new L1 verification path is mandatory.
- The diamond proxy's `IVerifier` interface would need a new facet or an upgrade to the existing verifier facet.

#### L2 System Contracts
- `ContractDeployer`, `MsgValueSimulator`, and other system contracts live at low addresses (`0x01`–`0x0a` range predominantly). Address `0x12` is in the precompile range but above the current system contract ceiling. Confirm via `SystemContractsCaller` and the system contract address constants in `contracts/zksync/contracts/Constants.sol` that `0x12` is truly unoccupied on the L2 side.
- If any L2 system contract currently hard-codes a range check like `address <= 0x10` for precompile detection, that check must be updated to `address <= 0x12` (or ideally replaced with a registry lookup).

#### Governance / Upgrade Path
- Deploying a new precompile requires a **governance proposal** through the `Governance.sol` contract (timelock). The upgrade path must:
  1. Deploy updated bootloader bytecode (containing `0x7c` handling and `0x12` registration).
  2. Update the `AllowList` or equivalent if the precompile needs explicit caller whitelisting.
  3. Emit a protocol upgrade event recognized by the state transition contract.
- The `BaseZkSyncUpgrade` abstract contract in `era-contracts` orchestrates these steps. A new upgrade script targeting the FRI precompile feature must be authored here.

#### Bootloader Formal Address
- `BOOTLOADER_FORMAL_ADDRESS` is defined both in the bootloader (Rust) and in `contracts/zksync/contracts/Constants.sol`. The check `from == BOOTLOADER_FORMAL_ADDRESS` for `0x7c` txs relies on this constant being identical in both places. This should be enforced via a test in `era-contracts` that reads the Rust constant and compares it to the Solidity constant.

#### L1→L2 Message Passing for Proof Results
- If L2 contracts need to emit FRI proof results back to L1 (e.g., for optimistic challenge resolution), the `Mailbox` facet's `_requestL2Transaction` must be reviewed for compatibility with proofs as message payloads. Current max calldata sizes in the mailbox may be insufficient for raw proof data.

#### Unresolved Questions (era-contracts)

10. **New `FRIVerifier.sol` vs. upgrade to existing verifier** — architectural decision needed from the contracts team.
11. **Address `0x12` L2 system contract registry** — needs explicit audit of `Constants.sol` and `SystemContractHelper.sol`.
12. **Governance timeline** — upgrade proposals have mandatory timelocks; FRI precompile activation cannot be instant.

---

## 4. `zkevm_circuits` / `zksync-crypto`

> Repositories: `matter-labs/zkevm_circuits`, `matter-labs/zksync-crypto`
> Role: The actual FRI/STARK proof circuits, the prover backend, and cryptographic primitives.

### Design Considerations

#### Public Input Layout
- The FRI precompile's output `bytes publicInputs` must match **exactly** what the circuit exposes as its public input vector. The circuit definition in `zkevm_circuits` is the source of truth for field ordering and encoding.
- If public inputs are field elements over a non-standard field (e.g., Goldilocks for Plonky2-style FRI), they cannot be directly ABI-encoded as `uint256`. A serialization convention (little-endian u64 packing, or conversion to `uint256`) must be agreed upon and tested end-to-end.
- The `proof_payload` passed to the bootloader via the `0x7c` tx is the serialized proof. The circuit's deserialization code must be reachable from within the zkOS witness generation path — this requires a Rust dependency on the circuit library, which may introduce compile-time cost and binary size increases.

#### Witness Generation
- The bootloader processes `0x7c` txs in the pre-tx loop, which runs inside the prover's witness generation context. This means **verifying the FRI proof inside the witness generator is itself being proven**. This is a recursive proving setup.
- The `zkevm_circuits` repo must expose a circuit that accepts a serialized FRI proof as a witness input and produces a boolean `verified` output as a public signal. This is a non-trivial circuit design task.
- Two options exist:
  - **Native verification**: verify the inner FRI proof natively in the outer circuit (expensive, but no trusted setup).
  - **Aggregation / recursion**: use a recursive SNARK to compress the inner proof into a constant-size proof that the outer circuit can verify cheaply. This is the recommended path but requires infrastructure in `zksync-crypto`.

#### Proof Format Stability
- The `version` byte in `proof_payload` must map to a specific circuit version in `zkevm_circuits`. When the circuit is upgraded (e.g., for a new constraint system or security fix), the `version` byte increments and the precompile must route to the correct verifier implementation.
- A **proof format registry** keyed by `version` byte is needed, likely implemented as a Rust `match` in the bootloader's `fri_proof_tx.rs` decoder.

#### Performance Constraints
- FRI verification inside a ZK circuit is computationally expensive. The `MAX_FRI_PROOF_TXS_PER_BLOCK` constant must be calibrated against the prover's cycle budget for the pre-tx loop. Exceeding the budget causes proof generation failure, not a graceful error.
- The `zkevm_circuits` team must provide a cycle cost estimate for a single FRI proof verification inside the outer circuit before `MAX_FRI_PROOF_TXS_PER_BLOCK` is finalized.

#### Commitment to `is_gateway`
- If `is_gateway` is a new field in `BlockMetadataFromOracle` committed into the proof, the **circuit must explicitly constrain this field**. If it is unconstrained, an adversarial prover could forge the `is_gateway` flag, enabling the FRI precompile on non-gateway nodes. This is a critical security requirement for the `zkevm_circuits` team.

#### Unresolved Questions (zkevm_circuits / zksync-crypto)

(Continuing unresolved question numbering from above)

- **Recursion strategy** — native vs. aggregation for inner FRI proof verification. Decision required before implementation begins.
- **Cycle cost per proof** — needed to bound `MAX_FRI_PROOF_TXS_PER_BLOCK`.
- **`is_gateway` circuit constraint** — must be explicitly added; security-critical.
- **`publicInputs` encoding** — field element serialization convention for ABI consumers.

---

## 5. `zk_evm`

> Repository: `matter-labs/zk_evm`
> Role: The ZK-EVM interpreter — opcode semantics, memory model, precompile dispatch, gas metering.

### Design Considerations

#### Precompile Dispatch
- `zk_evm` contains the `precompile_abi_execute` dispatch table that routes `CALL` instructions to native precompile implementations. Address `0x12` must be added here **in addition to** the registration in `zksync-os`'s `post_init_op.rs`.
- The two registrations serve different layers: `zk_evm` governs execution semantics (what runs during EVM interpretation), while `zksync-os` governs bootloader-level availability (whether the precompile exists in a given block context). Both must be consistent.
- If `zk_evm`'s dispatch table is a static array indexed by address, inserting `0x12` is straightforward. If it is a hash map, check for address collision handling.

#### Gas Metering
- Precompiles in `zk_evm` have associated gas cost functions. The FRI verifier precompile at `0x12` must have a gas cost defined. Options:
  - **Fixed cost**: simple but may be mispriced if proof sizes vary.
  - **Input-length-proportional cost**: more accurate but requires calldata length inspection.
  - **Free (zero gas)**: only acceptable if the precompile is callable exclusively by the bootloader itself; if L2 contracts can call it directly, zero gas creates a DoS vector.
- Recommendation: fixed high cost (e.g., `50_000` gas) for v1, revisable after benchmarking.

#### Memory Model for `proof_payload`
- The `proof_payload` bytes stored in `FriProofBlockContext` must be accessible to the precompile call at EVM execution time. `zk_evm`'s memory model allocates heap memory per-frame. The precompile's response (`bytes publicInputs`) must be written into the caller's return data buffer correctly.
- Review `precompile_abi.rs` (or equivalent) in `zk_evm` for the convention used by existing precompiles (e.g., `ecrecover` at `0x01`, `sha256` at `0x02`) to copy output into return data. The FRI precompile must follow the same convention.

#### Gateway-Only Availability at the EVM Layer
- If the precompile is registered in `zk_evm`'s dispatch table unconditionally but `zksync-os` only populates `FriProofBlockContext` on gateway nodes, then a call to `0x12` on a non-gateway node will dispatch to the precompile handler but find no proof data. The handler must return `(false, "")` gracefully rather than panic or revert with an unexpected error code.
- Alternatively, `zk_evm` could be taught to consult a runtime-provided availability flag before dispatching. This is more complex but cleaner.

#### Interaction with `STATICCALL`
- Precompiles are typically callable via both `CALL` and `STATICCALL`. The FRI verifier is read-only (it reads from `FriProofBlockContext` but does not write state), so `STATICCALL` should be supported. Verify that `zk_evm`'s precompile dispatch correctly handles `STATICCALL` for the new address.

#### Impact on Existing Precompile Tests
- `zk_evm` likely has a test suite that checks the behavior of all precompiles at addresses `0x01`–`0x0a` (standard EVM) and any ZKSync-specific additions. Adding `0x12` must not break existing address-range tests that assume a particular set of precompiles is exhaustive.
- A new test file `tests/fri_verifier_precompile.rs` should be added covering: successful lookup, missing proof index, non-gateway fallback behavior, and `STATICCALL` compatibility.

#### Unresolved Questions (zk_evm)

- **Gas cost model** — fixed vs. proportional; must be decided before audit.
- **Non-gateway fallback behavior** — graceful `(false, "")` vs. explicit revert; affects L2 contract error handling.
- **`STATICCALL` support** — confirm dispatch table handles read-only context correctly for `0x12`.
