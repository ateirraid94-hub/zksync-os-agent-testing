use alloy_consensus::{SidecarBuilder, SimpleCoder};
use risc_v_simulator::abstractions::memory::MemorySource;
use basic_system::system_implementation::system::pubdata_destination::blob_commitment_generator::VERSIONED_HASH_ADVICE_QUERY_ID;
use crypto::MiniDigest;
use oracle_provider::OracleQueryProcessor;
use crate::arithmetic::ArithmeticQuery;
use crate::utils::evaluate::{read_memory_as_u64, read_memory_as_u8, read_struct};
use crate::utils::usize_slice_iterator::UsizeSliceIteratorOwned;

pub struct VersionedHashAndProofQuery<M: MemorySource> {
    _marker: std::marker::PhantomData<M>,
}

impl<M: MemorySource> Default for VersionedHashAndProofQuery<M> {
    fn default() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<M: MemorySource> OracleQueryProcessor<M> for VersionedHashAndProofQuery<M> {
    fn supported_query_ids(&self) -> Vec<u32> {
        vec![VERSIONED_HASH_ADVICE_QUERY_ID]
    }

    fn process_buffered_query(
        &mut self,
        query_id: u32,
        query: Vec<usize>,
        memory: &M,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static> {
        debug_assert!(self.supports_query_id(query_id));

        let mut it = query.into_iter();

        let data_ptr = it.next().unwrap() as u32;
        let data_len = it.next().unwrap() as u32;

        assert!(
            it.next().is_none(),
            "A single RISC-V ptr should've been passed."
        );

        assert!(data_ptr.is_multiple_of(4));

        let data = read_memory_as_u8(memory, data_ptr, data_len).unwrap();
        let result = proof_and_commitment_alloy(&data);

        let r = result
            .into_iter()
            .array_chunks::<8>()
            .map(|x| u64::from_le_bytes(x) as usize)
            .collect::<Vec<_>>();
        // println!("=========!!!!!!!!! {:?}", r);
        let r = Vec::into_boxed_slice(r);

        let n = UsizeSliceIteratorOwned::new(r);

        Box::new(n)
    }
}

fn versioned_hash_for_kzg(data: &[u8]) -> [u8; 32] {
    use crypto::sha256::Digest;
    let mut hash: [u8; 32] = crypto::sha256::Sha256::digest(data).into();
    hash[0] = 1; // KZG_VERSIONED_HASH_VERSION_BYTE

    hash
}

fn proof_and_commitment_alloy(data: &[u8]) -> [u8; 96] {
    // TODO: this also creates ethereum kzg commitment proof(kzg proof for Fiat-Shamir challenge) which we don't need
    let sidecar_builder: SidecarBuilder<SimpleCoder> = alloy_consensus::SidecarBuilder::from_slice(data);
    let sidecar = sidecar_builder.build().unwrap();
    assert_eq!(sidecar.blobs.len(), 1);

    let commitment = sidecar.commitments[0];

    let mut hasher = crypto::blake2s::Blake2s256::new();
    hasher.update(versioned_hash_for_kzg(sidecar.commitments[0].as_slice()));
    hasher.update(data);
    let mut challenge_point = hasher.finalize();
    for byte in challenge_point[0..16].iter_mut() {
        *byte = 0;
    }
    let blob = unsafe { core::mem::transmute::<&alloy_consensus::Blob, &c_kzg::Blob>(&sidecar.blobs[0]) };
    let p = c_kzg::ethereum_kzg_settings(8).compute_kzg_proof(
        blob,
        &c_kzg::Bytes32::new(challenge_point)
    ).unwrap();
    let proof = p.0;

    let mut result = [0u8; 96];
    result[..48].copy_from_slice(&commitment.0);
    result[48..].copy_from_slice(proof.as_slice());
    result
}
