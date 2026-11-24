//! Solidity bindings generated from the Foundry artifacts in `contracts/out`
//! (run `forge build` in `contracts/` before `cargo build`).

use alloy::{primitives::Address, providers::DynProvider};
use serde::{Deserialize, Serialize};

alloy::sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    ProofRegistry,
    "../../contracts/out/ProofRegistry.sol/ProofRegistry.json"
);
alloy::sol!(
    #[sol(rpc)]
    SSTORE2Store,
    "../../contracts/out/SSTORE2Store.sol/SSTORE2Store.json"
);
alloy::sol!(
    #[sol(rpc)]
    CalldataStore,
    "../../contracts/out/CalldataStore.sol/CalldataStore.json"
);
alloy::sol!(
    #[sol(rpc)]
    EventLogStore,
    "../../contracts/out/EventLogStore.sol/EventLogStore.json"
);
alloy::sol!(
    #[sol(rpc)]
    SlotStore,
    "../../contracts/out/SlotStore.sol/SlotStore.json"
);
alloy::sol!(
    #[sol(rpc)]
    BlobAnchor,
    "../../contracts/out/BlobAnchor.sol/BlobAnchor.json"
);

/// `ProofRegistry.Kind` enum values (the JSON ABI encodes Solidity enums as `uint8`).
pub mod kind {
    pub const SSTORE2: u8 = 0;
    pub const CALLDATA: u8 = 1;
    pub const EVENTS: u8 = 2;
    pub const SLOTS: u8 = 3;
    pub const BLOB: u8 = 4;
    pub const COMMITMENT: u8 = 5;
}

/// Addresses of the deployed contracts on one chain.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Deployment {
    pub chain_id: u64,
    pub registry: Address,
    pub sstore2: Address,
    pub calldata: Address,
    pub events: Address,
    pub slots: Address,
    pub blob_anchor: Address,
}

impl Deployment {
    pub async fn deploy(p: &DynProvider) -> eyre::Result<Self> {
        use alloy::providers::Provider;
        Ok(Self {
            chain_id: p.get_chain_id().await?,
            registry: *ProofRegistry::deploy(p.clone()).await?.address(),
            sstore2: *SSTORE2Store::deploy(p.clone()).await?.address(),
            calldata: *CalldataStore::deploy(p.clone()).await?.address(),
            events: *EventLogStore::deploy(p.clone()).await?.address(),
            slots: *SlotStore::deploy(p.clone()).await?.address(),
            blob_anchor: *BlobAnchor::deploy(p.clone()).await?.address(),
        })
    }

    pub fn path_for(chain_id: u64) -> std::path::PathBuf {
        format!("deployments/{chain_id}.json").into()
    }

    pub fn save(&self) -> eyre::Result<std::path::PathBuf> {
        let path = Self::path_for(self.chain_id);
        std::fs::create_dir_all("deployments")?;
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(path)
    }

    pub fn load(chain_id: u64) -> eyre::Result<Self> {
        let path = Self::path_for(chain_id);
        let s = std::fs::read_to_string(&path)
            .map_err(|e| eyre::eyre!("{}: {e} (run `proof-store deploy` first)", path.display()))?;
        Ok(serde_json::from_str(&s)?)
    }
}
