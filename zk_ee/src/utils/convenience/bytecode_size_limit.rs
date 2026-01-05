/// As per EIP-3860, initcode is calculated via the following formula: MAX_INITCODE_SIZE = 2 * MAX_CODE_SIZE
pub fn derive_initcode_size_limit(max_code_size: u32) -> usize {
    (max_code_size * 2) as usize
}
