//! Extended pre-transaction loop for ZK header processing.
//!
//! FRI proof transactions (type `0x7c`) are consumed here, **before** the main
//! transaction loop, so that `proof_payload` data is available in
//! `fri_proof_context` storage for the precompile at `0x12` to reference.
//!
//! The ordering guarantee enforced here must match what `zksync-os-server`
//! places in the block: all `0x7c` txs precede all L2 txs.

use crate::bootloader::block_flow::BlockContext;
use crate::bootloader::errors::BootloaderError;
use zk_ee::system::metadata::ZKBasicBlockDataKeeper;

/// Maximum number of FRI proof transactions allowed per block.
/// Set conservatively until capacity profiling is complete.
pub const MAX_FRI_PROOF_TXS_PER_BLOCK: usize = 8;

/// Processes all `0x7c` FRI proof transactions that appear at the head of the
/// block transaction list before handing off to the main tx loop.
///
/// # Errors
/// Returns `BootloaderError::TooManyFriProofTxs` if more than
/// `MAX_FRI_PROOF_TXS_PER_BLOCK` are present.
pub fn process_fri_proof_pre_loop<Ctx: BlockContext>(
    ctx: &mut Ctx,
    block_keeper: &mut ZKBasicBlockDataKeeper,
) -> Result<usize, BootloaderError> {
    let mut count = 0usize;

    for tx in ctx.transactions_mut() {
        if tx.tx_type() != 0x7c {
            break;
        }
        if count >= MAX_FRI_PROOF_TXS_PER_BLOCK {
            return Err(BootloaderError::TooManyFriProofTxs);
        }

        let payload = tx
            .fri_proof_payload()
            .ok_or(BootloaderError::MalformedFriProofTx)?;

        block_keeper.add_fri_proof_entry(count, payload);
        count += 1;
    }

    Ok(count)
}
