# FRI Precompile Design for ZKsync Era

## Overview

This document outlines the design for implementing FRI (Fast Reed-Solomon Interactive Oracle Proofs) precompile functionality in ZKsync Era. The design enables on-chain verification of FRI proofs through a dedicated precompile while maintaining strict separation between gateway and L2 execution contexts.

## Architecture

### High-Level Design

The FRI precompile system consists of three main components:

1. **FRI Proof Transaction Type** - A new transaction variant that carries proof payloads
2. **Block-Level Proof Verification** - Verification occurs at block start before transaction execution
3. **FRI Verification Precompile** - EVM-accessible interface for querying proof verification results

### Execution Flow

```
Block Processing:
┌─────────────────────────────────────────────────────────────────┐
│ Phase 1: FRI Proof Verification (Block Start)                  │
│ ┌─────────────────┐    ┌──────────────────┐    ┌─────────────┐ │
│ │ FRI Proof Tx #1 │───▶│ Verify & Cache   │───▶│ TxContext   │ │
│ └─────────────────┘    │ Results          │    │ Storage     │ │
│ ┌─────────────────┐    │                  │    │             │ │
│ │ FRI Proof Tx #2 │───▶│                  │───▶│             │ │
│ └─────────────────┘    └──────────────────┘    └─────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│ Phase 2: Normal Transaction Execution                           │
│ ┌───────────────┐    ┌───────────────┐    ┌─────────────────┐   │
│ │ Regular Tx #1 │───▶│ EVM Execution │───▶│ Call FRI        │   │
│ └───────────────┘    └───────────────┘    │ Precompile      │   │
│ ┌───────────────┐                         │ (0x0101)        │   │
│ │ Regular Tx #2 │─────────────────────────▶│                 │   │
│ └───────────────┘                         └─────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## Transaction Type Design

### FRI Proof Transaction Structure

```
FriProofTransaction {
    chain_id: u64,
    nonce: u64, 
    gas_limit: u64,
    payload: FriProofPayload,
    signature: TransactionSignature
}

FriProofPayload {
    version: u8,                // Future compatibility
    proof_data: Vec<u8>,        // Serialized FRI proof
    public_inputs: Vec<u8>      // Public circuit inputs
}
```

### Transaction Processing Rules

- **Gateway Mode**: FRI proof transactions are accepted and processed
- **L2 Mode**: FRI proof transactions are rejected at bootloader level
- **Proof ID**: Generated via `keccak256(encoded_payload)` for unique identification
- **Gas Consumption**: FRI transactions consume gas during verification phase

## Precompile Interface

### Address and Function

- **Precompile Address**: `0x0101`
- **Function**: Query verification status of previously submitted FRI proofs
- **Gas Cost**: Base cost of 50,000 gas per call

### Input/Output Format

```
Input (32 bytes):
┌─────────────────────────────────────────┐
│ Proof ID (32 bytes - keccak256 hash)    │
└─────────────────────────────────────────┘

Output (33 bytes):
┌──────────────┬──────────────────────────────────────┐
│ Valid (1 bit)│ Public Inputs Hash (32 bytes)        │
└──────────────┴──────────────────────────────────────┘
```

### Error Conditions

- **NotSupported**: Called on L2 instance (non-gateway mode)
- **ProofNotFound**: Proof ID not found in current block context
- **InvalidInput**: Input length less than 32 bytes

## System Interface Extensions

### New System Interface Methods

```
trait SystemInterface {
    fn is_gateway_mode(&self) -> bool;
    fn get_verified_fri_proof(&self, proof_id: [u8; 32]) -> Option<VerifiedFriProof>;
    fn store_verified_fri_proof(&mut self, proof_id: [u8; 32], proof: VerifiedFriProof);
}
```

### TxContext Storage

```
TxContext {
    verified_fri_proofs: HashMap<[u8; 32], VerifiedFriProof>
}

