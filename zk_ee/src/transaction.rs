use crate::fri::FriProofPayload;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Transaction {
    EIP712(Eip712Transaction),
    EIP1559(Eip1559Transaction),
    EIP2930(Eip2930Transaction),
    Legacy(LegacyTransaction),
    FriProof(FriProofTransaction),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FriProofTransaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_limit: u64,
    pub payload: FriProofPayload,
    pub signature: TransactionSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Eip712Transaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_limit: u64,
    pub signature: TransactionSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Eip1559Transaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_limit: u64,
    pub signature: TransactionSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Eip2930Transaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_limit: u64,
    pub signature: TransactionSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LegacyTransaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_limit: u64,
    pub signature: TransactionSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct TransactionSignature {
    pub v: u64,
    pub r: [u8; 32],
    pub s: [u8; 32],
}

impl Transaction {
    pub fn tx_type(&self) -> TxType {
        match self {
            Transaction::EIP712(_) => TxType::EIP712,
            Transaction::EIP1559(_) => TxType::EIP1559,
            Transaction::EIP2930(_) => TxType::EIP2930,
            Transaction::Legacy(_) => TxType::Legacy,
            Transaction::FriProof(_) => TxType::FriProof,
        }
    }
    
    pub fn gas_limit(&self) -> u64 {
        match self {
            Transaction::EIP712(tx) => tx.gas_limit,
            Transaction::EIP1559(tx) => tx.gas_limit,
            Transaction::EIP2930(tx) => tx.gas_limit,
            Transaction::Legacy(tx) => tx.gas_limit,
            Transaction::FriProof(tx) => tx.gas_limit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TxType {
    EIP712 = 0x71,
    EIP1559 = 0x02,
    EIP2930 = 0x01,
    Legacy = 0x00,
    FriProof = 0x72,
}
