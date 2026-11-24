use super::{manifest, register, verify, Ctx, StorageStrategy, StoreReceipt};
use crate::{
    chunk::{chunks, SLOT_CHUNK},
    contracts::SlotStore,
};
use alloy::primitives::{Bytes, B256, U256};

/// Baseline: proof in raw 32-byte storage slots. Permanent but ~20× pricier than SSTORE2.
pub struct SlotStrategy;

impl StorageStrategy for SlotStrategy {
    fn kind(&self) -> u8 {
        crate::contracts::kind::SLOTS
    }

    async fn store(&self, ctx: &Ctx, id: B256, data: &[u8], _uri: Option<&str>) -> eyre::Result<StoreReceipt> {
        let store = SlotStore::new(ctx.deployment.slots, ctx.provider.clone());
        let parts = chunks(data, SLOT_CHUNK);
        let mut rcpt = StoreReceipt { chunks: parts.len(), ..Default::default() };
        for (i, c) in parts.iter().enumerate() {
            let start_word = U256::from(i * SLOT_CHUNK / 32);
            let r = store.write(id, start_word, Bytes::copy_from_slice(c)).send().await?.get_receipt().await?;
            rcpt.push(&r)?;
        }
        register(ctx, &mut rcpt, id, self.kind(), data, ctx.deployment.slots, vec![], String::new()).await?;
        Ok(rcpt)
    }

    async fn retrieve(&self, ctx: &Ctx, id: B256) -> eyre::Result<Vec<u8>> {
        let m = manifest(ctx, id, self.kind()).await?;
        let store = SlotStore::new(ctx.deployment.slots, ctx.provider.clone());
        let total = m.totalLength as usize;
        let mut out = Vec::with_capacity(total);
        let mut off = 0;
        while off < total {
            let len = SLOT_CHUNK.min(total - off);
            let b = store.read(id, U256::from(off / 32), U256::from(len)).call().await?;
            out.extend_from_slice(&b);
            off += len;
        }
        verify(&m, out)
    }
}
