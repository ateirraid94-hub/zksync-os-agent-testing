use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriProofPayload {
    pub version: u8,
    pub proof_data: Vec<u8>,
    pub public_inputs: Vec<u8>,
}

impl FriProofPayload {
    pub fn new(version: u8, proof_data: Vec<u8>, public_inputs: Vec<u8>) -> Self {
        Self {
            version,
            proof_data,
            public_inputs,
        }
    }
    
    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.push(self.version);
        encoded.extend_from_slice(&(self.proof_data.len() as u32).to_be_bytes());
        encoded.extend_from_slice(&self.proof_data);
        encoded.extend_from_slice(&(self.public_inputs.len() as u32).to_be_bytes());
        encoded.extend_from_slice(&self.public_inputs);
        encoded
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedFriProof {
    pub proof_id: [u8; 32],
    pub is_valid: bool,
    pub public_inputs_hash: [u8; 32],
}
