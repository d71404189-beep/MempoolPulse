import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import LicenseGate from "./components/LicenseGate";
import LiveTable from "./components/LiveTable";
import Settings from "./components/Settings";
import StatusBar from "./components/StatusBar";
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
  const [status, setStatus] = useState<ConnectionStatus>({
    connected: false,
    message: "Idle",
  });

  const lang = useMemo(() => resolveLang(settings?.language ?? "auto"), [settings?.language]);
  const tr = (key: Parameters<typeof t>[1]) => t(lang, key);

  // Bootstrap on mount.
  useEffect(() => {
    void (async () => {
      const license: LicenseStatus = await invoke("license_status");
      setLicensed(license.valid);
      const s: AppSettings = await invoke("get_settings");
      setSettings(s);
      const conn: ConnectionStatus = await invoke("connection_status");
      setStatus(conn);
      if (license.valid && s.rpc_ws_url) {
        await invoke("start_streaming");
      }
    })();
  }, []);

  // Stream pending transactions from the Rust worker.
  useEffect(() => {
    if (!settings) return;
    const cap = Math.max(50, settings.filters.buffer_size || 500);

    const unlistenPending = listen<PendingTx>("mempool://pending", (event) => {
      setTxs((current) => {
        const next = [event.payload, ...current];
        if (next.length > cap) next.length = cap;
        return next;
      });
    });

    const unlistenStatus = listen<ConnectionStatus>(
      "mempool://status",
      (event) => setStatus(event.payload),
    );

    return () => {
      void unlistenPending.then((fn) => fn());
      void unlistenStatus.then((fn) => fn());
    };
  }, [settings?.filters.buffer_size, settings]);

  const handleSettingsSaved = async (next: AppSettings) => {
    await invoke("save_settings", { newSettings: next });
    setSettings(next);
  };

  const handleLicensed = async (next: AppSettings) => {
    setLicensed(true);
    setSettings(next);
    if (next.rpc_ws_url) {
      await invoke("start_streaming");
    }
  };

  const headerStats = useMemo(() => {
    const total = txs.length;
    const totalUsd = txs.reduce(
      (acc, tx) => acc + (tx.value_usd ?? 0),
      0,
    );
    return { total, totalUsd };
  }, [txs]);

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
        status={status}
        onRestart={() => invoke("start_streaming")}
        onStop={() => invoke("stop_streaming")}
        lang={lang}
      />
    </div>
  );
}
