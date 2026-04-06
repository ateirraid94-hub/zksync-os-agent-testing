use crypto::fri_verifier::verify_fri_proof;
use zk_ee::{
    transaction::{Transaction, TxType},
    fri::VerifiedFriProof,
    system::{SystemInterface, TxContext},
};

pub struct Bootloader<S> {
    system: S,
}

#[derive(Debug)]
pub struct Block {
    pub transactions: Vec<Transaction>,
}

#[derive(Debug)]
pub struct BlockOutput {
    pub transaction_outputs: Vec<TransactionOutput>,
}

#[derive(Debug)]
pub struct TransactionOutput {
    pub success: bool,
    pub gas_used: u64,
}

#[derive(Debug)]
pub enum BootloaderError {
    UnsupportedTransactionType,
    TransactionFailed,
    ProofVerificationFailed,
}

impl<S: SystemInterface> Bootloader<S> {
    pub fn new(system: S) -> Self {
        Self { system }
    }
    
    pub fn execute_block(&mut self, block: Block) -> Result<BlockOutput, BootloaderError> {
        // Phase 1: Verify all FRI proofs at block start
        let mut tx_context = TxContext::default();
        self.verify_fri_proofs(&block.transactions, &mut tx_context)?;
        
        // Store tx_context in system
        self.system.set_tx_context(tx_context);
        
        // Phase 2: Execute transactions normally
        let mut outputs = Vec::new();
        
        for tx in &block.transactions {
            // Skip FRI proof transactions during execution - they were already processed
            if matches!(tx, Transaction::FriProof(_)) {
                continue;
            }
            
            let output = self.execute_transaction(tx)?;
            outputs.push(output);
        }
        
        Ok(BlockOutput { transaction_outputs: outputs })
    }
    
    fn verify_fri_proofs(
        &mut self,
        transactions: &[Transaction],
        tx_context: &mut TxContext,
    ) -> Result<(), BootloaderError> {
        for tx in transactions {
            if let Transaction::FriProof(fri_tx) = tx {
                // Reject FRI proof transactions on L2 instances
                if !self.system.is_gateway_mode() {
                    return Err(BootloaderError::UnsupportedTransactionType);
                }
                
                let verified_proof = verify_fri_proof(&fri_tx.payload);
                tx_context.verified_fri_proofs.insert(verified_proof.proof_id, verified_proof);
            }
        }
        Ok(())
    }
    
    fn execute_transaction(&mut self, tx: &Transaction) -> Result<TransactionOutput, BootloaderError> {
        // Mock transaction execution
        Ok(TransactionOutput {
            success: true,
            gas_used: tx.gas_limit(),
        })
    }
}
