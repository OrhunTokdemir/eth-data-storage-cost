use super::{manifest, register, verify, Ctx, StorageStrategy, StoreReceipt};
use crate::{
    chunk::{chunks, CALLDATA_CHUNK},
    contracts::CalldataStore,
};
use alloy::{
    consensus::Transaction as _,
    primitives::{keccak256, Bytes, B256},
    providers::Provider,
    rpc::types::Filter,
    sol_types::{SolCall, SolEvent},
};

/// Proof lives only in tx calldata; a tiny event indexes each chunk tx.
pub struct CalldataStrategy;

impl StorageStrategy for CalldataStrategy {
    fn kind(&self) -> u8 {
        crate::contracts::kind::CALLDATA
    }

    async fn store(&self, ctx: &Ctx, id: B256, data: &[u8], _uri: Option<&str>) -> eyre::Result<StoreReceipt> {
        let store = CalldataStore::new(ctx.deployment.calldata, ctx.provider.clone());
        let parts = chunks(data, CALLDATA_CHUNK);
        let mut rcpt = StoreReceipt { chunks: parts.len(), ..Default::default() };
        for (i, c) in parts.iter().enumerate() {
            let r = store.post(id, i as u32, Bytes::copy_from_slice(c)).send().await?.get_receipt().await?;
            rcpt.push(&r)?;
        }
        register(ctx, &mut rcpt, id, self.kind(), data, ctx.deployment.calldata, vec![], String::new()).await?;
        Ok(rcpt)
    }

    async fn retrieve(&self, ctx: &Ctx, id: B256) -> eyre::Result<Vec<u8>> {
        let m = manifest(ctx, id, self.kind()).await?;
        let filter = Filter::new()
            .address(ctx.deployment.calldata)
            .event_signature(CalldataStore::ChunkPosted::SIGNATURE_HASH)
            .topic1(id)
            .from_block(0);
        let logs = ctx.provider.get_logs(&filter).await?;
        let mut parts: Vec<(u32, Vec<u8>)> = Vec::new();
        for log in logs {
            let ev = log.log_decode::<CalldataStore::ChunkPosted>()?.inner.data;
            let h = log.transaction_hash.ok_or_else(|| eyre::eyre!("log without tx hash"))?;
            let tx = ctx.provider.get_transaction_by_hash(h).await?.ok_or_else(|| eyre::eyre!("tx {h} not found (history expired?)"))?;
            let call = CalldataStore::postCall::abi_decode(tx.input())?;
            eyre::ensure!(keccak256(&call.chunk) == ev.chunkHash, "chunk {} hash mismatch", ev.index);
            parts.push((ev.index, call.chunk.to_vec()));
        }
        parts.sort_by_key(|(i, _)| *i);
        parts.dedup_by_key(|(i, _)| *i);
        eyre::ensure!(parts.len() == m.chunkCount as usize, "found {} of {} chunks", parts.len(), m.chunkCount);
        verify(&m, parts.into_iter().flat_map(|(_, c)| c).collect())
    }
}
