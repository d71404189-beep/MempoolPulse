import type { AlertRule, AlertSound, ChainConfig } from "../types";
import { newRuleId, playSound, presetRules, SOUND_OPTIONS } from "../alerts";
import { t, type Lang } from "../i18n";

interface Props {
  rules: AlertRule[];
  chains: ChainConfig[];
  onChange: (next: AlertRule[]) => void;
  lang: Lang;
}

export default function AlertRulesEditor({ rules, chains, onChange, lang }: Props) {
  const tr = (k: Parameters<typeof t>[1]) => t(lang, k);

  const update = (i: number, patch: Partial<AlertRule>) =>
    onChange(rules.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));

  const remove = (i: number) => onChange(rules.filter((_, idx) => idx !== i));

  const addBlank = () =>
    onChange([
      ...rules,
      {
        id: newRuleId(),
        name: tr("alerts.new_rule_name"),
        enabled: true,
        chains: [],
        min_value_usd: null,
        label_contains: null,
        watchlist_only: false,
        sound: "chime",
        cooldown_secs: 15,
      },
    ]);

  const loadPresets = () => {
    const fresh = presetRules();
    // Append (don't replace) so users keep any custom rules they already have.
    const existingNames = new Set(rules.map((r) => r.name.toLowerCase()));
    const additions = fresh.filter((r) => !existingNames.has(r.name.toLowerCase()));
    onChange([...rules, ...additions]);
  };

  const toggleChain = (i: number, chainId: string) => {
    const r = rules[i];
    const next = r.chains.includes(chainId)
      ? r.chains.filter((c) => c !== chainId)
      : [...r.chains, chainId];
    update(i, { chains: next });
  };

  return (
    <div className="alert-rules">
      <div className="row" style={{ justifyContent: "space-between", marginBottom: 12 }}>
        <p className="lead" style={{ margin: 0, flex: 1 }}>{tr("alerts.lead")}</p>
        <div className="row" style={{ gap: 8 }}>
          <button type="button" className="btn secondary" onClick={loadPresets}>
            {tr("alerts.load_presets")}
          </button>
          <button type="button" className="btn primary" onClick={addBlank}>
            {tr("alerts.add_rule")}
          </button>
        </div>
      </div>
      {rules.length === 0 && (
        <div className="empty" style={{ padding: "24px 0", textAlign: "left" }}>
          <p className="muted">{tr("alerts.empty")}</p>
        </div>
      )}
      <div className="rules-list">
        {rules.map((r, i) => (
          <div key={r.id} className="rule-card">
            <div className="rule-head">
              <label className="chain-toggle">
                <input
                  type="checkbox"
                  checked={r.enabled}
                  onChange={(e) => update(i, { enabled: e.target.checked })}
                />
                <input
                  className="rule-name"
                  value={r.name}
                  onChange={(e) => update(i, { name: e.target.value })}
                  placeholder={tr("alerts.field.name")}
                  spellCheck={false}
                />
              </label>
              <div className="row" style={{ gap: 8 }}>
                <button
                  type="button"
                  className="btn-link"
                  onClick={() => playSound(r.sound)}
                  title={tr("alerts.test_sound")}
                >
                  ♪ {tr("alerts.test")}
                </button>
                <button
                  type="button"
                  className="btn-link"
                  onClick={() => remove(i)}
                  style={{ color: "var(--danger)" }}
                >
                  {tr("alerts.delete")}
                </button>
              </div>
            </div>

            <div className="rule-grid">
              <div className="field">
                <label>{tr("alerts.field.chains")}</label>
                <div className="chain-pills">
                  {chains.map((c) => {
                    const on = r.chains.includes(c.id);
                    return (
                      <button
                        type="button"
                        key={c.id}
                        className={on ? "pill on" : "pill"}
                        onClick={() => toggleChain(i, c.id)}
                      >
                        {c.name}
                      </button>
                    );
                  })}
                </div>
                {r.chains.length === 0 && (
                  <span className="muted hint">{tr("alerts.field.chains.any")}</span>
                )}
              </div>

              <div className="field">
                <label>{tr("alerts.field.min_usd")}</label>
                <input
                  type="number"
                  min={0}
                  step={1000}
                  placeholder={tr("alerts.field.min_usd.any")}
                  value={r.min_value_usd ?? ""}
                  onChange={(e) =>
                    update(i, {
                      min_value_usd: e.target.value === "" ? null : Number(e.target.value),
                    })
                  }
                />
              </div>

              <div className="field">
                <label>{tr("alerts.field.label_contains")}</label>
                <input
                  type="text"
                  placeholder={tr("alerts.field.label_contains.placeholder")}
                  value={r.label_contains ?? ""}
                  onChange={(e) =>
                    update(i, {
                      label_contains: e.target.value === "" ? null : e.target.value,
                    })
                  }
                  spellCheck={false}
                />
              </div>

              <div className="field">
                <label className="chain-toggle" style={{ gap: 8 }}>
                  <input
                    type="checkbox"
                    checked={r.watchlist_only}
                    onChange={(e) => update(i, { watchlist_only: e.target.checked })}
                  />
                  <span>{tr("alerts.field.watchlist_only")}</span>
                </label>
              </div>

              <div className="field">
                <label>{tr("alerts.field.sound")}</label>
                <select
                  value={r.sound}
                  onChange={(e) => update(i, { sound: e.target.value as AlertSound })}
                >
                  {SOUND_OPTIONS.map((s) => (
                    <option key={s} value={s}>
                      {tr(`alerts.sound.${s}` as Parameters<typeof t>[1])}
                    </option>
                  ))}
                </select>
              </div>

              <div className="field">
                <label>{tr("alerts.field.cooldown")}</label>
                <input
                  type="number"
                  min={0}
                  step={1}
                  value={r.cooldown_secs}
                  onChange={(e) =>
                    update(i, { cooldown_secs: Math.max(0, Number(e.target.value) || 0) })
                  }
                />
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
