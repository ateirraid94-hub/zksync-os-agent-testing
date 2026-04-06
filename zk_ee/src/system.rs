use crate::fri::VerifiedFriProof;
use std::collections::HashMap;

pub trait SystemInterface: Send + Sync + Clone + 'static {
    fn is_gateway_mode(&self) -> bool;
    fn get_verified_fri_proof(&self, proof_id: [u8; 32]) -> Option<VerifiedFriProof>;
    fn store_verified_fri_proof(&mut self, proof_id: [u8; 32], proof: VerifiedFriProof);
    fn set_tx_context(&mut self, context: TxContext);
}

#[derive(Debug, Default, Clone)]
pub struct TxContext {
    pub verified_fri_proofs: HashMap<[u8; 32], VerifiedFriProof>,
}
