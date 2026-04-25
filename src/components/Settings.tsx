import { useState } from "react";
import type { AppSettings, WatchEntry } from "../types";
import { t, type Lang } from "../i18n";

interface Props {
  settings: AppSettings;
  onSave: (next: AppSettings) => Promise<void>;
  lang: Lang;
}

export default function Settings({ settings, onSave, lang }: Props) {
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const tr = (k: Parameters<typeof t>[1]) => t(lang, k);

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
      <h2>{tr("settings.title")}</h2>
      <p className="lead">{tr("settings.lead")}</p>

      <div className="section">
        <h3>{tr("settings.section.lang")}</h3>
        <div className="field">
          <label>{tr("settings.lang.label")}</label>
          <select
            value={draft.language}
            onChange={(e) =>
              update("language", e.target.value as AppSettings["language"])
            }
          >
            <option value="auto">{tr("settings.lang.auto")}</option>
            <option value="en">{tr("settings.lang.en")}</option>
            <option value="ru">{tr("settings.lang.ru")}</option>
          </select>
        </div>
      </div>

      <div className="section">
        <h3>{tr("settings.section.rpc")}</h3>
        <div className="field">
          <label>{tr("settings.field.ws")}</label>
          <input
            placeholder="wss://eth-mainnet.g.alchemy.com/v2/YOUR_KEY"
            value={draft.rpc_ws_url}
            onChange={(e) => update("rpc_ws_url", e.target.value)}
            spellCheck={false}
          />
        </div>
        <div className="field">
          <label>{tr("settings.field.https")}</label>
          <input
            placeholder="https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY"
            value={draft.rpc_http_url}
            onChange={(e) => update("rpc_http_url", e.target.value)}
            spellCheck={false}
          />
        </div>
      </div>

      <div className="section">
        <h3>{tr("settings.section.filters")}</h3>
        <div className="row">
          <div className="field">
            <label>{tr("settings.field.min_eth")}</label>
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
            <label>{tr("settings.field.min_usd")}</label>
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
            <label>{tr("settings.field.buffer")}</label>
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
          <label>{tr("settings.field.contracts")}</label>
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
          <label>{tr("settings.field.selectors")}</label>
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
        <h3>{tr("settings.section.watchlist")}</h3>
        {draft.watchlist.map((w, i) => (
          <div key={i} className="watchlist-row">
            <input
              placeholder={tr("settings.watch.address")}
              value={w.address}
              onChange={(e) => updateWatch(i, { address: e.target.value })}
              spellCheck={false}
            />
            <input
              placeholder={tr("settings.watch.label")}
              value={w.label}
              onChange={(e) => updateWatch(i, { label: e.target.value })}
            />
            <button
              type="button"
              className="icon-btn"
              onClick={() => removeWatch(i)}
              aria-label={tr("settings.watch.remove")}
            >
              ×
            </button>
          </div>
        ))}
        <button type="button" className="btn secondary" onClick={addWatch}>
          {tr("settings.watch.add")}
        </button>
      </div>

      <div className="row" style={{ justifyContent: "flex-end" }}>
        {savedAt && (
          <span className="muted" style={{ alignSelf: "center" }}>
            {tr("settings.saved_at")} {new Date(savedAt).toLocaleTimeString()}
          </span>
        )}
        <button className="btn" disabled={saving} onClick={save}>
          {saving ? tr("settings.saving") : tr("settings.save")}
        </button>
      </div>
    </div>
  );
}