VerifiedFriProof {
    proof_id: [u8; 32],
    is_valid: bool,
    public_inputs_hash: [u8; 32]
}
```

## Repository-Specific Changes

This feature spans five repositories. Below are the required changes in each.

### 1. zksync-os (Core System)

**`zk_ee/src/transaction.rs`**
- Add `FriProof(FriProofTransaction)` variant to the `Transaction` enum
- Add `FriProof = 0x72` to the `TxType` enum
- Implement `gas_limit()` and `tx_type()` dispatch for the new variant

**`zk_ee/src/fri.rs`** (new file)
- Define `FriProofPayload` with version byte, proof data, and public inputs
- Define `VerifiedFriProof` result struct
- Implement deterministic payload encoding for proof ID derivation

**`zk_ee/src/system.rs`**
- Extend `SystemInterface` trait with `is_gateway_mode()`, `get_verified_fri_proof()`, `store_verified_fri_proof()`
- Add `TxContext` struct with `verified_fri_proofs: HashMap<[u8;32], VerifiedFriProof>`

**`system_hooks/src/precompiles/fri_verifier.rs`** (new file)
- Implement FRI verification precompile at address `0x0101`
- Enforce gateway-mode guard; return `NotSupported` on L2 instances
- Read 32-byte proof ID from input; return 33-byte validity + hash response

**`basic_bootloader/src/bootloader.rs`**
- Add block-start verification loop: iterate FRI proof transactions, call verifier, populate `TxContext`
- Skip `Transaction::FriProof` variants during the normal execution phase
- Reject FRI proof transactions with `UnsupportedTransactionType` when not in gateway mode

**`basic_system/src/basic_system.rs`**
- Add `gateway_mode: bool` and `tx_context: TxContext` fields to `BasicSystem`
- Implement the three new `SystemInterface` methods

### 2. zksync-os-server (Sequencer)

**Transaction Pool**
- Add `FriProof` transaction variant to the mempool's accepted type set
- Gate acceptance behind a `gateway_mode` configuration flag; reject on standard L2 sequencer instances
- Enforce per-block and per-sender rate limits on FRI proof transactions to prevent mempool flooding

**Block Building**
- During block assembly, place all pending FRI proof transactions at the front of the transaction list so they are processed in Phase 1 before any user transactions
- Enforce a configurable `max_fri_proofs_per_block` cap to bound verification overhead per block
- Record proof transaction hashes in block metadata for tracing and debugging

**API Layer**
- Expose a dedicated `eth_sendFriProofTransaction` RPC endpoint (or extend `eth_sendRawTransaction` with type `0x72` support)
- Return structured errors distinguishing gateway-mode rejection from malformed payload

### 3. era-contracts (Gateway Interface)

**Gateway Contract**
- Add a `submitFriProof(bytes calldata payload)` function that constructs and submits a type-`0x72` transaction to the sequencer
- Validate the version byte and minimum payload length on-chain before forwarding
- Emit a `FriProofSubmitted(bytes32 indexed proofId, address indexed submitter)` event for indexers

**Validation Logic**
- Verify that the submitting address holds the required role (e.g., `PROOF_SUBMITTER_ROLE`) to prevent unauthorized proof injection
- Check that `proof_data` length is within configured bounds to block oversized payloads
- Enforce a cooldown period per submitter address to limit submission rate

**Interface ABI**
- Export `IFriProofSubmitter` interface and `FriProofPayload` ABI type for downstream tooling

### 4. zksync-os-revm (Consistency Checker)

**Transaction Replay**
- Add `FriProof` transaction deserialization to the REVM transaction decoder so consistency checks can replay blocks containing type-`0x72` transactions without erroring
- During replay, re-execute the block-start verification phase and compare cached results against the canonical `TxContext` stored in the block

**Context Synchronization**
- Extend the REVM block context structure to carry `verified_fri_proofs` populated during the verification replay phase
- Ensure the FRI precompile (`0x0101`) resolves correctly during REVM execution using the replayed context, producing identical outputs to the primary execution path

**Mode Detection**
- Pass `is_gateway: bool` into the REVM execution environment; the FRI precompile must behave identically to the native implementation (reject on L2 mode, resolve on gateway mode)

### 5. zksync-airbender (Verifier Source)

**Unified Verifier Interface**
- Export a stable `verify_fri_proof(proof_data: &[u8], public_inputs: &[u8]) -> bool` function from the unified verifier crate
- Provide a C-compatible FFI binding (`extern "C" fn zksync_verify_fri_proof`) for environments that cannot link Rust directly

**Integration Point**
- The `crypto/src/fri_verifier.rs` module in `zksync-os` calls this interface; replace the mock implementation once the airbender interface is stable
- Version the verifier interface with a semver-compatible crate feature flag so `zksync-os` can pin to a specific proof format version

## Security Considerations

### Cross-Mode Protection

1. **Gateway Isolation**: FRI functionality strictly limited to gateway instances
2. **L2 Rejection**: Explicit rejection of FRI transactions on L2 prevents replay attacks
3. **Context Isolation**: Proof verification results are block-scoped and cannot persist

### Verification Integrity

1. **Deterministic Proof IDs**: Generated via cryptographic hash prevents collisions
2. **Immutable Results**: Once verified, proof results cannot be modified within block
3. **Version Compatibility**: Version byte enables backward compatibility for proof formats

### Gas Considerations

1. **Bounded Verification Cost**: Fixed gas cost model prevents DOS attacks
2. **Block-Level Batching**: Verification occurs once per proof, not per precompile call
3. **Failed Verification Handling**: Invalid proofs still consume verification gas

### Spam Protection

1. **Per-Block Cap**: `max_fri_proofs_per_block` limits total verification work per block, bounding worst-case block processing latency
2. **Submission Rate Limiting**: Per-sender cooldown enforced in the gateway contract prevents a single actor from flooding the proof queue
3. **Role-Based Access Control**: Only addresses holding `PROOF_SUBMITTER_ROLE` may submit FRI proof transactions via the gateway contract
4. **Payload Size Bounds**: Maximum `proof_data` and `public_inputs` lengths enforced both in the gateway contract and in the sequencer mempool to prevent oversized transaction payloads
5. **Mempool Segregation**: FRI proof transactions are held in a separate mempool pool with its own capacity limit, preventing them from crowding out regular user transactions

## Implementation Phases

### Phase 1: Core Infrastructure

- Transaction type definitions and encoding in `zk_ee`
- `FriProofPayload` and `VerifiedFriProof` data structures
- System interface extensions (`is_gateway_mode`, `get_verified_fri_proof`, `store_verified_fri_proof`)
- Basic precompile scaffold at `0x0101` with mock verifier

### Phase 2: Bootloader & System Integration

- Block-start FRI verification loop in the bootloader
- `TxContext` population and storage in `BasicSystem`
- Gateway-mode detection and per-mode routing
- Unit and integration tests for gateway vs. L2 behavior

### Phase 3: Verifier Integration

- Integration with `zksync-airbender` unified verifier (replace mock)
- Production-ready proof verification logic
- Gas cost calibration based on real verification benchmarks
- Proof ID collision analysis and stress tests

### Phase 4: Sequencer Support

- Type-`0x72` transaction handling in `zksync-os-server` mempool
- Block builder ordering logic (FRI proofs first)
- `max_fri_proofs_per_block` configuration and enforcement
- RPC endpoint for FRI proof submission

### Phase 5: Era Contracts & Gateway Interface

- `submitFriProof` function and `FriProofSubmitted` event in gateway contract
- Role-based access control and payload validation
- `IFriProofSubmitter` interface ABI export
- End-to-end test from contract submission through on-chain verification query

### Phase 6: REVM Compatibility & Ecosystem Hardening

- FRI transaction replay support in `zksync-os-revm` consistency checker
- Context synchronization between primary execution and REVM replay
- Developer documentation and SDK examples
- Audit preparation: security review of cross-mode boundaries and spam-protection mechanisms

## Future Extensibility

### Proof Format Evolution

- **Version Field**: Enables support for multiple proof formats
- **Payload Structure**: Flexible encoding supports additional metadata
- **Backward Compatibility**: Older versions remain supported

### Additional Verification Systems

- **Modular Design**: Framework can support other proof systems
- **Address Space**: Reserved precompile addresses for future systems
- **Interface Standardization**: Common patterns for proof verification precompiles

## Testing Strategy

### Unit Tests

- Transaction encoding/decoding correctness
- Precompile input/output validation
- Error condition handling

### Integration Tests

- End-to-end proof verification workflow
- Gateway vs L2 mode behavior differences
- Cross-transaction proof access patterns

### Performance Tests

- Gas consumption analysis
- Block processing latency impact
- Memory usage optimization

## Conclusion

The FRI precompile design provides a secure, efficient mechanism for on-chain proof verification while maintaining strict separation between gateway and L2 execution contexts. The block-level verification approach ensures deterministic execution across both forward and proving modes, while the precompile interface enables flexible access patterns for smart contracts requiring proof verification capabilities. The phased rollout across all five affected repositories ensures each system can be validated independently before full ecosystem integration.
