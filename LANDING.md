# Gumroad / Lemon Squeezy storefront copy

Drop the headline + body straight into Gumroad's product editor.

---

## Headline

**See whales before they trade. MempoolPulse shows you Ethereum's pending
transactions in real-time — directly from your own RPC.**

## Subheadline

A native desktop app for traders, on-chain analysts, and DeFi power-users.
One-time payment. No subscription. No data leaves your machine.

## What you get

- Live stream of pending Ethereum transactions, decoded into something
  human-readable: Uniswap V2/V3 swaps, ERC20 transfers, approvals,
  Universal Router executions, Gnosis Safe executions, and more.
- Filter by minimum ETH value, USD value, contract address, or 4-byte
  selector.
- Watchlist of addresses — rows from those wallets are highlighted and
  always pass the filter, even if their value is below the threshold.
- Search across hash / address / decoded label.
- USD price column powered by CoinGecko (cached, no API key required).
- Local-only settings: your RPC URL, watchlist, and filters live on your
  machine. We don't operate any server.

## Why not a website?

Because every web tool that touches the mempool either:

1. Sends your RPC key to their backend, or
2. Aggregates everyone's view through a paid API and rate-limits you, or
3. Wraps a tiny feature in a $50-200/month subscription.

MempoolPulse is the opposite: native binary, one-time payment, your RPC,
your machine.

## Bring your own RPC

You'll need a free WebSocket RPC URL from any of these:

- [Alchemy](https://alchemy.com) — free tier supports
  `alchemy_pendingTransactions` (full tx bodies)
- [QuickNode](https://quicknode.com) — free tier supports
  `newPendingTransactions`
- [Infura](https://infura.io) — free tier supports
  `newPendingTransactions`

Setup takes about 60 seconds. Paste the URL into the Settings tab and
hit Save.

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
Ethereum mainnet ships in v1.0. Arbitrum, Base, and BSC follow in v1.1
(free update). Solana support is on the roadmap.

**Q: Why does my screen sometimes go quiet?**
Mempool throughput varies — a quiet block + your filters being aggressive
will produce gaps. Lower the "Min value (ETH)" or clear the contract
filter to verify the stream is alive.

**Q: I'm a market maker / trading firm, can I get a team license?**
Yes — email me at hello@mempoolpulse.app for site licenses.
