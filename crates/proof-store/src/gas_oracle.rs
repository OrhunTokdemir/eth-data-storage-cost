//! Live gas prices from the Etherscan V2 gas oracle, with fallbacks.
//!
//! `GET https://api.etherscan.io/v2/api?module=gastracker&action=gasoracle&chainid=<id>&apikey=<key>`

use crate::cost::{PriceSource, PricingConfig, ResolvedPrice, Tier};
use alloy::providers::{Provider, ProviderBuilder};
use serde::Deserialize;

const ETHERSCAN_V2: &str = "https://api.etherscan.io/v2/api";

#[derive(Debug, Deserialize)]
pub struct OracleResponse {
    pub status: String,
    pub message: String,
    pub result: serde_json::Value,
}

/// Parsed `result` object. Etherscan returns every number as a string.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GasOracle {
    #[serde(deserialize_with = "de_str_u64")]
    pub last_block: u64,
    #[serde(deserialize_with = "de_str_f64")]
    pub safe_gas_price: f64,
    #[serde(deserialize_with = "de_str_f64")]
    pub propose_gas_price: f64,
    #[serde(deserialize_with = "de_str_f64")]
    pub fast_gas_price: f64,
    #[serde(rename = "suggestBaseFee", deserialize_with = "de_str_f64")]
    pub suggest_base_fee: f64,
}

impl GasOracle {
    pub fn price(&self, tier: Tier) -> f64 {
        match tier {
            Tier::Safe => self.safe_gas_price,
            Tier::Propose => self.propose_gas_price,
            Tier::Fast => self.fast_gas_price,
        }
    }
}

fn de_str_f64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let s = String::deserialize(d)?;
    s.trim().parse().map_err(serde::de::Error::custom)
}
fn de_str_u64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let s = String::deserialize(d)?;
    s.trim().parse().map_err(serde::de::Error::custom)
}

/// Parse an Etherscan gas-oracle JSON body. Errors if `status != "1"`.
pub fn parse_oracle(body: &str) -> eyre::Result<GasOracle> {
    let r: OracleResponse = serde_json::from_str(body)?;
    if r.status != "1" {
        // On errors `result` is a human-readable string (e.g. "Invalid API Key").
        eyre::bail!("Etherscan returned status {} ({}): {}", r.status, r.message, r.result);
    }
    Ok(serde_json::from_value(r.result)?)
}

/// Fetch the gas oracle for `chain_id`. The API key is never included in error messages.
pub async fn fetch_etherscan(
    client: &reqwest::Client,
    api_key: &str,
    chain_id: u64,
) -> eyre::Result<GasOracle> {
    let resp = client
        .get(ETHERSCAN_V2)
        .query(&[
            ("module", "gastracker"),
            ("action", "gasoracle"),
            ("chainid", &chain_id.to_string()),
            ("apikey", api_key),
        ])
        .send()
        .await
        .map_err(|e| eyre::eyre!("Etherscan request failed: {}", e.without_url()))?;
    let body = resp
        .text()
        .await
        .map_err(|e| eyre::eyre!("Etherscan read failed: {}", e.without_url()))?;
    parse_oracle(&body)
}

async fn rpc_gas_price_gwei(url: &str) -> eyre::Result<f64> {
    let p = ProviderBuilder::new().connect_http(url.parse()?);
    let wei = p.get_gas_price().await?;
    Ok(wei as f64 / 1e9)
}

/// Inputs for [`resolve_prices`].
pub struct LiveOptions<'a> {
    pub use_live: bool,
    pub etherscan_api_key: Option<&'a str>,
    pub sepolia_rpc_url: Option<&'a str>,
}

