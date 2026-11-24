//! Storage strategies. Each one chunks the proof, writes it, registers a manifest in
//! `ProofRegistry`, and can read it back.

mod blob;
mod calldata;
mod commitment;
mod events;
mod slots;
mod sstore2;

pub use blob::BlobStrategy;
pub use calldata::CalldataStrategy;
pub use commitment::CommitmentStrategy;
pub use events::EventLogStrategy;
pub use slots::SlotStrategy;
pub use sstore2::Sstore2Strategy;

use crate::contracts::{Deployment, ProofRegistry};
use alloy::{
    primitives::{keccak256, Address, B256},
    providers::DynProvider,
    rpc::types::TransactionReceipt,
};
use std::{fmt, str::FromStr};

/// Shared context: a signing provider and the deployed contract addresses.
#[derive(Clone)]
pub struct Ctx {
    pub provider: DynProvider,
    pub deployment: Deployment,
}

/// Gas accounting for everything one `store` call sent.
#[derive(Debug, Clone, Default)]
pub struct StoreReceipt {
    pub tx_hashes: Vec<B256>,
    /// Execution gas (includes calldata, code deposit, registry write).
    pub gas_used: u64,
    /// Blob gas (only for the blob strategy).
    pub blob_gas_used: u64,
    pub chunks: usize,
}

impl StoreReceipt {
    pub fn push(&mut self, r: &TransactionReceipt) -> eyre::Result<()> {
        eyre::ensure!(r.status(), "transaction {} reverted", r.transaction_hash);
        self.tx_hashes.push(r.transaction_hash);
        self.gas_used += r.gas_used;
        self.blob_gas_used += r.blob_gas_used.unwrap_or(0);
        Ok(())
    }
}

/// Common interface every storage option implements.
pub trait StorageStrategy {
    /// Registry kind this strategy registers under.
    fn kind(&self) -> u8;

    /// Store `data` for `election_id`; returns gas accounting. `uri` is only used by the
    /// commitment strategy (where the data lives off-chain).
    fn store(
        &self,
        ctx: &Ctx,
        election_id: B256,
        data: &[u8],
        uri: Option<&str>,
    ) -> impl std::future::Future<Output = eyre::Result<StoreReceipt>> + Send;

    /// Read the proof back using only the chain (+ the off-chain URI for commitments).
    /// Implementations must check length and keccak against the manifest.
    fn retrieve(&self, ctx: &Ctx, election_id: B256) -> impl std::future::Future<Output = eyre::Result<Vec<u8>>> + Send;
}

/// All strategies, for CLI selection and dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Sstore2,
    Calldata,
    Events,
    Slots,
    Blob,
    Commitment,
}

impl Strategy {
    pub const ALL: [Strategy; 6] = [
        Strategy::Sstore2,
        Strategy::Calldata,
        Strategy::Events,
        Strategy::Slots,
        Strategy::Blob,
        Strategy::Commitment,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Strategy::Sstore2 => "sstore2",
            Strategy::Calldata => "calldata",
            Strategy::Events => "events",
            Strategy::Slots => "slots",
            Strategy::Blob => "blob",
            Strategy::Commitment => "commitment",
        }
    }

    /// Where the bytes end up and how long they survive.
    pub fn permanence(&self) -> &'static str {
        match self {
            Strategy::Sstore2 => "state (permanent)",
            Strategy::Calldata => "tx history (EIP-4444 expiry)",
            Strategy::Events => "receipts (EIP-4444 expiry)",
            Strategy::Slots => "state (permanent)",
            Strategy::Blob => "~18 days; hash permanent",
            Strategy::Commitment => "off-chain; hash permanent",
        }
    }

    pub async fn store(&self, ctx: &Ctx, id: B256, data: &[u8], uri: Option<&str>) -> eyre::Result<StoreReceipt> {
        match self {
            Strategy::Sstore2 => Sstore2Strategy.store(ctx, id, data, uri).await,
            Strategy::Calldata => CalldataStrategy.store(ctx, id, data, uri).await,
            Strategy::Events => EventLogStrategy.store(ctx, id, data, uri).await,
            Strategy::Slots => SlotStrategy.store(ctx, id, data, uri).await,
            Strategy::Blob => BlobStrategy.store(ctx, id, data, uri).await,
            Strategy::Commitment => CommitmentStrategy.store(ctx, id, data, uri).await,
        }
    }

    pub async fn retrieve(&self, ctx: &Ctx, id: B256) -> eyre::Result<Vec<u8>> {
        match self {
            Strategy::Sstore2 => Sstore2Strategy.retrieve(ctx, id).await,
            Strategy::Calldata => CalldataStrategy.retrieve(ctx, id).await,
            Strategy::Events => EventLogStrategy.retrieve(ctx, id).await,
            Strategy::Slots => SlotStrategy.retrieve(ctx, id).await,
            Strategy::Blob => BlobStrategy.retrieve(ctx, id).await,
            Strategy::Commitment => CommitmentStrategy.retrieve(ctx, id).await,
        }
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Strategy {
    type Err = eyre::Report;
    fn from_str(s: &str) -> eyre::Result<Self> {
        Strategy::ALL
            .into_iter()
            .find(|x| x.name().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| eyre::eyre!("unknown strategy {s:?}; one of sstore2,calldata,events,slots,blob,commitment"))
    }
}

/// Write the write-once manifest for this proof.
pub(crate) async fn register(
    ctx: &Ctx,
    rcpt: &mut StoreReceipt,
    id: B256,
    kind: u8,
    data: &[u8],
    store: Address,
    refs: Vec<B256>,
    uri: String,
) -> eyre::Result<()> {
    let reg = ProofRegistry::new(ctx.deployment.registry, ctx.provider.clone());
    let r = reg
        .register(id, kind, data.len().try_into()?, rcpt.chunks.try_into()?, keccak256(data), store, refs, uri)
        .send()
        .await?
        .get_receipt()
        .await?;
    rcpt.push(&r)
}

pub(crate) async fn manifest(ctx: &Ctx, id: B256, kind: u8) -> eyre::Result<ProofRegistry::Manifest> {
    let reg = ProofRegistry::new(ctx.deployment.registry, ctx.provider.clone());
    let m = reg.get(id, kind).call().await?;
    eyre::ensure!(m.createdAt != 0, "no manifest registered for this election/strategy");
    Ok(m)
}

/// Final integrity check against the on-chain manifest.
pub(crate) fn verify(m: &ProofRegistry::Manifest, data: Vec<u8>) -> eyre::Result<Vec<u8>> {
    eyre::ensure!(data.len() == m.totalLength as usize, "length mismatch: got {}, manifest {}", data.len(), m.totalLength);
    eyre::ensure!(keccak256(&data) == m.dataHash, "keccak256 mismatch against on-chain manifest");
    Ok(data)
}
