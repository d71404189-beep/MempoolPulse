import { useMemo, useState } from "react";
import type { AppSettings, PendingTx } from "../types";
import { t, type Lang } from "../i18n";

interface Props {
  txs: PendingTx[];
  settings: AppSettings | null;
  lang: Lang;
}

export default function LiveTable({ txs, settings, lang }: Props) {
  const [search, setSearch] = useState("");
  const tr = (k: Parameters<typeof t>[1]) => t(lang, k);

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

  return (
    <div className="live">
      <div className="live-toolbar">
        <input
          placeholder={tr("live.search_placeholder")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <span className="muted">{filtered.length} {tr("live.matched")}</span>
      </div>
      <div className="table-scroll">
        <table className="tx-table">
          <thead>
            <tr>
              <th>{tr("live.col.hash")}</th>
              <th>{tr("live.col.from")}</th>
              <th>{tr("live.col.to")}</th>
              <th>{tr("live.col.value_eth")}</th>
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
                  key={tx.hash}
                  className={watchHit ? "watch-hit" : ""}
                  title={tx.summary ?? undefined}
                >
                  <td className="mono">{shorten(tx.hash)}</td>
                  <td className="mono">{shorten(tx.from)}</td>
                  <td className="mono">{tx.to ? shorten(tx.to) : <span className="muted">{tr("live.create")}</span>}</td>
                  <td className="value-eth">{tx.value_eth.toFixed(4)}</td>
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

function formatUsd(n: number) {
  return n.toLocaleString(undefined, { maximumFractionDigits: 0 });
}