/// Resolve the gas price for every network in the config.
///
/// * `use_live == false` → defaults from the config.
/// * `use_live == true`  → Etherscan (configured tier); for Sepolia, falls back to
///   `eth_gasPrice` on `sepolia_rpc_url`; on any failure → default, with a warning.
///
/// Returns the prices plus human-readable warnings.
pub async fn resolve_prices(cfg: &PricingConfig, opts: &LiveOptions<'_>) -> (Vec<ResolvedPrice>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let client = reqwest::Client::new();
    let key = opts.etherscan_api_key.filter(|k| !k.trim().is_empty());

    if opts.use_live && key.is_none() {
        warnings.push("ETHERSCAN_API_KEY not set — using default gas prices".to_string());
    }

    for (name, net) in &cfg.networks {
        let mut price = ResolvedPrice::from_default(name, net);
        if opts.use_live {
            let mut done = false;
            if let Some(key) = key {
                match fetch_etherscan(&client, key, net.chain_id).await {
                    Ok(o) => {
                        price.gas_price_gwei = o.price(cfg.live_tier);
                        price.source = PriceSource::Etherscan { tier: cfg.live_tier, last_block: o.last_block };
                        done = true;
                    }
                    Err(e) => warnings.push(format!("{name}: {e}")),
                }
            }
            if !done && name == "sepolia" {
                if let Some(url) = opts.sepolia_rpc_url.filter(|u| !u.trim().is_empty()) {
                    match rpc_gas_price_gwei(url).await {
                        Ok(g) => {
                            price.gas_price_gwei = g;
                            price.source = PriceSource::Rpc;
                            done = true;
                        }
                        Err(e) => warnings.push(format!("sepolia RPC gas price failed: {e}")),
                    }
                }
            }
            if !done && key.is_some() {
                warnings.push(format!("{name}: falling back to default gas price"));
            }
        }
        out.push(price);
    }
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"{  "status": "1",  "message": "OK",  "result": {    "LastBlock": "23467872",    "SafeGasPrice": "0.496839934",    "ProposeGasPrice": "0.496840168",    "FastGasPrice": "0.55411917",    "suggestBaseFee": "0.496839934",    "gasUsedRatio": "0.405942555555556,0.784013777777778,0.502624148542588,0.719479644444444,0.45673524947105"  }}"#;

    #[test]
    fn parses_example_response() {
        let o = parse_oracle(EXAMPLE).unwrap();
        assert_eq!(o.last_block, 23_467_872);
        assert_eq!(o.price(Tier::Safe), 0.496839934);
        assert_eq!(o.price(Tier::Propose), 0.496840168);
        assert_eq!(o.price(Tier::Fast), 0.55411917);
        assert_eq!(o.suggest_base_fee, 0.496839934);
    }

    #[test]
    fn rejects_error_status() {
        let body = r#"{"status":"0","message":"NOTOK","result":"Invalid API Key"}"#;
        let e = parse_oracle(body).unwrap_err().to_string();
        assert!(e.contains("Invalid API Key"));
    }

    #[tokio::test]
    async fn no_live_uses_defaults() {
        let cfg: PricingConfig = serde_json::from_str(
            r#"{"live_tier":"propose","eth_usd":null,"networks":{
                "mainnet":{"chain_id":1,"default_gas_price_gwei":10.0,"default_blob_gas_price_gwei":1.0},
                "sepolia":{"chain_id":11155111,"default_gas_price_gwei":1.0,"default_blob_gas_price_gwei":1.0}}}"#,
        )
        .unwrap();
        let (prices, warnings) =
            resolve_prices(&cfg, &LiveOptions { use_live: false, etherscan_api_key: None, sepolia_rpc_url: None }).await;
        assert!(warnings.is_empty());
        assert!(prices.iter().all(|p| p.source == PriceSource::Default));
        // live requested without a key → defaults + warning, no network call
        let (prices, warnings) =
            resolve_prices(&cfg, &LiveOptions { use_live: true, etherscan_api_key: Some(""), sepolia_rpc_url: None }).await;
        assert_eq!(warnings.len(), 1);
        assert!(prices.iter().all(|p| p.source == PriceSource::Default));
    }
}
