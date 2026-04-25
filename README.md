# MempoolPulse

> Real-time Ethereum mempool monitor for spotting whale transactions before they confirm.

MempoolPulse is a desktop app (macOS, Windows, Linux) that streams pending
transactions directly from your own RPC provider, decodes the most common
DEX swaps and ERC20 calls, and surfaces the ones that matter — large
ETH transfers, watchlisted wallets, Uniswap V2/V3 swaps — with a clean
table and instant alerts.

It does **not** trade for you. It is a pure observation tool.

## Why use it

- **Bring your own RPC.** Plug in any Alchemy / QuickNode / Infura WebSocket
  URL. Your data never touches our servers because we don't run any.
- **Local-first.** Settings, watchlists, and (eventually) license keys are
  stored under your OS config dir. Nothing is uploaded.
- **Native and tiny.** Built with Tauri 2 + Rust, the binaries are < 10 MB
  and the app idles at < 50 MB of RAM.
- **Decodes the noise away.** Out of every 1,000 pending txs you only care
  about a handful — Uniswap routers, large ETH transfers, calls to your
  watchlisted contracts. MempoolPulse filters the firehose for you.
- **One-time payment.** No subscription.

## Quick start

1. Sign up for a free Alchemy / QuickNode / Infura account.
2. Copy your **WebSocket** RPC URL (e.g. `wss://eth-mainnet.g.alchemy.com/v2/<KEY>`).
3. Launch MempoolPulse, paste the URL into Settings, hit Save.
4. The Live feed should start populating within a few seconds.

For richer decoding when your provider only emits transaction hashes,
also paste the matching HTTPS URL.

## Selling model

This is a paid product distributed via Gumroad. The app boots into a license
gate on first run and verifies the entered key against the Gumroad licenses
API. See [`LANDING.md`](LANDING.md) for ready-to-use storefront copy.

In development builds (`cargo tauri dev`) the license check accepts any
non-empty key, so contributors don't need a real product permalink.

## Building locally

```bash
# One-time deps (Linux):
sudo apt-get install libwebkit2gtk-4.1-dev librsvg2-dev libsoup-3.0-dev \
                     libssl-dev pkg-config build-essential libayatana-appindicator3-dev

npm install
npm run tauri dev
```

Bundle release artifacts:

```bash
GUMROAD_PRODUCT_PERMALINK=mempoolpulse npm run tauri build
```

## Repo layout

```
src/                 React + TypeScript frontend
src-tauri/src/       Rust backend
  decoder.rs         Selector lookup + lightweight ABI decoding
  mempool.rs         WebSocket subscription worker
  prices.rs          ETH/USD price fetcher (CoinGecko, cached)
  license.rs         Gumroad license verification
  state.rs           Shared app state
  types.rs           Serializable types shared with the frontend
.github/workflows/   Cross-platform release builds
```

## Roadmap

- v1.0 — Ethereum mainnet (this release)
- v1.1 — Arbitrum, Base, BSC mempools
- v1.2 — Sound alerts on watchlist hits, custom alert rules
- v1.3 — Tx replay sandbox via local fork
- v1.4 — Solana support (subscription `accountChange` + program filters)

## License

Proprietary. See [LICENSE](LICENSE).
