# Eth-Data-storage-Test-Rust

How to store a **~330 KB recursive, aggregated ZK-STARK proof** on Ethereum, permanently and
cheaply, so it can serve as an artifact for **end-to-end verifiable e-voting**.

This repo implements six storage strategies in Rust ([alloy](https://github.com/alloy-rs/alloy)) and
Solidity ([Foundry](https://getfoundry.sh)). It measures the real gas each one uses on a local Anvil
chain (Prague rules, with the EIP-7825 per-tx gas cap enforced), reads the proof back, checks it
byte for byte, and prices the result for **Ethereum mainnet** and the **Sepolia** testnet.

---

## TL;DR

| | Strategy | Permanent? | Gas (330 KB) | Mainnet @ 0.5 gwei | @ 2 gwei | @ 10 gwei | Sepolia @ 1 gwei |
|---|---|---|---:|---:|---:|---:|---:|
| 🥇 | **Calldata** + on-chain manifest | History (see EIP-4444) | 13,351,985 | 0.0067 ETH | 0.0267 ETH | 0.1335 ETH | 0.0134 |
| | **Event logs** + manifest | History (see EIP-4444) | 13,351,985 | 0.0067 ETH | 0.0267 ETH | 0.1335 ETH | 0.0134 |
| 🥈 | **SSTORE2** (contract bytecode) | **State: forever** | 72,564,106 | 0.0363 ETH | 0.1451 ETH | 0.7256 ETH | 0.0726 |
| | Raw `SSTORE` slots (baseline) | State: forever | 238,140,357 | 0.1191 ETH | 0.4763 ETH | 2.3814 ETH | 0.2381 |
| | Blobs + `BlobAnchor` | ❌ data pruned after ~18 days; commitment permanent | 366,759 + 393,216 blob gas | 0.0006 ETH¹ | 0.0011 ETH¹ | 0.0041 ETH¹ | 0.0008 |
| | Hash commitment + off-chain URI | Only as permanent as the off-chain store | 188,382 | 0.0001 ETH | 0.0004 ETH | 0.0019 ETH | 0.0002 |

Numbers come from `cargo run -p cli -- bench --proof random:330000`. Random bytes are a worst case:
zstd ratio 100% and no zero bytes. ¹ Blob gas is priced at 1 gwei here. Real blob base fees are
often close to 1 wei, so the blob part is usually negligible.

**Measured with the real 336 KB recursive proof** (`proofs/proof.bin`, zstd -19 → 91.9%) and live
Etherscan prices on 2026-11-24 (mainnet 0.426 gwei/gas, Sepolia ~21 wei/gas):

| Strategy | Txs | Gas used | Mainnet ETH | Sepolia ETH (test) |
|---|---:|---:|---:|---:|
| sstore2 | 8 | 73,822,107 | 0.031427 | 0.0000000016 |
| calldata | 4 | 13,442,965 | 0.005723 | 0.0000000003 |
| events | 4 | 13,442,863 | 0.005723 | 0.0000000003 |
| slots | 22 | 242,396,419 | 0.103192 | 0.0000000051 |
| blob | 2 | 366,759 + 393,216 blob gas | 0.000549 | 0.000393 (blob gas at the 1 gwei default) |
| commitment | 1 | 188,382 | 0.000080 | ≈ 0 |

All six read back byte for byte (Roundtrip OK).

**Recommendation for e-voting:**

1. **If the proof must be permanent and readable by contracts** (for example, a verifier contract or
   an on-chain audit trail that reads it), use **SSTORE2**. It lives in state, so it survives
   history expiry, and contracts can read it with `EXTCODECOPY`. At today's ~0.5 gwei it costs
   about 0.036 ETH.
2. **If the cheapest durable on-L1 option is the goal,** use **calldata, with an Arweave mirror and
   the manifest** (keccak256 and length) in `ProofRegistry`. That is about 5.4× cheaper than
   SSTORE2. The manifest stays in state forever, so anyone holding a copy (from an archive node,
   the Portal Network, Arweave or IPFS) can prove it's the original.
3. **The better long-term fix:** wrap the STARK in a **Groth16 or PLONK SNARK** (as RISC Zero,
   SP1 and others do). The proof shrinks to about 0.3–1 KB, can be verified on chain for about
   250k gas, and costs almost nothing to store. Keep the STARK on Arweave as the auditable
   original.
4. **Don't rely on blobs** for anything that has to be permanent. They work as a cheap broadcast
   channel whose commitment (the versioned hash) is permanent, but the data itself disappears
   after about 18 days.

---

## Why the numbers look like this

| Constraint | Effect |
|---|---|
| **EIP-170**: max contract code 24,576 B | SSTORE2 needs 14 pointer contracts (24,575 data bytes each, plus a `0x00` STOP prefix). |
| **Code deposit**: 200 gas per byte | The main SSTORE2 cost: 330,000 × 200 ≈ 66M gas, plus calldata and `CREATE`. |
| **EIP-7825**: ≤ 16,777,216 gas per tx | 2 SSTORE2 pointers per tx (~11M gas), 512 slots per tx. Forces multi-tx uploads. |
| **txpool tx size** ~128 KiB | Calldata and event chunks are 120,000 B, so 3 txs. |
| **EIP-7623** calldata floor (10 gas per token; non-zero byte = 4 tokens) | Data-heavy txs pay at least ~40 gas per non-zero byte. **Event logs cost exactly the same as plain calldata**, because LOG (8 gas/byte) plus execution stays below the floor. |
| **SSTORE**: 22,100 gas per 32-byte word | Raw storage costs about 690 gas per byte, roughly 18× calldata. |
| **EIP-4844 blobs**: 131,072 B; data pruned after 4096 epochs | 3 blobs for 330 KB. The `BLOBHASH` versioned hash is recorded permanently in `BlobAnchor`. |

### Permanence: state vs history vs blobs

- **State** (SSTORE2, slots): every full node keeps it for as long as Ethereum exists. This is the
  only truly permanent storage on L1.
- **History** (calldata, logs): under **EIP-4444 / history expiry**, normal nodes are allowed to
  drop old block bodies and receipts, and clients have already started pruning old history. The
  data stays reconstructible from archive nodes, the Portal Network and public history archives,
  but it's no longer guaranteed by every node. For e-voting, pair it with a second copy
  (Arweave) and the on-chain hash.
- **Blobs**: consensus clients prune blob sidecars after about 18 days. Only the KZG commitment
  stays on chain. Anyone who archived the blob can prove it matches.

---

## What each strategy does

All strategies write a **write-once manifest** to `ProofRegistry` under `(electionId, kind)`. It
holds the length, chunk count, `keccak256(proof)`, the store contract and refs (SSTORE2 pointer
addresses or blob versioned hashes) plus an optional URI. Every `retrieve` checks the result
against this manifest.

| Strategy | Contract | Write | Read back |
|---|---|---|---|
| `sstore2` | `SSTORE2Store` | `write(id, firstIndex, bytes[])` deploys pointer contracts, 2 per tx | `eth_getCode(pointer)[1..]` for each ref |
| `calldata` | `CalldataStore` | `post(id, i, chunk)`: 120 KB per tx; emits `ChunkPosted(id, i, keccak, len)` | `eth_getLogs` finds the tx hashes, then `eth_getTransactionByHash` gives the input to ABI-decode |
| `events` | `EventLogStore` | `emitChunk(id, i, chunk)` emits `Chunk(id, i, bytes)` | `eth_getLogs` |
| `slots` | `SlotStore` | `write(id, startWord, chunk)`: 16 KB per tx | `read(id, startWord, len)` via `eth_call` |
| `blob` | `BlobAnchor` | type-3 tx with up to 6 blobs; `anchor(key, keccak)` records `blobhash(i)` | `anvil_getBlobByHash`. On a real network: a beacon node's `/eth/v1/beacon/blob_sidecars`, within ~18 days |
| `commitment` | `ProofRegistry` only | hash and `ar://`, `ipfs://`, `https://` or `file://` URI | fetch the URI and verify the keccak |

### Options not implemented here (and why)

- **SNARK wrapping** (Groth16/PLONK over the STARK) is the best fit for e-voting, as described
  above. It needs your proving stack's wrapper circuit, so it's out of scope for a storage
  benchmark.
- **Compression**: the bench prints the zstd ratio. STARK proofs (field elements and Merkle
  paths) usually compress by only a few percent. Random test data doesn't compress at all.
- **Arweave** (pay once, permanent) or **Filecoin/IPFS** (needs pinning or deals) combined with the
  `commitment` strategy. This is the cheapest way to get permanence while still anchoring trust
  on Ethereum.
- **L2s, or DA layers such as Celestia or EigenDA**: cheaper, but they move the trust and
  availability assumptions away from Ethereum L1.

---

## Gas prices: default or live (Etherscan)

Gas used is the same on every network; only the **gas price** changes. So the bench measures gas
once on Anvil and prices it for mainnet and Sepolia side by side:

```
ETH cost = gas used × gas price (gwei/gas) × 1e-9   (+ blob gas × blob gas price × 1e-9)
```

The gwei→ETH factor (1e-9) is a constant in code. The prices live in
[`config/gas_pricing.json`](config/gas_pricing.json):

```json
{
  "live_tier": "propose",
  "eth_usd": null,
  "networks": {
    "mainnet": { "chain_id": 1,        "default_gas_price_gwei": 10.0, "default_blob_gas_price_gwei": 1.0 },
    "sepolia": { "chain_id": 11155111, "default_gas_price_gwei": 1.0,  "default_blob_gas_price_gwei": 1.0 }
  }
}
```

- Before anything is stored, `bench` and `store` ask **"Use live gas price from Etherscan? [y/N]"**.
  - **y**: calls `https://api.etherscan.io/v2/api?module=gastracker&action=gasoracle&chainid=<id>&apikey=…`
    for each network and uses `SafeGasPrice`, `ProposeGasPrice` or `FastGasPrice` according to
    `live_tier`.
  - **n**: uses `default_gas_price_gwei`.
  - Price sources are tried in this order, and the first one that works wins:
    1. `module=gastracker&action=gasoracle` gives the safe, propose and fast tiers. **Mainnet only:**
       Etherscan answers Sepolia with `Missing Or invalid Action name`.
    2. `module=proxy&action=eth_gasPrice` on the same chain id. This is used for **Sepolia**.
    3. `eth_gasPrice` on `SEPOLIA_RPC_URL` (Sepolia, if set).
    4. `default_gas_price_gwei` from the config. A warning is printed only in this case.
- `--live-gas` / `--no-live-gas` skip the prompt. Non-interactive runs default to *no*.
- Etherscan's oracle has no blob base fee, so blob gas always uses `default_blob_gas_price_gwei`.
- Set `eth_usd` to a number to add a **Mainnet USD** column. Sepolia ETH has no market value.
- The table header shows where each price came from, for example
  `mainnet 0.426 gwei/gas (Etherscan propose, block 26485314)` or
  `sepolia 0.0000000210 gwei/gas (Etherscan eth_gasPrice)`. Very small values keep 3 significant
  digits instead of rounding to 0.
- If the proof file is missing, the tool exits right away, before asking about gas prices.

---

## Usage

Requirements: Rust (stable), [Foundry](https://getfoundry.sh) (`forge`, `anvil`).

```bash
# 1. contracts (Rust bindings are generated from contracts/out at compile time)
cd contracts
forge install foundry-rs/forge-std --no-git
forge build && forge test --gas-report
cd ..

# 2. config
cp .env.example .env              # then put your ETHERSCAN_API_KEY in .env

# 3. benchmark every strategy (spawns its own Anvil)
mkdir -p proofs && cp /path/to/your/proof.bin proofs/proof.bin   # proofs/ is gitignored
cargo run -p cli -- bench                          # uses $PROOF_PATH / proofs/proof.bin
cargo run -p cli -- bench --proof random:330000 --no-live-gas
cargo run -p cli -- bench --strategy sstore2,calldata --network mainnet --live-gas

# 4. store and retrieve against a node (RPC_URL + PRIVATE_KEY from .env)
anvil --hardfork prague --enable-tx-gas-limit &
cargo run -p cli -- deploy                         # writes deployments/<chain_id>.json
cargo run -p cli -- store    --strategy sstore2 --election "city-council-2026"
cargo run -p cli -- retrieve --strategy sstore2 --election "city-council-2026" --out proof.bin
cargo run -p cli -- store    --strategy commitment --election "city-council-2026" --uri ar://<txid>
```

Tests: `cargo test` runs the unit tests (cost math, Etherscan parsing using the real example
response, fallback to defaults, chunking, blob decoding) and an Anvil integration test that
stores and reads back 330 KB with **every** strategy. Tests never call the network.

## Layout

```
contracts/src/            ProofRegistry, SSTORE2Store, CalldataStore, EventLogStore, SlotStore, BlobAnchor
contracts/test/           Foundry tests
crates/proof-store/       library: StorageStrategy trait, chunk limits, strategies, cost model, Etherscan oracle
crates/proof-store/tests/ Anvil end-to-end round-trip
crates/cli/               `proof-store` binary (bench / deploy / store / retrieve) + table output
config/gas_pricing.json   default gas prices per network, live tier, ETH/USD
```

## Caveats (read before mainnet)

- **Registry access control**: `ProofRegistry` is write-once per `(electionId, kind)`, and the
  first writer wins. In production, restrict `register` to the election authority (or make the
  election id include the authority's address).
- **Log queries** use `from_block(0)`. On real RPCs, record the deployment or upload block and
  query only that range.
- **Mempool**: multi-tx uploads (SSTORE2 with 7 txs, slots with 21) aren't atomic. The manifest is
  registered last, so a half-finished upload simply has no manifest.
- Gas comes from Anvil with Prague rules. Mainnet after Osaka (Fusaka) should match closely; if
  in doubt, re-run against a mainnet fork (`anvil --fork-url …`).
- Calldata and event chunks are 120 KB to stay under geth's 128 KiB txpool limit. Some builders
  or relays accept less.

## License

MIT
