use super::{manifest, register, verify, Ctx, StorageStrategy, StoreReceipt};
use crate::{
    chunk::{chunks, CALLDATA_CHUNK},
    contracts::EventLogStore,
};
use alloy::{
    primitives::{Bytes, B256},
    providers::Provider,
    rpc::types::Filter,
    sol_types::SolEvent,
};

/// Proof emitted in event logs; read back with eth_getLogs.
pub struct EventLogStrategy;

impl StorageStrategy for EventLogStrategy {
    fn kind(&self) -> u8 {
        crate::contracts::kind::EVENTS
    }

    async fn store(&self, ctx: &Ctx, id: B256, data: &[u8], _uri: Option<&str>) -> eyre::Result<StoreReceipt> {
        let store = EventLogStore::new(ctx.deployment.events, ctx.provider.clone());
        let parts = chunks(data, CALLDATA_CHUNK);
        let mut rcpt = StoreReceipt { chunks: parts.len(), ..Default::default() };
        for (i, c) in parts.iter().enumerate() {
            let r = store.emitChunk(id, i as u32, Bytes::copy_from_slice(c)).send().await?.get_receipt().await?;
            rcpt.push(&r)?;
        }
        register(ctx, &mut rcpt, id, self.kind(), data, ctx.deployment.events, vec![], String::new()).await?;
        Ok(rcpt)
    }

    async fn retrieve(&self, ctx: &Ctx, id: B256) -> eyre::Result<Vec<u8>> {
        let m = manifest(ctx, id, self.kind()).await?;
        let filter = Filter::new()
            .address(ctx.deployment.events)
            .event_signature(EventLogStore::Chunk::SIGNATURE_HASH)
            .topic1(id)
            .from_block(0);
        let mut parts: Vec<(u32, Vec<u8>)> = Vec::new();
        for log in ctx.provider.get_logs(&filter).await? {
            let ev = log.log_decode::<EventLogStore::Chunk>()?.inner.data;
            parts.push((ev.index, ev.data.to_vec()));
        }
        parts.sort_by_key(|(i, _)| *i);
        parts.dedup_by_key(|(i, _)| *i);
        eyre::ensure!(parts.len() == m.chunkCount as usize, "found {} of {} chunks", parts.len(), m.chunkCount);
        verify(&m, parts.into_iter().flat_map(|(_, c)| c).collect())
    }
}
