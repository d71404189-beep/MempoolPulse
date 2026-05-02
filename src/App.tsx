import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import LicenseGate from "./components/LicenseGate";
import LiveTable from "./components/LiveTable";
import Settings from "./components/Settings";
import StatusBar from "./components/StatusBar";
import AlertToasts, { type ToastEntry } from "./components/AlertToasts";
import { describeMatch, matchRule, playSound } from "./alerts";
import type {
  AppSettings,
  ConnectionStatus,
  LicenseStatus,
  PendingTx,
} from "./types";
import { resolveLang, t } from "./i18n";
import "./App.css";

type Tab = "live" | "settings";

export default function App() {
  const [licensed, setLicensed] = useState<boolean | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [tab, setTab] = useState<Tab>("live");
  const [txs, setTxs] = useState<PendingTx[]>([]);
  // Per-chain status keyed by chain id. Replaces the v1.0 single ConnectionStatus.
  const [statusMap, setStatusMap] = useState<Record<string, ConnectionStatus>>({});
  const [toasts, setToasts] = useState<ToastEntry[]>([]);
  // Latest settings reference so the listener closure always reads fresh
  // alert rules / watchlist without rebinding the listener on every change.
  const settingsRef = useRef<AppSettings | null>(null);
  // Last-fire timestamp per rule id, for cooldown enforcement.
  const ruleFiresRef = useRef<Map<string, number>>(new Map());
  const dismissToast = (id: string) =>
    setToasts((cur) => cur.filter((t) => t.id !== id));

  const lang = useMemo(() => resolveLang(settings?.language ?? "auto"), [settings?.language]);
  const tr = (key: Parameters<typeof t>[1]) => t(lang, key);

  // Keep the ref in sync with state so the pending-tx listener always reads
  // the freshest rules (and we don't have to re-bind the listener on every
  // settings change, which would drop in-flight events).
  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  // Bootstrap on mount.
  useEffect(() => {
    void (async () => {
      const license: LicenseStatus = await invoke("license_status");
      setLicensed(license.valid);
      const s: AppSettings = await invoke("get_settings");
      setSettings(s);
      const conns: ConnectionStatus[] = await invoke("connection_status");
      setStatusMap(toMap(conns));
      const hasEnabled = s.chains.some(hasUsableUrl);
      if (license.valid && hasEnabled) {
        await invoke("start_streaming");
      }
    })();
  }, []);

  // Stream pending transactions from the Rust workers.
  useEffect(() => {
    if (!settings) return;
    const cap = Math.max(50, settings.filters.buffer_size || 500);
    // Per-chain cap: each chain gets at most this fraction of the total buffer.
    // This prevents high-volume chains (Solana ~2-3k tx/s) from crowding out
    // lower-volume ones (Ethereum, TON, etc.).
    const perChainCap = Math.max(20, Math.floor(cap / 4));

    const unlistenPending = listen<PendingTx>("mempool://pending", (event) => {
      const tx = event.payload;
      // Apply min_value filters before adding to list.
      // For non-EVM chains (Solana etc.) value_usd may be null — if a USD
      // threshold is set we skip txs where the value is unknown or below it.
      const sf = settingsRef.current;
      if (sf) {
        const minUsd = sf.filters.min_value_usd ?? 0;
        const minNative = sf.filters.min_value_eth ?? 0;
        if (minUsd > 0 && (tx.value_usd == null || tx.value_usd < minUsd)) return;
        if (minNative > 0 && tx.value_native < minNative) return;
      }
      setTxs((current) => {
        // Count how many txs this chain already has in the buffer
        const chainCount = current.filter((t) => t.chain === tx.chain).length;
        if (chainCount >= perChainCap) {
          // Remove the oldest tx from this chain to make room
          const oldestIdx = current.map((t, i) => ({ chain: t.chain, i }))
            .reverse()
            .find((x) => x.chain === tx.chain)?.i;
          const next = [tx, ...current.filter((_, i) => i !== oldestIdx)];
          if (next.length > cap) next.length = cap;
          return next;
        }
        const next = [tx, ...current];
        if (next.length > cap) next.length = cap;
        return next;
      });
      // Evaluate against current alert rules. Reads from the ref so this
      // listener doesn't need to be rebuilt every time settings change.
      const s = settingsRef.current;
      if (!s || !s.alert_rules.length) return;
      const rule = matchRule(tx, s);
      if (!rule) return;
      const fires = ruleFiresRef.current;
      const last = fires.get(rule.id) ?? 0;
      const now = Date.now();
      if (now - last < rule.cooldown_secs * 1000) return;
      fires.set(rule.id, now);
      playSound(rule.sound);
      const id = `${rule.id}:${tx.hash}:${now}`;
      setToasts((cur) => [
        { id, ruleId: rule.id, text: describeMatch(tx, rule), tx },
        ...cur,
      ].slice(0, 4));
      window.setTimeout(() => dismissToast(id), 6000);
    });

    const unlistenStatus = listen<ConnectionStatus>(
      "mempool://status",
      (event) => {
        const s = event.payload;
        setStatusMap((prev) => ({ ...prev, [s.chain || "_"]: s }));
      },
    );

    return () => {
      void unlistenPending.then((fn) => fn());
      void unlistenStatus.then((fn) => fn());
    };
  }, [settings?.filters.buffer_size, settings]);

  const handleSettingsSaved = async (next: AppSettings) => {
    await invoke("save_settings", { newSettings: next });
    setSettings(next);
    // Refresh the status snapshot — the Rust backend reset chains it disabled
    // to "idle" and restarted enabled chains, so the in-memory map can be stale.
    const conns: ConnectionStatus[] = await invoke("connection_status");
    setStatusMap(toMap(conns));
  };

  const handleLicensed = async (next: AppSettings) => {
    setLicensed(true);
    setSettings(next);
    if (next.chains.some(hasUsableUrl)) {
      await invoke("start_streaming");
    }
  };

  // A chain auto-starts streaming if it's enabled and has whichever URL its
  // worker actually uses: WebSocket for EVM/Bitcoin/Solana/Sui, HTTP for
  // TRON/TON. Without this distinction, TRON/TON-only configs (which leave
  // rpc_ws_url empty by design) would never auto-start.
  function hasUsableUrl(c: AppSettings["chains"][number]): boolean {
    if (!c.enabled) return false;
    const kind = c.kind ?? "evm";
    if (kind === "tron" || kind === "ton") return c.rpc_http_url.length > 0;
    return c.rpc_ws_url.length > 0;
  }

  const headerStats = useMemo(() => {
    const total = txs.length;
    const totalUsd = txs.reduce(
      (acc, tx) => acc + (tx.value_usd ?? 0),
      0,
    );
    return { total, totalUsd };
  }, [txs]);

  const statuses = useMemo(() => Object.values(statusMap), [statusMap]);

  if (licensed === null) {
    return (
      <div className="loading">
        <h1>MempoolPulse</h1>
        <p>{tr("app.loading")}</p>
      </div>
    );
  }

  if (!licensed) {
    return <LicenseGate onActivated={handleLicensed} lang={lang} />;
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand-dot" />
          <span className="brand-name">MempoolPulse</span>
        </div>
        <nav className="tabs">
          <button
            className={tab === "live" ? "tab active" : "tab"}
            onClick={() => setTab("live")}
          >
            {tr("app.tab.live")}
          </button>
          <button
            className={tab === "settings" ? "tab active" : "tab"}
            onClick={() => setTab("settings")}
          >
            {tr("app.tab.settings")}
          </button>
        </nav>
        <div className="stats">
          <span>{headerStats.total} {tr("app.stats.txs")}</span>
          <span>
            ≈ ${headerStats.totalUsd.toLocaleString(undefined, { maximumFractionDigits: 0 })}
          </span>
        </div>
      </header>

      <main className="main">
        {tab === "live" && <LiveTable txs={txs} settings={settings} lang={lang} />}
        {tab === "settings" && settings && (
          <Settings settings={settings} onSave={handleSettingsSaved} lang={lang} />
        )}
      </main>

      <StatusBar
        statuses={statuses}
        settings={settings}
        onRestart={() => invoke("start_streaming")}
        onStop={() => invoke("stop_streaming")}
        lang={lang}
      />

      <AlertToasts toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}

function toMap(conns: ConnectionStatus[]): Record<string, ConnectionStatus> {
  const map: Record<string, ConnectionStatus> = {};
  for (const c of conns) {
    map[c.chain || "_"] = c;
  }
  return map;
}
