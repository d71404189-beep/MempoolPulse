import { useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { AppSettings, PendingTx } from "../types";
import { t, type Lang } from "../i18n";

/**
 * Map a chain id to its public block explorer base URL. Used to wire up the
 * clickable hash / from / to cells in the live feed.
 */
const EXPLORERS: Record<string, { tx: string; addr: string }> = {
  ethereum: { tx: "https://etherscan.io/tx/", addr: "https://etherscan.io/address/" },
  arbitrum: { tx: "https://arbiscan.io/tx/", addr: "https://arbiscan.io/address/" },
  base: { tx: "https://basescan.org/tx/", addr: "https://basescan.org/address/" },
  bsc: { tx: "https://bscscan.com/tx/", addr: "https://bscscan.com/address/" },
  polygon: { tx: "https://polygonscan.com/tx/", addr: "https://polygonscan.com/address/" },
  optimism: {
    tx: "https://optimistic.etherscan.io/tx/",
    addr: "https://optimistic.etherscan.io/address/",
  },
  avalanche: { tx: "https://snowtrace.io/tx/", addr: "https://snowtrace.io/address/" },
};

function whaleTier(usd: number | null | undefined): "" | "whale-s" | "whale-m" | "whale-l" {
  if (usd == null) return "";
  if (usd >= 1_000_000) return "whale-l";
  if (usd >= 100_000) return "whale-m";
  if (usd >= 10_000) return "whale-s";
  return "";
}

interface Props {
  txs: PendingTx[];
  settings: AppSettings | null;
  lang: Lang;
}

export default function LiveTable({ txs, settings, lang }: Props) {
  const [search, setSearch] = useState("");
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const tr = (k: Parameters<typeof t>[1]) => t(lang, k);

  const copy = async (key: string, value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedKey(key);
      window.setTimeout(() => setCopiedKey((c) => (c === key ? null : c)), 1200);
    } catch {
      // clipboard may be unavailable in some webviews; fail silently
    }
  };

  const openExplorer = (chain: string, kind: "tx" | "addr", value: string) => {
    const e = EXPLORERS[chain];
    if (!e) return;
    void openUrl(`${kind === "tx" ? e.tx : e.addr}${value}`);
  };

  const watchSet = useMemo(() => {
    const set = new Set<string>();
    settings?.watchlist.forEach((w) => set.add(w.address.toLowerCase()));
    return set;
  }, [settings?.watchlist]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return txs;
    return txs.filter((tx) => {
      return (
        tx.hash.toLowerCase().includes(q) ||
        tx.from.toLowerCase().includes(q) ||
        (tx.to ?? "").toLowerCase().includes(q) ||
        (tx.label ?? "").toLowerCase().includes(q)
      );
    });
  }, [txs, search]);

  if (txs.length === 0) {
    return (
      <div className="empty">
        <h3>{tr("live.empty.title")}</h3>
        <p>
          {tr("live.empty.body_prefix")}{" "}
          <code>newPendingTransactions</code> {tr("live.empty.body_or")}{" "}
          <code>alchemy_pendingTransactions</code>
          {tr("live.empty.body_suffix")}
        </p>
      </div>
    );
  }

  const exportCsv = () => downloadFile(buildCsv(filtered, settings), `mempoolpulse-${nowTag()}.csv`, "text/csv");
  const exportJson = () => downloadFile(buildJson(filtered, settings), `mempoolpulse-${nowTag()}.json`, "application/json");

  return (
    <div className="live">
      <div className="live-toolbar">
        <input
          placeholder={tr("live.search_placeholder")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <span className="muted">{filtered.length} {tr("live.matched")}</span>
        <div className="live-export">
          <button
            type="button"
            className="btn-secondary"
            onClick={exportCsv}
            disabled={filtered.length === 0}
            title={tr("live.export.tooltip")}
          >
            {tr("live.export.csv")}
          </button>
          <button
            type="button"
            className="btn-secondary"
            onClick={exportJson}
            disabled={filtered.length === 0}
            title={tr("live.export.tooltip")}
          >
            {tr("live.export.json")}
          </button>
        </div>
      </div>
      <div className="table-scroll">
        <table className="tx-table">
          <thead>
            <tr>
              <th>{tr("live.col.chain")}</th>
              <th>{tr("live.col.hash")}</th>
              <th>{tr("live.col.from")}</th>
              <th>{tr("live.col.to")}</th>
              <th>{tr("live.col.value")}</th>
              <th>{tr("live.col.usd")}</th>
              <th>{tr("live.col.gas")}</th>
              <th>{tr("live.col.method")}</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((tx) => {
              const watchHit =
                watchSet.has(tx.from) || (tx.to && watchSet.has(tx.to));
              return (
                <tr
                  key={`${tx.chain}:${tx.hash}`}
                  className={[watchHit ? "watch-hit" : "", whaleTier(tx.value_usd)].filter(Boolean).join(" ")}
                  title={tx.summary ?? undefined}
                >
                  <td><span className="chain-badge">{chainLabel(tx, settings)}</span></td>
                  <td className="mono">
                    <button
                      type="button"
                      className="link-cell"
                      title={`${tr("live.action.open_explorer")} \u2014 ${tx.hash}`}
                      onClick={() => openExplorer(tx.chain, "tx", tx.hash)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        void copy(`h:${tx.hash}`, tx.hash);
                      }}
                    >
                      {shorten(tx.hash)}
                      {copiedKey === `h:${tx.hash}` && (
                        <span className="copied-flash">{tr("live.action.copied")}</span>
                      )}
                    </button>
                  </td>
                  <td className="mono">
                    <button
                      type="button"
                      className="link-cell"
                      title={`${tr("live.action.open_explorer")} \u2014 ${tx.from}`}
                      onClick={() => openExplorer(tx.chain, "addr", tx.from)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        void copy(`f:${tx.hash}`, tx.from);
                      }}
                    >
                      {shorten(tx.from)}
                      {copiedKey === `f:${tx.hash}` && (
                        <span className="copied-flash">{tr("live.action.copied")}</span>
                      )}
                    </button>
                  </td>
                  <td className="mono">
                    {tx.to ? (
                      <button
                        type="button"
                        className="link-cell"
                        title={`${tr("live.action.open_explorer")} \u2014 ${tx.to}`}
                        onClick={() => openExplorer(tx.chain, "addr", tx.to ?? "")}
                        onContextMenu={(e) => {
                          e.preventDefault();
                          void copy(`t:${tx.hash}`, tx.to ?? "");
                        }}
                      >
                        {shorten(tx.to)}
                        {copiedKey === `t:${tx.hash}` && (
                          <span className="copied-flash">{tr("live.action.copied")}</span>
                        )}
                      </button>
                    ) : (
                      <span className="muted">{tr("live.create")}</span>
                    )}
                  </td>
                  <td className="value-eth">
                    {tx.value_native.toFixed(4)} <span className="muted">{tx.native_symbol || "ETH"}</span>
                  </td>
                  <td>{tx.value_usd != null ? `$${formatUsd(tx.value_usd)}` : <span className="muted">—</span>}</td>
                  <td>{tx.gas_gwei != null ? tx.gas_gwei.toFixed(1) : <span className="muted">—</span>}</td>
                  <td>
                    <span className={tx.label ? "" : "muted"}>
                      {tx.label ?? tr("live.raw_call")}
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function shorten(addr: string) {
  if (addr.length < 12) return addr;
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`;
}

function chainLabel(tx: PendingTx, settings: AppSettings | null): string {
  const known = settings?.chains.find((c) => c.id === tx.chain);
  if (known) return known.name;
  if (!tx.chain) return "—";
  return tx.chain;
}

function formatUsd(n: number) {
  return n.toLocaleString(undefined, { maximumFractionDigits: 0 });
}

function nowTag() {
  const d = new Date();
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
}

function csvCell(val: unknown): string {
  if (val === null || val === undefined) return "";
  const s = String(val);
  if (/[",\n\r]/.test(s)) {
    return `"${s.replace(/"/g, '""')}"`;
  }
  return s;
}

