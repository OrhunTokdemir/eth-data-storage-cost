//! Human-readable output: one table, gas measured once, priced for mainnet and Sepolia.

use comfy_table::{presets::UTF8_FULL_CONDENSED, Cell, CellAlignment, ContentArrangement, Table};
use proof_store::{cost::{fmt_small, ResolvedPrice}, StoreReceipt, Strategy};

#[derive(Clone, Copy, PartialEq)]
pub enum NetworkFilter {
    Mainnet,
    Sepolia,
    Both,
}

impl NetworkFilter {
    fn includes(&self, network: &str) -> bool {
        match self {
            NetworkFilter::Both => true,
            NetworkFilter::Mainnet => network == "mainnet",
            NetworkFilter::Sepolia => network == "sepolia",
        }
    }
}

pub struct BenchRow {
    pub strategy: Strategy,
    pub receipt: StoreReceipt,
    pub roundtrip_ok: bool,
}

/// 1234567 -> "1,234,567"
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn label(network: &str) -> String {
    if network == "sepolia" {
        "Sepolia ETH (test)".to_string()
    } else {
        let mut c = network.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() + " ETH"
    }
}

pub fn print_header(len: usize, zstd: f64, prices: &[ResolvedPrice], filter: NetworkFilter) {
    println!();
    println!("Proof size : {} bytes ({:.1} KB)", thousands(len as u64), len as f64 / 1000.0);
    println!("zstd -19   : {:.1}% of original", zstd * 100.0);
    let p: Vec<String> = prices
        .iter()
        .filter(|p| filter.includes(&p.network))
        .map(|p| format!("{} {} (blob {} gwei)", p.network, p.describe(), fmt_small(p.blob_gas_price_gwei, 3)))
        .collect();
    println!("Gas prices : {}", p.join("  |  "));
    println!("ETH cost   = gas used × gas price (gwei) × 1e-9  (+ blob gas × blob gas price × 1e-9)");
    println!();
}

pub fn print_table(rows: &[BenchRow], prices: &[ResolvedPrice], eth_usd: Option<f64>, filter: NetworkFilter) {
    let nets: Vec<&ResolvedPrice> = prices.iter().filter(|p| filter.includes(&p.network)).collect();

    let mut header = vec!["Strategy", "Txs", "Gas used", "Blob gas"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    for n in &nets {
        header.push(label(&n.network));
        if n.network == "mainnet" && eth_usd.is_some() {
            header.push("Mainnet USD".into());
        }
    }
    header.extend(["Where it lives".to_string(), "Roundtrip".to_string()]);

    let mut t = Table::new();
    t.load_style(UTF8_FULL_CONDENSED).set_content_arrangement(ContentArrangement::Dynamic).set_header(header.clone());

    for r in rows {
        let mut row = vec![
            Cell::new(r.strategy.name()),
            Cell::new(r.receipt.tx_hashes.len()),
            Cell::new(thousands(r.receipt.gas_used)),
            Cell::new(thousands(r.receipt.blob_gas_used)),
        ];
        for n in &nets {
            let eth = n.eth_cost(r.receipt.gas_used, r.receipt.blob_gas_used);
            row.push(Cell::new(fmt_small(eth, 6)));
            if n.network == "mainnet" {
                if let Some(usd) = eth_usd {
                    row.push(Cell::new(format!("${:.2}", eth * usd)));
                }
            }
        }
        row.push(Cell::new(r.strategy.permanence()));
        row.push(Cell::new(if r.roundtrip_ok { "OK" } else { "FAIL" }));
        t.add_row(row);
    }
    // right-align numeric columns (everything between Strategy and "Where it lives")
    for i in 1..header.len() - 2 {
        if let Some(c) = t.column_mut(i) {
            c.set_cell_alignment(CellAlignment::Right);
        }
    }
    println!("{t}");
}

#[cfg(test)]
mod tests {
    use super::thousands;
    #[test]
    fn formats_thousands() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(72_564_130), "72,564,130");
    }
}
