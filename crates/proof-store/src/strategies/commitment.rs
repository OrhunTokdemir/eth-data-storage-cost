use super::{manifest, register, verify, Ctx, StorageStrategy, StoreReceipt};
use alloy::primitives::{Address, B256};

/// Only keccak256 + length + an off-chain URI go on-chain (~100k gas total).
/// Permanence depends on the off-chain store (Arweave = pay-once permanent; IPFS needs pinning).
pub struct CommitmentStrategy;

impl StorageStrategy for CommitmentStrategy {
    fn kind(&self) -> u8 {
        crate::contracts::kind::COMMITMENT
    }

    async fn store(&self, ctx: &Ctx, id: B256, data: &[u8], uri: Option<&str>) -> eyre::Result<StoreReceipt> {
        let uri = uri.ok_or_else(|| eyre::eyre!("commitment strategy needs --uri (ar://, ipfs://, https://, file://)"))?;
        let mut rcpt = StoreReceipt { chunks: 1, ..Default::default() };
        register(ctx, &mut rcpt, id, self.kind(), data, Address::ZERO, vec![], uri.to_string()).await?;
        Ok(rcpt)
    }

    async fn retrieve(&self, ctx: &Ctx, id: B256) -> eyre::Result<Vec<u8>> {
        let m = manifest(ctx, id, self.kind()).await?;
        verify(&m, fetch_uri(&m.uri).await?)
    }
}

/// Resolve `file://`, `ar://`, `ipfs://` and `http(s)://` URIs.
pub async fn fetch_uri(uri: &str) -> eyre::Result<Vec<u8>> {
    if let Some(p) = uri.strip_prefix("file://") {
        return Ok(std::fs::read(p)?);
    }
    let url = if let Some(id) = uri.strip_prefix("ar://") {
        format!("https://arweave.net/{id}")
    } else if let Some(cid) = uri.strip_prefix("ipfs://") {
        format!("https://ipfs.io/ipfs/{cid}")
    } else {
        uri.to_string()
    };
    let resp = reqwest::get(&url).await?.error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}
