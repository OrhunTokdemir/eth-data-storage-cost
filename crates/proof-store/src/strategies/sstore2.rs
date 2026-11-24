use super::{manifest, register, verify, Ctx, StorageStrategy, StoreReceipt};
use crate::{
    chunk::{chunks, SSTORE2_MAX_CHUNK, SSTORE2_PER_TX},
    contracts::SSTORE2Store,
};
use alloy::{
    primitives::{Address, Bytes, B256},
    providers::Provider,
};

/// Proof stored as bytecode of pointer contracts (≤ 24,575 B each). Permanent, contract-readable.
pub struct Sstore2Strategy;

impl StorageStrategy for Sstore2Strategy {
    fn kind(&self) -> u8 {
        crate::contracts::kind::SSTORE2
    }

    async fn store(&self, ctx: &Ctx, id: B256, data: &[u8], _uri: Option<&str>) -> eyre::Result<StoreReceipt> {
        let store = SSTORE2Store::new(ctx.deployment.sstore2, ctx.provider.clone());
        let parts = chunks(data, SSTORE2_MAX_CHUNK);
        let mut rcpt = StoreReceipt { chunks: parts.len(), ..Default::default() };
        let mut pointers: Vec<(u32, Address)> = Vec::new();

        for (batch_no, batch) in parts.chunks(SSTORE2_PER_TX).enumerate() {
            let first = (batch_no * SSTORE2_PER_TX) as u32;
            let payload: Vec<Bytes> = batch.iter().map(|c| Bytes::copy_from_slice(c)).collect();
            let r = store.write(id, first, payload).send().await?.get_receipt().await?;
            rcpt.push(&r)?;
            for log in r.logs() {
                if let Ok(ev) = log.log_decode::<SSTORE2Store::PointerCreated>() {
                    pointers.push((ev.inner.data.index, ev.inner.data.pointer));
                }
            }
        }
        pointers.sort_by_key(|(i, _)| *i);
        eyre::ensure!(pointers.len() == parts.len(), "expected {} pointers, got {}", parts.len(), pointers.len());
        let refs = pointers.iter().map(|(_, a)| a.into_word()).collect();
        register(ctx, &mut rcpt, id, self.kind(), data, ctx.deployment.sstore2, refs, String::new()).await?;
        Ok(rcpt)
    }

    async fn retrieve(&self, ctx: &Ctx, id: B256) -> eyre::Result<Vec<u8>> {
        let m = manifest(ctx, id, self.kind()).await?;
        let mut out = Vec::with_capacity(m.totalLength as usize);
        for r in &m.refs {
            let ptr = Address::from_word(*r);
            let code = ctx.provider.get_code_at(ptr).await?;
            eyre::ensure!(!code.is_empty() && code[0] == 0x00, "pointer {ptr} has no SSTORE2 data");
            out.extend_from_slice(&code[1..]);
        }
        verify(&m, out)
    }
}
