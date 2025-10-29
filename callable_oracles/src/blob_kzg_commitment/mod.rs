use alloy_consensus::{SidecarBuilder, SimpleCoder};
use risc_v_simulator::abstractions::memory::MemorySource;
use basic_system::system_implementation::system::da_commitment_generator::BLOB_COMMITMENT_AND_PROOF_QUERY_ID;
use crypto::MiniDigest;
use oracle_provider::OracleQueryProcessor;
use crate::utils::evaluate::read_memory_as_u8;
use crate::utils::usize_slice_iterator::UsizeSliceIteratorOwned;

///
/// Query processor, which returns blob kzg commitment and proof for a given data.
///
/// Proof is basically kzg proof in a point derived from data and kzg commitment,
/// so it allows to verify kzg commitment correctness by validating this proof and value of the polynomial in this point.
///
pub struct BlobCommitmentAndProofQuery<M: MemorySource> {
    _marker: std::marker::PhantomData<M>,
}

impl<M: MemorySource> Default for BlobCommitmentAndProofQuery<M> {
    fn default() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<M: MemorySource> OracleQueryProcessor<M> for BlobCommitmentAndProofQuery<M> {
    fn supported_query_ids(&self) -> Vec<u32> {
        vec![BLOB_COMMITMENT_AND_PROOF_QUERY_ID]
    }

    fn process_buffered_query(
        &mut self,
        query_id: u32,
        query: Vec<usize>,
        memory: &M,
    ) -> Box<dyn ExactSizeIterator<Item = usize> + 'static> {
        debug_assert!(self.supports_query_id(query_id));

        // this query processor supposed to work only on "host" architecture, which is always 64 bit
        const { assert!(8 == core::mem::size_of::<usize>()) };
        let mut it = query.into_iter();

        // Even though on riscv32 pointer and length are 32 bits, they are encoded as u64 and take a whole 64-bit word here
        let data_ptr = it.next().unwrap() as u32;
        let data_len = it.next().unwrap() as u32;
        assert!(
            it.next().is_none(),
            "A single RISC-V ptr should've been passed."
        );

        assert!(data_ptr.is_multiple_of(4));

        let data = read_memory_as_u8(memory, data_ptr, data_len).unwrap();
        let result = blob_kzg_commitment_and_proof(&data);

        let r = result
            .into_iter()
            .array_chunks::<8>()
            .map(|x| u64::from_le_bytes(x) as usize)
            .collect::<Vec<_>>();
        let r = Vec::into_boxed_slice(r);
        let n = UsizeSliceIteratorOwned::new(r);
        Box::new(n)
    }
}

///
/// Calculate kzg commitment and proof at the point `blake2s(versioned_hash & data)` for blob created from passed data.
/// We encode data into the blob following alloy default(`SimpleCoder`) approach.
///
fn blob_kzg_commitment_and_proof(data: &[u8]) -> [u8; 96] {
    let sidecar_builder: SidecarBuilder<SimpleCoder> = SidecarBuilder::from_slice(data);
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

