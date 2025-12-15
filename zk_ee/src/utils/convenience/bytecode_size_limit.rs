pub fn derive_initcode_size_limit(max_code_size: u32) -> usize {
    (max_code_size * 2) as usize
}
