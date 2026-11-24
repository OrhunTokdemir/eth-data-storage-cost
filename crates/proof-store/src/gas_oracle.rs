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

/// Parse an Etherscan `proxy/eth_gasPrice` body (`{"jsonrpc":"2.0","id":73,"result":"0x…"}`, hex wei)
/// into gwei. Error bodies (`{"status":"0","message":"NOTOK","result":"…"}` or a JSON-RPC
/// `error` object) are returned as errors.
pub fn parse_proxy_gas_price(body: &str) -> eyre::Result<f64> {
    let v: serde_json::Value = serde_json::from_str(body)?;
    if let Some(hex) = v.get("result").and_then(|r| r.as_str()).and_then(|r| r.strip_prefix("0x")) {
        let wei = u128::from_str_radix(hex, 16)?;
        return Ok(wei as f64 / 1e9);
    }
    let msg = v.get("error").or_else(|| v.get("result")).cloned().unwrap_or(v);
    eyre::bail!("Etherscan eth_gasPrice failed: {msg}")
}

/// Current gas price (gwei) via Etherscan's JSON-RPC proxy — works on chains where the
/// gas oracle is unsupported (e.g. Sepolia). The API key is never included in errors.
pub async fn fetch_etherscan_gas_price(client: &reqwest::Client, api_key: &str, chain_id: u64) -> eyre::Result<f64> {
    let resp = client
        .get(ETHERSCAN_V2)
        .query(&[
            ("module", "proxy"),
            ("action", "eth_gasPrice"),
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
    parse_proxy_gas_price(&body)
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
            // Try each source in order; only warn if all of them fail.
            let mut errors: Vec<String> = Vec::new();
            let mut done = false;
            if let Some(key) = key {
                // 1. gas oracle (safe/propose/fast tiers; mainnet only in practice)
                match fetch_etherscan(&client, key, net.chain_id).await {
                    Ok(o) => {
                        price.gas_price_gwei = o.price(cfg.live_tier);
                        price.source = PriceSource::Etherscan { tier: cfg.live_tier, last_block: o.last_block };
                        done = true;
                    }
                    Err(e) => errors.push(format!("gasoracle: {e}")),
                }
                // 2. JSON-RPC proxy eth_gasPrice (Sepolia and other chains without the oracle)
                if !done {
                    match fetch_etherscan_gas_price(&client, key, net.chain_id).await {
                        Ok(g) => {
                            price.gas_price_gwei = g;
                            price.source = PriceSource::EtherscanProxy;
                            done = true;
                        }
                        Err(e) => errors.push(format!("proxy: {e}")),
                    }
                }
            }
            // 3. user-supplied Sepolia RPC
            if !done && name == "sepolia" {
                if let Some(url) = opts.sepolia_rpc_url.filter(|u| !u.trim().is_empty()) {
                    match rpc_gas_price_gwei(url).await {
                        Ok(g) => {
                            price.gas_price_gwei = g;
                            price.source = PriceSource::Rpc;
                            done = true;
                        }
                        Err(e) => errors.push(format!("SEPOLIA_RPC_URL: {e}")),
                    }
                }
            }
            // 4. config default
            if !done && key.is_some() {
                warnings.push(format!("{name}: using default gas price ({})", errors.join("; ")));
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
    fn parses_proxy_gas_price() {
        let g = parse_proxy_gas_price(r#"{"jsonrpc":"2.0","id":73,"result":"0x3b9aca00"}"#).unwrap();
        assert_eq!(g, 1.0);
        assert!(parse_proxy_gas_price(r#"{"status":"0","message":"NOTOK","result":"Invalid API Key"}"#).is_err());
        assert!(parse_proxy_gas_price(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"x"}}"#).is_err());
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
