#[derive(Debug)]
pub enum ZkEeError {
    InvalidTransaction,
    ExecutionFailed,
    MemoryError,
    GasExhausted,
}
