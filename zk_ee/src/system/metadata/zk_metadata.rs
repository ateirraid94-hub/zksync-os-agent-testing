//! Block metadata provided by the oracle to the ZK execution environment.
//!
//! `is_gateway` is a new field that gates gateway-only features such as the
//! FRI proof verifier precompile at `0x12`.
//!
//! The field is serialized as part of `BLOCK_METADATA_QUERY_ID` and committed
//! in the proof so that verifiers can enforce gateway-only invariants.

/// Metadata supplied by the oracle at the start of each block.
#[derive(Debug, Clone, Default)]
pub struct BlockMetadataFromOracle {
    /// Chain ID for the current execution environment.
    pub chain_id: u64,

    /// Protocol version active for this block.
    pub protocol_version: u32,

    /// If `true`, this node is operating in gateway mode and may include
    /// FRI proof transactions (`0x7c`) and expose the `0x12` precompile.
    ///
    /// Committed in the proof — must match the oracle's attestation.
    pub is_gateway: bool,
    // ... existing fields follow ...
}
