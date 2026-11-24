//! `proof-store` CLI: benchmark, deploy, store and retrieve a proof on Ethereum.

mod report;

use alloy::{
    node_bindings::Anvil,
    providers::{DynProvider, Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use proof_store::{
    contracts::Deployment,
    cost::{PricingConfig, ResolvedPrice},
    election_id,
    gas_oracle::{resolve_prices, LiveOptions},
    load_proof,
    strategies::Ctx,
    zstd_ratio, Strategy,
};
use report::{BenchRow, NetworkFilter};
use std::io::IsTerminal;

#[derive(Parser)]
#[command(name = "proof-store", version, about = "Store a large ZK proof on Ethereum and compare costs")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Measure gas for every strategy on a fresh local Anvil and price it for mainnet + Sepolia.
    Bench {
        #[command(flatten)]
        common: Common,
        /// Comma-separated strategies or `all`.
        #[arg(long, default_value = "all")]
        strategy: String,
        /// Use an existing node instead of spawning Anvil (needs PRIVATE_KEY).
        #[arg(long)]
        rpc: Option<String>,
    },
    /// Deploy the storage contracts to RPC_URL and save `deployments/<chain_id>.json`.
    Deploy,
    /// Store a proof with one strategy on RPC_URL (contracts must be deployed).
    Store {
        #[command(flatten)]
        common: Common,
        #[arg(long)]
        strategy: Strategy,
        /// Election label or 0x-prefixed 32-byte id.
        #[arg(long)]
        election: String,
        /// Off-chain location (commitment strategy only): ar://, ipfs://, https://, file://
        #[arg(long)]
        uri: Option<String>,
    },
    /// Read a stored proof back from RPC_URL and verify it against the on-chain manifest.
    Retrieve {
        #[arg(long)]
        strategy: Strategy,
        #[arg(long)]
        election: String,
        #[arg(long, default_value = "retrieved-proof.bin")]
        out: String,
    },
}

#[derive(Args)]
struct Common {
    /// Proof file, or `random:<bytes>`. Defaults to $PROOF_PATH, then proofs/proof.bin.
    #[arg(long, env = "PROOF_PATH", default_value = "proofs/proof.bin")]
    proof: String,
    /// Gas pricing config.
    #[arg(long, default_value = "config/gas_pricing.json")]
    pricing: String,
    /// Which networks to price.
    #[arg(long, value_enum, default_value_t = NetworkArg::Both)]
    network: NetworkArg,
    /// Use live Etherscan gas prices without asking.
    #[arg(long, conflicts_with = "no_live_gas")]
    live_gas: bool,
    /// Use default gas prices from the config without asking.
    #[arg(long)]
    no_live_gas: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum NetworkArg {
    Mainnet,
    Sepolia,
    Both,
}

impl From<NetworkArg> for NetworkFilter {
    fn from(n: NetworkArg) -> Self {
        match n {
            NetworkArg::Mainnet => NetworkFilter::Mainnet,
            NetworkArg::Sepolia => NetworkFilter::Sepolia,
            NetworkArg::Both => NetworkFilter::Both,
        }
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // The program (not the agent) loads .env for RPC_URL / PRIVATE_KEY / ETHERSCAN_API_KEY.
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Bench { common, strategy, rpc } => bench(common, &strategy, rpc).await,
        Cmd::Deploy => {
            let p = env_provider().await?;
            let d = Deployment::deploy(&p).await?;
            println!("deployed: {d:#?}\nsaved to {}", d.save()?.display());
            Ok(())
        }
        Cmd::Store { common, strategy, election, uri } => store(common, strategy, &election, uri).await,
        Cmd::Retrieve { strategy, election, out } => {
            let p = env_provider().await?;
            let ctx = Ctx { deployment: Deployment::load(p.get_chain_id().await?)?, provider: p };
            let data = strategy.retrieve(&ctx, election_id(&election)).await?;
            std::fs::write(&out, &data)?;
            println!("retrieved {} bytes via {strategy}, keccak verified against manifest -> {out}", data.len());
            Ok(())
        }
    }
}

/// Ask (or read flags) whether to use live gas prices, then resolve prices for each network.
/// Runs before anything is stored.
async fn gas_prices(common: &Common) -> eyre::Result<(PricingConfig, Vec<ResolvedPrice>)> {
    let cfg = PricingConfig::load(&common.pricing)?;
    let use_live = if common.live_gas {
        true
    } else if common.no_live_gas {
        false
    } else if std::io::stdin().is_terminal() {
        dialoguer::Confirm::new()
            .with_prompt("Use live gas price from Etherscan?")
            .default(false)
            .interact()?
    } else {
        eprintln!("(non-interactive: using default gas prices; pass --live-gas to fetch live ones)");
        false
    };
    let key = std::env::var("ETHERSCAN_API_KEY").ok();
    let sepolia_rpc = std::env::var("SEPOLIA_RPC_URL").ok();
    let (prices, warnings) = resolve_prices(
        &cfg,
        &LiveOptions { use_live, etherscan_api_key: key.as_deref(), sepolia_rpc_url: sepolia_rpc.as_deref() },
    )
    .await;
    for w in warnings {
        eprintln!("warning: {w}");
    }
    Ok((cfg, prices))
}

async fn bench(common: Common, strategy: &str, rpc: Option<String>) -> eyre::Result<()> {
    let strategies: Vec<Strategy> = if strategy.trim() == "all" {
        Strategy::ALL.to_vec()
    } else {
        strategy.split(',').map(str::parse).collect::<eyre::Result<_>>()?
    };
    let proof = load_proof(&common.proof)?; // fail fast, before asking about gas prices
    let (cfg, prices) = gas_prices(&common).await?;

    // Gas is identical on every network, so measure once locally and price per network.
    let _anvil; // keep the spawned node alive for the whole run
    let provider: DynProvider = match rpc {
        Some(url) => key_provider(&url)?,
        None => {
            let a = Anvil::new().args(["--hardfork", "prague", "--enable-tx-gas-limit"]).try_spawn()?;
            let signer: PrivateKeySigner = a.keys()[0].clone().into();
            let p = ProviderBuilder::new().wallet(signer).connect_http(a.endpoint_url()).erased();
            _anvil = a;
            p
        }
    };
    let ctx = Ctx { deployment: Deployment::deploy(&provider).await?, provider };

    // The commitment strategy needs an off-chain copy: write one locally for the benchmark.
    std::fs::create_dir_all("target")?;
    let local = std::fs::canonicalize("target")?.join("bench-proof.bin");
    std::fs::write(&local, &proof)?;
    let uri = format!("file://{}", local.display());

    let run = format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos());
    let mut rows = Vec::new();
    for s in strategies {
        eprintln!("running {s} ...");
        let id = election_id(&format!("bench-{run}-{s}"));
        let r = s.store(&ctx, id, &proof, Some(&uri)).await?;
        let ok = s.retrieve(&ctx, id).await.map(|b| b == proof).unwrap_or(false);
        rows.push(BenchRow { strategy: s, receipt: r, roundtrip_ok: ok });
    }

    report::print_header(proof.len(), zstd_ratio(&proof)?, &prices, common.network.into());
    report::print_table(&rows, &prices, cfg.eth_usd, common.network.into());
    Ok(())
}

async fn store(common: Common, strategy: Strategy, election: &str, uri: Option<String>) -> eyre::Result<()> {
    let proof = load_proof(&common.proof)?; // fail fast, before asking about gas prices
    let (cfg, prices) = gas_prices(&common).await?;
    let p = env_provider().await?;
    let ctx = Ctx { deployment: Deployment::load(p.get_chain_id().await?)?, provider: p };
    let r = strategy.store(&ctx, election_id(election), &proof, uri.as_deref()).await?;
    report::print_header(proof.len(), zstd_ratio(&proof)?, &prices, common.network.into());
    let rows = [BenchRow { strategy, receipt: r, roundtrip_ok: true }];
    report::print_table(&rows, &prices, cfg.eth_usd, common.network.into());
    for h in &rows[0].receipt.tx_hashes {
        println!("tx {h}");
    }
    Ok(())
}

fn key_provider(url: &str) -> eyre::Result<DynProvider> {
    let key = std::env::var("PRIVATE_KEY").map_err(|_| eyre::eyre!("PRIVATE_KEY not set (see .env.example)"))?;
    let signer: PrivateKeySigner = key.parse()?;
    Ok(ProviderBuilder::new().wallet(signer).connect_http(url.parse()?).erased())
}

async fn env_provider() -> eyre::Result<DynProvider> {
    let url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".into());
    key_provider(&url)
}
