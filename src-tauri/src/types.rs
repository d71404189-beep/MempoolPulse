use serde::{Deserialize, Serialize};

/// A pending transaction observed in the mempool, enriched by our decoder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTx {
    /// Source chain id (e.g. "ethereum", "arbitrum", "base", "bsc").
    #[serde(default)]
    pub chain: String,
    /// Native token symbol of the source chain (e.g. "ETH", "BNB").
    #[serde(default)]
    pub native_symbol: String,
    /// Transaction hash (0x-prefixed hex).
    pub hash: String,
    /// Sender address (0x-prefixed hex, lowercase).
    pub from: String,
    /// Recipient address. May be null for contract creation.
    pub to: Option<String>,
    /// Native token value in wei, decimal string.
    pub value_wei: String,
    /// Native token value as a float (in native units, e.g. ETH or BNB) for
    /// sorting/filtering. Lossy, fine for UI.
    pub value_native: f64,
    /// Approximate USD value of the native transfer based on the latest
    /// CoinGecko price for the chain's native asset.
    pub value_usd: Option<f64>,
    /// Gas price in gwei (legacy or maxFeePerGas), if present.
    pub gas_gwei: Option<f64>,
    /// Gas limit, decimal.
    pub gas_limit: Option<String>,
    /// Raw input data (0x-prefixed hex). Empty string for plain transfers.
    pub input: String,
    /// First 4 bytes of input as 0x-prefixed hex (function selector), if any.
    pub selector: Option<String>,
    /// Human-readable label for the function call (e.g. "Uniswap V2: swapExactETHForTokens").
    pub label: Option<String>,
    /// Decoded summary in plain text (e.g. "Swap 1.5 ETH for USDC").
    pub summary: Option<String>,
    /// Unix timestamp when the tx was first seen.
    pub seen_at: i64,
}

/// One EVM chain the user wants to monitor. The four built-in chains
/// (Ethereum, Arbitrum, Base, BNB Chain) share the same selector decoder
/// because Uniswap V2/V3 forks and ERC20 are deployed identically across
/// these networks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Stable identifier used as the map key (e.g. "ethereum").
    pub id: String,
    /// Display name shown in the UI ("Ethereum", "Arbitrum One", ...).
    pub name: String,
    /// Native asset symbol (e.g. "ETH", "BNB").
    pub native_symbol: String,
    /// CoinGecko coin id used to fetch the native asset's USD price.
    pub coingecko_id: String,
    /// WebSocket RPC URL (e.g. wss://arbitrum-one-rpc.publicnode.com).
    pub rpc_ws_url: String,
    /// HTTPS RPC URL used to enrich hash-only feeds.
    pub rpc_http_url: String,
    /// Whether the worker should subscribe to this chain on launch.
    pub enabled: bool,
}

impl ChainConfig {
    pub fn ethereum_default() -> Self {
        Self {
            id: "ethereum".into(),
            name: "Ethereum".into(),
            native_symbol: "ETH".into(),
            coingecko_id: "ethereum".into(),
            rpc_ws_url: "wss://ethereum-rpc.publicnode.com".into(),
            rpc_http_url: "https://ethereum-rpc.publicnode.com".into(),
            enabled: true,
        }
    }
    pub fn arbitrum_default() -> Self {
        Self {
            id: "arbitrum".into(),
            name: "Arbitrum One".into(),
            native_symbol: "ETH".into(),
            coingecko_id: "ethereum".into(),
            rpc_ws_url: "wss://arbitrum-one-rpc.publicnode.com".into(),
            rpc_http_url: "https://arbitrum-one-rpc.publicnode.com".into(),
            enabled: false,
        }
    }
    pub fn base_default() -> Self {
        Self {
            id: "base".into(),
            name: "Base".into(),
            native_symbol: "ETH".into(),
            coingecko_id: "ethereum".into(),
            rpc_ws_url: "wss://base-rpc.publicnode.com".into(),
            rpc_http_url: "https://base-rpc.publicnode.com".into(),
            enabled: false,
        }
    }
    pub fn bsc_default() -> Self {
        Self {
            id: "bsc".into(),
            name: "BNB Chain".into(),
            native_symbol: "BNB".into(),
            coingecko_id: "binancecoin".into(),
            rpc_ws_url: "wss://bsc-rpc.publicnode.com".into(),
            rpc_http_url: "https://bsc-rpc.publicnode.com".into(),
            enabled: false,
        }
    }

