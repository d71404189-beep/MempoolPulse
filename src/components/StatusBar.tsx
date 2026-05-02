import { useState } from "react";
import type { AppSettings, ConnectionStatus } from "../types";
import { formatStatus, t, type Lang } from "../i18n";

interface Props {
  statuses: ConnectionStatus[];
  settings: AppSettings | null;
  onRestart: () => void;
  onStop: () => void;
  lang: Lang;
}

export default function StatusBar({ statuses, settings, onRestart, onStop, lang }: Props) {
  const tr = (k: Parameters<typeof t>[1]) => t(lang, k);
  const [expanded, setExpanded] = useState(false);

  const enabledIds = new Set(
    settings?.chains.filter((c) => c.enabled).map((c) => c.id) ?? [],
  );
  const visible = statuses.filter((s) => !s.chain || enabledIds.has(s.chain));
  const connected = visible.filter((s) => s.connected).length;
  const total = visible.length;
  const anyConnected = connected > 0;

  return (
    <div className="statusbar-wrap">
      {expanded && (
        <div className="statusbar-panel">
          <div className="statusbar-panel-title">Chain Status</div>
          <div className="statusbar-chains">
            {visible.map((s) => {
              const name = chainName(s.chain, settings) ?? s.chain ?? "—";
              const detail = formatStatus(lang, s);
              return (
                <div key={s.chain ?? "_"} className={`statusbar-chain-row ${s.connected ? "chain-ok" : "chain-err"}`}>
                  <span className={`chain-dot ${s.connected ? "chain-dot-ok" : "chain-dot-err"}`} />
                  <span className="chain-row-name">{name}</span>
                  <span className="chain-row-status">{detail}</span>
                </div>
              );
            })}
            {visible.length === 0 && (
              <div className="chain-row-empty">No chains enabled</div>
            )}
          </div>
        </div>
      )}
      <div className="statusbar">
        <span className={anyConnected ? "dot connected" : "dot"} />
        <button
          className="statusbar-summary-btn"
          onClick={() => setExpanded((e) => !e)}
          title="Click to see chain details"
        >
          {total === 0
            ? tr("status.no_chains")
            : `Streaming on ${connected}/${total} chains ${expanded ? "▲" : "▼"}`}
        </button>
        <div className="actions">
          <button onClick={onRestart}>{tr("status.reconnect")}</button>
          <button onClick={onStop}>{tr("status.stop")}</button>
        </div>
      </div>
    </div>
  );
}

function chainName(id: string | undefined, settings: AppSettings | null): string | undefined {
  if (!id) return undefined;
  return settings?.chains.find((c) => c.id === id)?.name;
}
