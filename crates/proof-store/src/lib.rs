//! Store a large (~330 KB) proof on Ethereum using several strategies and compare their cost.
//!
//! Every strategy chunks the proof to fit protocol limits, writes it, registers a write-once
//! [`contracts::ProofRegistry`] manifest (length + keccak256), and can read it back for a
//! byte-for-byte check.

#![allow(clippy::too_many_arguments)] // generated contract bindings + registry call mirror the ABI

pub mod chunk;
pub mod contracts;
pub mod cost;
pub mod gas_oracle;
pub mod strategies;

use alloy::primitives::{keccak256, B256};

pub use strategies::{Strategy, StoreReceipt, StorageStrategy};

/// Derive a 32-byte election id: a `0x`-prefixed 32-byte hex string is used verbatim,
/// anything else is hashed with keccak256.
pub fn election_id(label: &str) -> B256 {
    if let Ok(b) = label.parse::<B256>() {
        if label.starts_with("0x") && label.len() == 66 {
            return b;
        }
    }
    keccak256(label.as_bytes())
}

/// Load proof bytes from a file path, or generate random bytes with `random:<len>`.
pub fn load_proof(spec: &str) -> eyre::Result<Vec<u8>> {
    if let Some(n) = spec.strip_prefix("random:") {
        let len: usize = n.parse()?;
        let mut v = vec![0u8; len];
        rand::fill(&mut v[..]);
        Ok(v)
    } else {
        std::fs::read(spec).map_err(|e| eyre::eyre!("cannot read proof file {spec}: {e}"))
    }
}

/// zstd compression ratio (compressed/original) at level 19 — STARK proofs are mostly field
/// elements and usually compress poorly, but it's worth checking for your proof format.
pub fn zstd_ratio(data: &[u8]) -> eyre::Result<f64> {
    let c = zstd::bulk::compress(data, 19)?;
    Ok(c.len() as f64 / data.len().max(1) as f64)
}
