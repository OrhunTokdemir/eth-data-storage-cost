use super::{manifest, register, verify, Ctx, StorageStrategy, StoreReceipt};
use crate::{
    chunk::MAX_BLOBS_PER_TX,
    contracts::BlobAnchor,
};
use alloy::{
    consensus::{SidecarBuilder, SidecarCoder, SimpleCoder},
    eips::eip4844::FIELD_ELEMENTS_PER_BLOB,
    network::{TransactionBuilder, TransactionBuilder4844},
    primitives::{keccak256, Bytes, B256},
    providers::Provider,
    rpc::types::TransactionRequest,
    sol_types::SolCall,
};

/// Usable bytes per blob with `SimpleCoder` (31 bytes per field element, 1 FE length header).
const BYTES_PER_BLOB: usize = (FIELD_ELEMENTS_PER_BLOB as usize - 1) * 31;
/// Max proof bytes carried by one type-3 tx.
const PART_BYTES: usize = BYTES_PER_BLOB * MAX_BLOBS_PER_TX;

/// Proof carried as EIP-4844 blobs; `BlobAnchor` records the versioned hashes (KZG commitments)
/// in the same tx. Blob data is pruned after ~18 days — the on-chain commitment is permanent.
pub struct BlobStrategy;

fn blobs_for(len: usize) -> usize {
    let c = SimpleCoder::default();
    c.required_fe(&vec![0u8; len]).div_ceil(FIELD_ELEMENTS_PER_BLOB as usize)
}

/// Anchor key for part `i` of a proof: keccak256(electionId ‖ i).
fn part_key(id: B256, i: usize) -> B256 {
    let mut b = id.to_vec();
    b.extend_from_slice(&(i as u32).to_be_bytes());
    keccak256(b)
}

impl StorageStrategy for BlobStrategy {
    fn kind(&self) -> u8 {
        crate::contracts::kind::BLOB
    }

    async fn store(&self, ctx: &Ctx, id: B256, data: &[u8], _uri: Option<&str>) -> eyre::Result<StoreReceipt> {
        let parts: Vec<&[u8]> = data.chunks(PART_BYTES).collect();
        let mut rcpt = StoreReceipt { chunks: parts.len(), ..Default::default() };
        let mut refs = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            let sidecar = SidecarBuilder::<SimpleCoder>::from_slice(part).build_4844()?;
            eyre::ensure!(sidecar.blobs.len() == blobs_for(part.len()), "unexpected blob count");
            refs.extend(sidecar.versioned_hashes());
            let input = BlobAnchor::anchorCall { electionId: part_key(id, i), dataHash: keccak256(part) }.abi_encode();
            let tx = TransactionRequest::default()
                .with_to(ctx.deployment.blob_anchor)
                .with_input(input)
                .with_blob_sidecar_4844(sidecar);
            let r = ctx.provider.send_transaction(tx).await?.get_receipt().await?;
            rcpt.push(&r)?;
        }
        register(ctx, &mut rcpt, id, self.kind(), data, ctx.deployment.blob_anchor, refs, String::new()).await?;
        Ok(rcpt)
    }

    /// Blob data is NOT available via the execution JSON-RPC. This uses Anvil's
    /// `anvil_getBlobByHash`; on a real network fetch sidecars from a beacon node
    /// (`/eth/v1/beacon/blob_sidecars/{block}`) or a blob archive within ~18 days.
    async fn retrieve(&self, ctx: &Ctx, id: B256) -> eyre::Result<Vec<u8>> {
        let m = manifest(ctx, id, self.kind()).await?;
        let total = m.totalLength as usize;
        let mut out = Vec::with_capacity(total);
        let mut refs = m.refs.iter();
        let mut off = 0;
        let mut part_no = 0;
        while off < total {
            let len = PART_BYTES.min(total - off);
            // Blobs are 128 KiB each: keep them on the heap (`Bytes`), not as `Blob` arrays in the future.
            let mut blobs: Vec<u8> = Vec::new();
            for _ in 0..blobs_for(len) {
                let h = refs.next().ok_or_else(|| eyre::eyre!("manifest has too few versioned hashes"))?;
                let blob: Option<Bytes> = ctx
                    .provider
                    .raw_request("anvil_getBlobByHash".into(), (h,))
                    .await
                    .map_err(|e| eyre::eyre!("blob {h} unavailable (pruned, or node lacks anvil_getBlobByHash): {e}"))?;
                blobs.extend_from_slice(&blob.ok_or_else(|| eyre::eyre!("blob {h} not found (pruned?)"))?);
            }
            let decoded = simple_decode(&blobs)?;
            // the anchor recorded keccak(part) permanently in the same tx
            let anchor = BlobAnchor::new(ctx.deployment.blob_anchor, ctx.provider.clone());
            let a = anchor.get(part_key(id, part_no)).call().await?;
            eyre::ensure!(keccak256(&decoded) == a.dataHash, "part {part_no} keccak mismatch with BlobAnchor");
            out.extend_from_slice(&decoded);
            off += len;
            part_no += 1;
        }
        verify(&m, out)
    }
}

/// Decode one `SimpleCoder` item from concatenated blob bytes: each 32-byte field element
/// carries 31 data bytes (byte 0 is zero); the first FE holds the big-endian u64 length.
fn simple_decode(blobs: &[u8]) -> eyre::Result<Vec<u8>> {
    let mut fes = blobs.as_chunks::<32>().0.iter();
    let head = fes.next().ok_or_else(|| eyre::eyre!("empty blob data"))?;
    let mut remaining = u64::from_be_bytes(head[1..9].try_into()?) as usize;
    let mut out = Vec::with_capacity(remaining);
    while remaining > 0 {
        let fe = fes.next().ok_or_else(|| eyre::eyre!("blob data truncated"))?;
        let n = remaining.min(31);
        out.extend_from_slice(&fe[1..1 + n]);
        remaining -= n;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_decode_matches_builder() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i * 7 + 3) as u8).collect();
        let sc = SidecarBuilder::<SimpleCoder>::from_slice(&data).build_4844().unwrap();
        let flat: Vec<u8> = sc.blobs.iter().flat_map(|b| b.iter().copied()).collect();
        assert_eq!(simple_decode(&flat).unwrap(), data);
    }

    #[test]
    fn blob_count_for_330kb() {
        assert_eq!(blobs_for(330_000), 3);
        const { assert!(PART_BYTES > 700_000) };
    }
}
