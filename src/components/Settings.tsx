import { useState } from "react";
import type { AppSettings, WatchEntry } from "../types";

interface Props {
  settings: AppSettings;
  onSave: (next: AppSettings) => Promise<void>;
}

export default function Settings({ settings, onSave }: Props) {
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);

  const update = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const updateFilter = <K extends keyof AppSettings["filters"]>(
    key: K,
    value: AppSettings["filters"][K],
  ) => setDraft((d) => ({ ...d, filters: { ...d.filters, [key]: value } }));

  const addWatch = () =>
    setDraft((d) => ({
      ...d,
      watchlist: [...d.watchlist, { address: "", label: "" }],
    }));

  const updateWatch = (i: number, patch: Partial<WatchEntry>) =>
    setDraft((d) => ({
      ...d,
      watchlist: d.watchlist.map((w, idx) =>
        idx === i ? { ...w, ...patch } : w,
      ),
    }));

  const removeWatch = (i: number) =>
    setDraft((d) => ({
      ...d,
      watchlist: d.watchlist.filter((_, idx) => idx !== i),
    }));

  const save = async () => {
    setSaving(true);
    try {
      await onSave(draft);
      setSavedAt(Date.now());
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="settings">
      <h2>Settings</h2>
      <p className="lead">
        Bring your own RPC. MempoolPulse never proxies your traffic — your keys
        and pending-tx data stay on this machine.
      </p>

      <div className="section">
        <h3>RPC endpoints</h3>
        <div className="field">
          <label>WebSocket URL</label>
          <input
            placeholder="wss://eth-mainnet.g.alchemy.com/v2/YOUR_KEY"
            value={draft.rpc_ws_url}
            onChange={(e) => update("rpc_ws_url", e.target.value)}
            spellCheck={false}
          />
        </div>
        <div className="field">
          <label>HTTPS URL (optional, used to enrich hash-only feeds)</label>
          <input
            placeholder="https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY"
            value={draft.rpc_http_url}
            onChange={(e) => update("rpc_http_url", e.target.value)}
            spellCheck={false}
          />
        </div>
      </div>

      <div className="section">
        <h3>Filters</h3>
        <div className="row">
          <div className="field">
            <label>Min value (ETH)</label>
            <input
              type="number"
              min={0}
              step={0.1}
              value={draft.filters.min_value_eth}
              onChange={(e) =>
                updateFilter("min_value_eth", Number(e.target.value) || 0)
              }
            />
          </div>
          <div className="field">
            <label>Min value (USD)</label>
            <input
              type="number"
              min={0}
              step={100}
              value={draft.filters.min_value_usd}
              onChange={(e) =>
                updateFilter("min_value_usd", Number(e.target.value) || 0)
              }
            />
          </div>
          <div className="field">
            <label>Buffer size</label>
            <input
              type="number"
              min={50}
              max={5000}
              step={50}
              value={draft.filters.buffer_size}
              onChange={(e) =>
                updateFilter("buffer_size", Number(e.target.value) || 500)
              }
            />
          </div>
        </div>
        <div className="field">
          <label>Filter to contracts (one address per line)</label>
          <textarea
            rows={3}
            value={draft.filters.to_contracts.join("\n")}
            onChange={(e) =>
              updateFilter(
                "to_contracts",
                e.target.value
                  .split("\n")
                  .map((s) => s.trim())
                  .filter(Boolean),
              )
            }
          />
        </div>
        <div className="field">
          <label>Function selectors (4-byte hex, one per line)</label>
          <textarea
            rows={3}
            value={draft.filters.selectors.join("\n")}
            onChange={(e) =>
              updateFilter(
                "selectors",
                e.target.value
                  .split("\n")
                  .map((s) => s.trim())
                  .filter(Boolean),
              )
            }
          />
        </div>
      </div>

      <div className="section">
        <h3>Watchlist</h3>
        {draft.watchlist.map((w, i) => (
          <div key={i} className="watchlist-row">
            <input
              placeholder="0xWalletAddress"
              value={w.address}
              onChange={(e) => updateWatch(i, { address: e.target.value })}
              spellCheck={false}
            />
            <input
              placeholder="Label (e.g. Whale #1)"
              value={w.label}
              onChange={(e) => updateWatch(i, { label: e.target.value })}
            />
            <button
              type="button"
              className="icon-btn"
              onClick={() => removeWatch(i)}
              aria-label="Remove"
            >
              ×
            </button>
          </div>
        ))}
        <button type="button" className="btn secondary" onClick={addWatch}>
          + Add address
        </button>
      </div>

      <div className="row" style={{ justifyContent: "flex-end" }}>
        {savedAt && (
          <span className="muted" style={{ alignSelf: "center" }}>
            Saved {new Date(savedAt).toLocaleTimeString()}
          </span>
        )}
        <button className="btn" disabled={saving} onClick={save}>
          {saving ? "Saving…" : "Save"}
        </button>
      </div>
    </div>
  );
}