    pub fn defaults() -> Vec<Self> {
        vec![
            Self::ethereum_default(),
            Self::arbitrum_default(),
            Self::base_default(),
            Self::bsc_default(),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Legacy single-chain WebSocket URL. Migrated into `chains[0]` on first
    /// load if non-empty; kept for back-compat reading old settings.json files.
    #[serde(default)]
    pub rpc_ws_url: String,
    /// Legacy single-chain HTTPS URL (same migration story as `rpc_ws_url`).
    #[serde(default)]
    pub rpc_http_url: String,
    /// Per-chain configuration. New format; supersedes the two `rpc_*_url`
    /// fields. If empty when loaded, the four built-in defaults are inserted.
    #[serde(default)]
    pub chains: Vec<ChainConfig>,
    /// Saved license key from Gumroad. Empty if not yet activated.
    pub license_key: String,
    /// Optional per-user nickname displayed in notifications.
    pub display_name: String,
    /// UI language preference: "auto" (default, follows system locale), "en", or "ru".
    #[serde(default = "default_language")]
    pub language: String,
    /// Filters applied on the live feed.
    pub filters: Filters,
    /// List of watched addresses; matches highlight rows and trigger system notifications.
    pub watchlist: Vec<WatchEntry>,
}

fn default_language() -> String {
    "auto".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            rpc_ws_url: String::new(),
            rpc_http_url: String::new(),
            chains: ChainConfig::defaults(),
            license_key: String::new(),
            display_name: String::new(),
            language: default_language(),
            filters: Filters::default(),
            watchlist: Vec::new(),
        }
    }
}

impl AppSettings {
    /// Migrates old single-chain settings into the new `chains` list and
    /// fills in any missing built-in chain rows so the UI always has the
    /// four presets visible.
    pub fn normalize(&mut self) {
        // Migrate legacy single-chain URL into chains[0] = Ethereum.
        if self.chains.is_empty() && !self.rpc_ws_url.is_empty() {
            let mut eth = ChainConfig::ethereum_default();
            eth.rpc_ws_url = std::mem::take(&mut self.rpc_ws_url);
            eth.rpc_http_url = std::mem::take(&mut self.rpc_http_url);
            eth.enabled = true;
            self.chains = vec![
                eth,
                ChainConfig::arbitrum_default(),
                ChainConfig::base_default(),
                ChainConfig::bsc_default(),
            ];
        }
        if self.chains.is_empty() {
            self.chains = ChainConfig::defaults();
        }
        // Make sure each built-in chain id has at least one row so users see
        // them in Settings even after upgrading from a partial config.
        for preset in ChainConfig::defaults() {
            if !self.chains.iter().any(|c| c.id == preset.id) {
                self.chains.push(preset);
            }
        }
        // Backfill empty URL fields with the publicnode defaults so users who
        // never edit Settings get a working configuration out of the box.
        for chain in self.chains.iter_mut() {
            let default = match chain.id.as_str() {
                "ethereum" => Some(ChainConfig::ethereum_default()),
                "arbitrum" => Some(ChainConfig::arbitrum_default()),
                "base" => Some(ChainConfig::base_default()),
                "bsc" => Some(ChainConfig::bsc_default()),
                _ => None,
            };
            if let Some(d) = default {
                if chain.rpc_ws_url.trim().is_empty() {
                    chain.rpc_ws_url = d.rpc_ws_url.clone();
                }
                if chain.rpc_http_url.trim().is_empty() {
                    chain.rpc_http_url = d.rpc_http_url;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Filters {
    /// Minimum native value in ETH to show.
    pub min_value_eth: f64,
    /// Minimum estimated USD value to show.
    pub min_value_usd: f64,
    /// Only show calls to one of these "to" contract addresses (lowercase).
    pub to_contracts: Vec<String>,
    /// Only show these function selectors (0x-prefixed, 4 bytes).
    pub selectors: Vec<String>,
    /// Notify on ALL matched rows, not just watchlist.
    pub notify_all_matches: bool,
    /// Maximum number of rows kept in memory for the UI.
    pub buffer_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatchEntry {
    pub address: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseStatus {
    pub valid: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    /// Chain id this status applies to (e.g. "ethereum"). Empty in legacy
    /// single-chain payloads; always set for events emitted by the v1.1+
    /// per-chain workers.
    #[serde(default)]
    pub chain: String,
    pub connected: bool,
    /// English fallback message. Always populated for back-compat with the
    /// original API; the frontend prefers `code`+`params` when present.
    pub message: String,
    /// Localization key (e.g. "status.streaming"). When set, the frontend
    /// renders the localized template and substitutes `params`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Substitution params for the localized template (e.g. {"url": "wss://…"}).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<std::collections::HashMap<String, String>>,
}
