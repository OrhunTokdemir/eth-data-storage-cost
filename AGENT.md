# AGENT.md — instructions for AI coding agents

## 🔒 Secrets — hard rules (read first)

- **Do NOT read, open, `cat`, `less`, `grep`, `rg`, `head`, `tail`, `source`, print, diff, upload or otherwise inspect `.env`**, or any `.env.*` file **other than `.env.example`**.
- **Do NOT read or inspect anything inside `secrets/`** (any depth), nor list its contents.
- Do not echo environment variables that may hold secrets (`PRIVATE_KEY`, `ETHERSCAN_API_KEY`, RPC URLs with keys) into logs, commits, test output or chat.
- Never commit `.env`, `secrets/`, or `proofs/`. Check `git status` before every commit.
- ✅ You **may** read and edit **`.env.example`**. It must only ever contain placeholders or the public Anvil dev key — never a real key.
- If a task seems to require a secret value, ask the human instead of looking for it.

## Project overview

Benchmarks and implements ways to store a ~330 KB recursive STARK proof on Ethereum
(SSTORE2, calldata, event logs, raw storage slots, blobs + anchor, hash commitment).
See `README.md` for the analysis.

```
contracts/           Foundry project (Solidity storage contracts + tests)
crates/proof-store/  Rust library: StorageStrategy trait, chunking, strategies, cost model
crates/cli/          `proof-store` binary (bench / store / retrieve)
config/gas_pricing.json  default gas prices (gwei/gas) for mainnet & sepolia, live tier, ETH/USD
proofs/              local proof files (gitignored)
```

## Commands

```bash
cd contracts && forge install foundry-rs/forge-std --no-git  # once
cd contracts && forge build && forge test --gas-report   # contracts (must build before Rust)
cargo build                                              # Rust (reads contracts/out/*.json)
cargo test                                               # unit + Anvil integration tests
anvil &                                                  # local chain
cargo run -p cli -- bench --proof random:330000 --no-live-gas
```

## Conventions

- New storage options implement the `StorageStrategy` trait in `crates/proof-store/src/strategies/`
  and must round-trip (store → retrieve → byte-for-byte equal + keccak check).
- Respect protocol limits: 24,575 data bytes per SSTORE2 pointer (EIP-170),
  ≤ 16,777,216 gas per tx (EIP-7825), ≤ ~128 KB per tx (txpool).
- Tests must never hit the network (use `--no-live-gas` / `LivePolicy::Never`).
- Gas prices in config are **gwei per gas**; gwei→ETH is the constant 1e-9 in code.
