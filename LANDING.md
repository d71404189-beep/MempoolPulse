# Gumroad / Lemon Squeezy storefront copy

Drop the headline + body straight into Gumroad's product editor.

---

## Headline

**See whales before they trade. MempoolPulse shows you pending transactions
on Ethereum, Arbitrum, Base, and BNB Chain in real-time — directly from
your own RPCs.**

## Subheadline

A native desktop app for traders, on-chain analysts, and DeFi power-users.
One-time payment. No subscription. No data leaves your machine.

## What you get

- Live stream of pending transactions on **Ethereum, Arbitrum One, Base,
  and BNB Chain** — toggle each chain independently, all in one window.
- Decoded into something human-readable: Uniswap V2/V3 swaps, ERC20
  transfers, approvals, Universal Router executions, Gnosis Safe
  executions, and more.
- Filter by minimum native value, USD value, contract address, or 4-byte
  selector — applies across all enabled chains.
- Watchlist of addresses — rows from those wallets are highlighted and
  always pass the filter, even if their value is below the threshold.
- Search across hash / address / decoded label.
- USD price column powered by CoinGecko (cached, no API key required) —
  per-chain pricing (ETH for L2s, BNB for BNB Chain).
- Russian and English UI with auto-detection from your system locale.
- Local-only settings: your RPC URLs, watchlist, and filters live on
  your machine. We don't operate any server.

## Why not a website?

Because every web tool that touches the mempool either:

1. Sends your RPC key to their backend, or
2. Aggregates everyone's view through a paid API and rate-limits you, or
3. Wraps a tiny feature in a $50-200/month subscription.

MempoolPulse is the opposite: native binary, one-time payment, your RPC,
your machine.

## Bring your own RPC

You'll need a free WebSocket RPC URL per chain you want to monitor. Either:

- Use the bundled defaults (publicnode.com — no signup, free, decent for
  Ethereum/BSC/Base; Arbitrum sequencer rarely emits pendings on public
  nodes regardless of provider).
- Or paste your own from [Alchemy](https://alchemy.com), [QuickNode](https://quicknode.com),
  or [Infura](https://infura.io) for higher throughput. Alchemy's free
  tier supports `alchemy_pendingTransactions` (full tx bodies); the others
  use `newPendingTransactions` (hashes, fetched on demand).

Setup takes about 60 seconds per chain. Paste the URL into Settings →
Chains and hit Save.

## Specs

- macOS 11+ (Apple Silicon and Intel), Windows 10+, Linux x86_64
- ~10 MB installer, ~50 MB RAM at idle
- Built with Rust + Tauri for native performance and a tiny footprint
- Auto-reconnects with exponential backoff if the RPC drops

## What MempoolPulse is **not**

- It does **not** trade for you. There is no auto-buy, no auto-sell, no
  signing. It is an observation tool.
- It does **not** access your private keys, seed phrases, or wallets.
- It does **not** subscribe you to anything. One payment, lifetime use of
  the version you bought, free updates within the major version.

## License

A single license key activates the app on up to 3 devices. Need more?
Email me and I'll bump your seat count.

---

## FAQ for the storefront

**Q: Will it work on testnets?**
Yes — point the WebSocket URL at any chain your provider exposes (Sepolia,
Holesky, etc.).

**Q: Does it support L2s?**
v1.0 ships with **Ethereum, Arbitrum One, Base, and BNB Chain** out of
the box. Solana is planned for a future update (free for license
holders).

**Q: Why does my screen sometimes go quiet?**
Mempool throughput varies — a quiet block + your filters being aggressive
will produce gaps. Lower the "Min value (native)" or clear the contract
filter to verify the stream is alive.

**Q: I'm a market maker / trading firm, can I get a team license?**
Yes — email me at hello@mempoolpulse.app for site licenses.
