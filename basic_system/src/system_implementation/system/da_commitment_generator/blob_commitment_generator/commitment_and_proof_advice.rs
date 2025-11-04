pub const BLOB_COMMITMENT_AND_PROOF_QUERY_ID: u32 =
    zk_ee::oracle::query_ids::ADVICE_SUBSPACE_MASK | 0x20;

#[cfg(target_arch = "riscv32")]
pub fn blob_commitment_and_proof_advice(
    data: &[u8],
    oracle: &mut impl zk_ee::oracle::IOOracle,
) -> [u8; 96] {
    let mut it = oracle
        .raw_query(
            BLOB_COMMITMENT_AND_PROOF_QUERY_ID,
            &(data.as_ptr() as usize as u32, data.len() as u32),
        )
        .unwrap();

    let mut buffer = [0u8; 96];
    unsafe {
        let buffer_u32_ptr: *mut u32 = buffer.as_mut_ptr().cast::<[u32; 24]>().cast();
        for i in 0..24 {
            buffer_u32_ptr.add(i).write(it.next().unwrap() as u32)
        }
    }
    assert!(it.next().is_none());
    buffer
}

#[cfg(not(target_arch = "riscv32"))]
pub fn blob_commitment_and_proof_advice(
    data: &[u8],
    _oracle: &mut impl zk_ee::oracle::IOOracle,
) -> [u8; 96] {
    use crypto::MiniDigest;

    let sidecar_builder: alloy_consensus::SidecarBuilder<alloy_consensus::SimpleCoder> =
        alloy_consensus::SidecarBuilder::from_slice(data);
    // TODO: at this step we compute also kzg proof, which is not needed in fact
    let sidecar = sidecar_builder.build().unwrap();
    assert_eq!(sidecar.blobs.len(), 1);

    let commitment = sidecar.commitments[0];
    let versioned_hash = sidecar.versioned_hashes().next().unwrap();

    let mut hasher = crypto::blake2s::Blake2s256::new();
    hasher.update(versioned_hash.as_slice());
    hasher.update(data);
    let mut challenge_point = hasher.finalize();
    // truncate hash to 128 bits
    // NOTE: it is safe to draw a random scalar at max 128 bits because of the schwartz zippel lemma
    for byte in challenge_point[0..16].iter_mut() {
        *byte = 0;
    }
    let blob =
        unsafe { core::mem::transmute::<&alloy_consensus::Blob, &c_kzg::Blob>(&sidecar.blobs[0]) };
    let p = c_kzg::ethereum_kzg_settings(8)
        .compute_kzg_proof(blob, &c_kzg::Bytes32::new(challenge_point))
        .unwrap();
    let proof = p.0;

    let mut result = [0u8; 96];
    result[..48].copy_from_slice(&commitment.0);
    result[48..].copy_from_slice(proof.as_slice());
    result
}
