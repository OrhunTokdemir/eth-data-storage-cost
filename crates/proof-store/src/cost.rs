//! Gas → ETH cost model.
//!
//! All prices are **gas prices in gwei per unit of gas**. The gwei→ETH conversion is fixed.

use serde::Deserialize;
use std::{collections::BTreeMap, fmt, path::Path};

/// 1 gwei = 1e-9 ETH. Fixed by definition; not configurable.
pub const GWEI_TO_ETH: f64 = 1e-9;

/// Which Etherscan gas-oracle field to use when live prices are enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Safe,
    Propose,
    Fast,
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Tier::Safe => "safe",
            Tier::Propose => "propose",
            Tier::Fast => "fast",
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkPricing {
    pub chain_id: u64,
    pub default_gas_price_gwei: f64,
    pub default_blob_gas_price_gwei: f64,
}

/// Contents of `config/gas_pricing.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct PricingConfig {
    pub live_tier: Tier,
    /// Mainnet ETH price in USD; `null` hides the USD column.
    pub eth_usd: Option<f64>,
    pub networks: BTreeMap<String, NetworkPricing>,
}

impl PricingConfig {
    pub fn load(path: impl AsRef<Path>) -> eyre::Result<Self> {
        let path = path.as_ref();
        let s = std::fs::read_to_string(path)
            .map_err(|e| eyre::eyre!("cannot read pricing config {}: {e}", path.display()))?;
        Ok(serde_json::from_str(&s)?)
    }
}

/// Where a resolved gas price came from.
#[derive(Debug, Clone, PartialEq)]
pub enum PriceSource {
    /// Etherscan gas oracle (`gastracker/gasoracle`).
    Etherscan { tier: Tier, last_block: u64 },
    /// `eth_gasPrice` from an RPC endpoint (Sepolia fallback).
    Rpc,
    /// `default_gas_price_gwei` from the config file.
    Default,
}

impl fmt::Display for PriceSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PriceSource::Etherscan { tier, last_block } => {
                write!(f, "Etherscan {tier}, block {last_block}")
            }
            PriceSource::Rpc => f.write_str("RPC eth_gasPrice"),
            PriceSource::Default => f.write_str("default"),
        }
    }
}

/// Gas price chosen for one network.
#[derive(Debug, Clone)]
pub struct ResolvedPrice {
    pub network: String,
    pub chain_id: u64,
    pub gas_price_gwei: f64,
    /// Blob gas is always taken from the config (Etherscan has no blob base fee).
    pub blob_gas_price_gwei: f64,
    pub source: PriceSource,
}

impl ResolvedPrice {
    pub fn from_default(network: &str, n: &NetworkPricing) -> Self {
        Self {
            network: network.to_string(),
            chain_id: n.chain_id,
            gas_price_gwei: n.default_gas_price_gwei,
            blob_gas_price_gwei: n.default_blob_gas_price_gwei,
            source: PriceSource::Default,
        }
    }

    /// ETH cost of `gas` execution gas plus `blob_gas` blob gas.
    pub fn eth_cost(&self, gas: u64, blob_gas: u64) -> f64 {
        gas as f64 * self.gas_price_gwei * GWEI_TO_ETH
            + blob_gas as f64 * self.blob_gas_price_gwei * GWEI_TO_ETH
    }

    /// e.g. `0.497 gwei/gas (Etherscan propose, block 23467872)`
    pub fn describe(&self) -> String {
        format!("{:.3} gwei/gas ({})", self.gas_price_gwei, self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eth_cost_math() {
        let p = ResolvedPrice {
            network: "mainnet".into(),
            chain_id: 1,
            gas_price_gwei: 10.0,
            blob_gas_price_gwei: 1.0,
            source: PriceSource::Default,
        };
        // 1M gas at 10 gwei = 0.01 ETH; 131072 blob gas at 1 gwei = 0.000131072 ETH
        assert!((p.eth_cost(1_000_000, 0) - 0.01).abs() < 1e-12);
        assert!((p.eth_cost(0, 131_072) - 0.000131072).abs() < 1e-12);
    }

    #[test]
    fn repo_config_parses() {
        let cfg = PricingConfig::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/gas_pricing.json")).unwrap();
        assert_eq!(cfg.networks["mainnet"].chain_id, 1);
        assert_eq!(cfg.networks["sepolia"].chain_id, 11_155_111);
    }
}
