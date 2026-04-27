import type { ConnectionStatus } from "../types";
import { t, type Lang } from "../i18n";

interface Props {
  status: ConnectionStatus;
  onRestart: () => void;
  onStop: () => void;
  lang: Lang;
}

export default function StatusBar({ status, onRestart, onStop, lang }: Props) {
  const tr = (k: Parameters<typeof t>[1]) => t(lang, k);
  return (
    <div className="statusbar">
      <span className={status.connected ? "dot connected" : "dot"} />
      <span>{status.message}</span>
      <div className="actions">
        <button onClick={onRestart}>{tr("status.reconnect")}</button>
        <button onClick={onStop}>{tr("status.stop")}</button>
      </div>
    </div>
  );
}