function buildCsv(rows: PendingTx[], settings: AppSettings | null): string {
  const header = [
    "chain",
    "chain_name",
    "hash",
    "from",
    "to",
    "value_native",
    "native_symbol",
    "value_usd",
    "gas_gwei",
    "label",
    "summary",
    "seen_at_iso",
  ];
  const lines = [header.join(",")];
  for (const tx of rows) {
    lines.push(
      [
        tx.chain,
        chainLabel(tx, settings),
        tx.hash,
        tx.from,
        tx.to ?? "",
        tx.value_native,
        tx.native_symbol,
        tx.value_usd ?? "",
        tx.gas_gwei ?? "",
        tx.label ?? "",
        tx.summary ?? "",
        tx.seen_at ? new Date(tx.seen_at).toISOString() : "",
      ]
        .map(csvCell)
        .join(","),
    );
  }
  return lines.join("\n");
}

function buildJson(rows: PendingTx[], settings: AppSettings | null): string {
  const enriched = rows.map((tx) => ({
    ...tx,
    chain_name: chainLabel(tx, settings),
  }));
  return JSON.stringify(
    {
      exported_at: new Date().toISOString(),
      count: rows.length,
      txs: enriched,
    },
    null,
    2,
  );
}

function downloadFile(content: string, filename: string, mime: string) {
  const blob = new Blob([content], { type: `${mime};charset=utf-8` });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  // Revoke after the click handler has had a chance to grab the URL.
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}
