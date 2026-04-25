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

  // Visible chains = the ones the user has enabled. If nothing's enabled, show
  // a hint pointing to Settings instead of "0/0".
  const enabledIds = new Set(
    settings?.chains.filter((c) => c.enabled).map((c) => c.id) ?? [],
  );
  const visible = statuses.filter((s) => !s.chain || enabledIds.has(s.chain));

  const anyConnected = visible.some((s) => s.connected);
  const summary = renderSummary(visible, lang, settings, tr);
  const tooltip = renderTooltip(visible, lang, settings);

  return (
    <div className="statusbar">
      <span className={anyConnected ? "dot connected" : "dot"} />
      <span title={tooltip}>{summary}</span>
      <div className="actions">
        <button onClick={onRestart}>{tr("status.reconnect")}</button>
        <button onClick={onStop}>{tr("status.stop")}</button>
      </div>
    </div>
  );
}

function renderSummary(
  statuses: ConnectionStatus[],
  lang: Lang,
  settings: AppSettings | null,
  tr: (k: Parameters<typeof t>[1]) => string,
): string {
  if (statuses.length === 0) {
    return tr("status.no_chains");
  }
  if (statuses.length === 1) {
    const only = statuses[0];
    const name = chainName(only.chain, settings);
    const detail = formatStatus(lang, only);
    return name ? `${name}: ${detail}` : detail;
  }
  const connected = statuses.filter((s) => s.connected).length;
  if (connected === 0) {
    return tr("status.aggregate_none");
  }
  const tpl = t(lang, "status.aggregate");
  return tpl
    .replace("{connected}", String(connected))
    .replace("{total}", String(statuses.length));
}

function renderTooltip(
  statuses: ConnectionStatus[],
  lang: Lang,
  settings: AppSettings | null,
): string {
  return statuses
    .map((s) => {
      const name = chainName(s.chain, settings) ?? s.chain ?? "";
      return `${name}: ${formatStatus(lang, s)}`;
    })
    .join("\n");
}

function chainName(id: string | undefined, settings: AppSettings | null): string | undefined {
  if (!id) return undefined;
  return settings?.chains.find((c) => c.id === id)?.name;
}
