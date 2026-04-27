use serde::{Deserialize, Serialize};

/// A pending transaction observed in the mempool, enriched by our decoder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTx {
    /// Transaction hash (0x-prefixed hex).
    pub hash: String,
    /// Sender address (0x-prefixed hex, lowercase).
    pub from: String,
    /// Recipient address. May be null for contract creation.
    pub to: Option<String>,
    /// Native token value in wei, decimal string.
    pub value_wei: String,
    /// Native token value as a float for sorting/filtering (lossy, fine for UI).
    pub value_eth: f64,
    /// Approximate USD value of the native transfer based on the latest ETH price.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// WebSocket RPC URL provided by the user (e.g. wss://eth-mainnet.g.alchemy.com/v2/<KEY>).
    pub rpc_ws_url: String,
    /// HTTPS RPC URL used for follow-up calls like eth_getTransactionByHash.
    pub rpc_http_url: String,
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
            license_key: String::new(),
            display_name: String::new(),
            language: default_language(),
            filters: Filters::default(),
            watchlist: Vec::new(),
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
    pub connected: bool,
    pub message: String,
}
