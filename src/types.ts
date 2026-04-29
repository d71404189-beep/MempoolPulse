export interface PendingTx {
  /** Source chain id (e.g. "ethereum", "arbitrum", "base", "bsc"). */
  chain: string;
  /** Native asset symbol of the source chain (e.g. "ETH", "BNB"). */
  native_symbol: string;
  hash: string;
  from: string;
  to: string | null;
  value_wei: string;
  /** Native-asset value (e.g. ETH or BNB). */
  value_native: number;
  value_usd: number | null;
  gas_gwei: number | null;
  gas_limit: string | null;
  input: string;
  selector: string | null;
  label: string | null;
  summary: string | null;
  seen_at: number;
}

export interface WatchEntry {
  address: string;
  label: string;
}

export interface Filters {
  /** Minimum native-asset value (kept name for back-compat with stored configs). */
  min_value_eth: number;
  min_value_usd: number;
  to_contracts: string[];
  selectors: string[];
  notify_all_matches: boolean;
  buffer_size: number;
}

export type ChainKind = "evm" | "bitcoin" | "solana" | "tron" | "ton" | "sui";

export interface ChainConfig {
  id: string;
  name: string;
  native_symbol: string;
  coingecko_id: string;
  rpc_ws_url: string;
  rpc_http_url: string;
  enabled: boolean;
  kind?: ChainKind;
}

export type AlertSound = "ping" | "chime" | "bell" | "siren";

export interface AlertRule {
  id: string;
  name: string;
  enabled: boolean;
  /** Chain ids to match (empty = any chain). */
  chains: string[];
  /** Minimum USD value of the tx (null = any). */
  min_value_usd: number | null;
  /** Case-insensitive substring of `tx.label` (null/empty = any). */
  label_contains: string | null;
  /** Only fire if `from` or `to` is in the watchlist. */
  watchlist_only: boolean;
  sound: AlertSound;
  cooldown_secs: number;
}

export interface AppSettings {
  /** Legacy single-chain URL — migrated by the backend on first load. */
  rpc_ws_url: string;
  /** Legacy single-chain URL — migrated by the backend on first load. */
  rpc_http_url: string;
  chains: ChainConfig[];
  license_key: string;
  display_name: string;
  language: "auto" | "en" | "ru";
  filters: Filters;
  watchlist: WatchEntry[];
  alert_rules: AlertRule[];
}

export interface ConnectionStatus {
  /** Chain id this status applies to (empty for legacy single-chain payloads). */
  chain?: string;
  connected: boolean;
  /** English fallback rendered by the Rust backend; used when `code` is absent. */
  message: string;
  /** Optional localization key (e.g. "status.streaming") emitted by the backend. */
  code?: string | null;
  /** Optional substitution params for the localized template. */
  params?: Record<string, string> | null;
}

export interface LicenseStatus {
  valid: boolean;
  message: string;
}
