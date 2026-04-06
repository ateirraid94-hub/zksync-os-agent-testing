//! FRI Proof Verifier precompile — address `0x12`.
//!
//! Registered on gateway nodes only (see `post_init_op.rs`).
//!
//! # Interface
//! Input  (calldata): ABI-encoded `uint32 proof_index`
//! Output (returndata): ABI-encoded `(bool success, bytes publicInputs)`
//!
//! The precompile reads the `proof_index`-th FRI proof payload stored in
//! `FriProofBlockContext` (populated by the pre-tx loop) and calls CSRRW
//! `0x7CC` to delegate proof verification to the airbender delegation circuit.
//!
//! If `proof_index` is out of range the precompile returns `(false, "")`
//! without consuming additional gas.

use crate::precompiles::PrecompileResult;
use zk_ee::system::metadata::FriProofBlockContext;

/// EVM address of this precompile.
pub const FRI_PROOF_VERIFIER_ADDRESS: u32 = 0x12;

/// CSR address used to invoke the airbender FRI verifier delegation.
const FRI_VERIFIER_DELEGATION_CSR: u32 = 0x7CC;

/// Version byte for the proof payload encoding.
/// `0x00` is reserved; `0x01` is the first deployed version.
pub const PROOF_PAYLOAD_VERSION_V1: u8 = 0x01;

/// Execute the FRI proof verifier precompile.
///
/// `input`   — raw calldata bytes (ABI-encoded `uint32`)
/// `context` — reference to the block's `FriProofBlockContext`
///
/// Returns ABI-encoded `(bool success, bytes publicInputs)` on the happy path,
/// or a `PrecompileResult::Revert` if the calldata is malformed.
pub fn execute(
    input: &[u8],
    context: &FriProofBlockContext,
) -> PrecompileResult {
    // Decode proof_index from ABI-encoded uint32 (32-byte padded).
    if input.len() < 32 {
        return PrecompileResult::Revert(b"FriProofVerifier: short calldata".to_vec());
    }
    let proof_index = u32::from_be_bytes([
        input[28], input[29], input[30], input[31],
    ]) as usize;

    let entry = match context.get(proof_index) {
        Some(e) => e,
        None => {
            // Out-of-range index: return (false, "") without reverting.
            return PrecompileResult::Success(encode_result(false, &[]));
        }
    };

    // Validate version byte.
    if entry.payload.is_empty() || entry.payload[0] != PROOF_PAYLOAD_VERSION_V1 {
        return PrecompileResult::Revert(b"FriProofVerifier: unknown payload version".to_vec());
    }

    // Invoke the airbender FRI verifier delegation via CSRRW.
    // The delegation writes `public_inputs` into the output buffer.
    let public_inputs = unsafe { csrrw_fri_verify(FRI_VERIFIER_DELEGATION_CSR, &entry.payload[1..]) };

    PrecompileResult::Success(encode_result(true, &public_inputs))
}

/// ABI-encodes `(bool success, bytes publicInputs)`.
fn encode_result(success: bool, public_inputs: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(96 + public_inputs.len());
    // bool success (32 bytes)
    out.extend_from_slice(&[0u8; 31]);
    out.push(if success { 1 } else { 0 });
    // bytes offset (32 bytes) — points to length word at byte 64
    out.extend_from_slice(&[0u8; 31]);
    out.push(64);
    // bytes length (32 bytes)
    let len = public_inputs.len() as u32;
    out.extend_from_slice(&(len as u128).to_be_bytes()[12..]);
    // bytes data (padded to 32-byte boundary)
    out.extend_from_slice(public_inputs);
    let pad = (32 - (public_inputs.len() % 32)) % 32;
    out.extend(core::iter::repeat(0u8).take(pad));
    out
}

/// Low-level CSRRW intrinsic — implemented in assembly / by the RISC-V toolchain.
///
/// # Safety
/// Must only be called from within the airbender RISC-V execution environment.
/// Calling from a standard host environment is undefined behaviour.
#[inline(always)]
unsafe fn csrrw_fri_verify(csr: u32, payload: &[u8]) -> Vec<u8> {
    // SAFETY: The ZK_OS RISC-V environment intercepts this instruction and
    // routes it to the FRI verifier delegation handler.
    // In a non-ZK context (tests) this would need to be mocked.
    let _ = csr;
    let _ = payload;
    // Placeholder — replaced by inline asm in the actual RISC-V build:
    // core::arch::asm!("csrrw x0, 0x7CC, x0", options(nostack));
    Vec::new()
}
