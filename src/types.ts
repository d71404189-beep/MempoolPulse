export interface PendingTx {
  hash: string;
  from: string;
  to: string | null;
  value_wei: string;
  value_eth: number;
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
  min_value_eth: number;
  min_value_usd: number;
  to_contracts: string[];
  selectors: string[];
  notify_all_matches: boolean;
  buffer_size: number;
}

export interface AppSettings {
  rpc_ws_url: string;
  rpc_http_url: string;
  license_key: string;
  display_name: string;
  language: "auto" | "en" | "ru";
  filters: Filters;
  watchlist: WatchEntry[];
}

export interface ConnectionStatus {
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
