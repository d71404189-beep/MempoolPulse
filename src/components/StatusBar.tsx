import type { ConnectionStatus } from "../types";

interface Props {
  status: ConnectionStatus;
  onRestart: () => void;
  onStop: () => void;
}

export default function StatusBar({ status, onRestart, onStop }: Props) {
  return (
    <div className="statusbar">
      <span className={status.connected ? "dot connected" : "dot"} />
      <span>{status.message}</span>
      <div className="actions">
        <button onClick={onRestart}>Reconnect</button>
        <button onClick={onStop}>Stop</button>
      </div>
    </div>
  );
}
