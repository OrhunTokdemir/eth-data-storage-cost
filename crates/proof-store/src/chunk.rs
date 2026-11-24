//! Chunking helpers and protocol limits.

/// EIP-170 max runtime code size minus the 1-byte STOP prefix used by SSTORE2.
pub const SSTORE2_MAX_CHUNK: usize = 24_575;
/// Keep calldata/event txs under the ~128 KiB txpool tx-size limit (geth `txMaxSize`).
pub const CALLDATA_CHUNK: usize = 120_000;
/// 512 words × ~22.1k gas ≈ 11.3M gas, under the EIP-7825 per-tx cap.
pub const SLOT_CHUNK: usize = 16_384;
/// SSTORE2 pointers deployed per tx (each full pointer ≈ 5.4M gas; cap is 16,777,216).
pub const SSTORE2_PER_TX: usize = 2;
/// Blobs per type-3 tx (EIP-7594 per-tx max).
pub const MAX_BLOBS_PER_TX: usize = 6;
/// EIP-7825 per-transaction gas cap.
pub const TX_GAS_CAP: u64 = 1 << 24;

pub fn chunks(data: &[u8], size: usize) -> Vec<&[u8]> {
    if data.is_empty() {
        return vec![];
    }
    data.chunks(size).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunking_covers_all_bytes() {
        let d = vec![7u8; 330_000];
        let c = chunks(&d, SSTORE2_MAX_CHUNK);
        assert_eq!(c.len(), 14);
        assert_eq!(c.iter().map(|x| x.len()).sum::<usize>(), d.len());
        assert_eq!(chunks(&d, CALLDATA_CHUNK).len(), 3);
    }
}
