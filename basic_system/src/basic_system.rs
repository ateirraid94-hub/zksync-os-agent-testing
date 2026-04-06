use zk_ee::{
    fri::VerifiedFriProof,
    system::{SystemInterface, TxContext},
};
use std::collections::HashMap;

pub struct BasicSystem<O> {
    pub oracle: O,
    pub gateway_mode: bool,
    pub tx_context: TxContext,
}

pub trait Oracle: Send + Sync + Clone + 'static {}

impl<O: Oracle> Clone for BasicSystem<O> {
    fn clone(&self) -> Self {
        Self {
            oracle: self.oracle.clone(),
            gateway_mode: self.gateway_mode,
            tx_context: self.tx_context.clone(),
        }
    }
}

impl<O: Oracle> SystemInterface for BasicSystem<O> {
    fn is_gateway_mode(&self) -> bool {
        self.gateway_mode
    }
    
    fn get_verified_fri_proof(&self, proof_id: [u8; 32]) -> Option<VerifiedFriProof> {
        self.tx_context.verified_fri_proofs.get(&proof_id).cloned()
    }
    
    fn store_verified_fri_proof(&mut self, proof_id: [u8; 32], proof: VerifiedFriProof) {
        self.tx_context.verified_fri_proofs.insert(proof_id, proof);
    }
    
    fn set_tx_context(&mut self, context: TxContext) {
        self.tx_context = context;
    }
}

impl<O: Oracle> Default for BasicSystem<O> 
where 
    O: Default,
{
    fn default() -> Self {
        Self {
            oracle: O::default(),
            gateway_mode: false,
            tx_context: TxContext::default(),
        }
    }
}
