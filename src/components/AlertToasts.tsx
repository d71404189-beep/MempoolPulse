import type { PendingTx } from "../types";

export interface ToastEntry {
  id: string;
  ruleId: string;
  text: string;
  tx: PendingTx;
}

interface Props {
  toasts: ToastEntry[];
  onDismiss: (id: string) => void;
}

export default function AlertToasts({ toasts, onDismiss }: Props) {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-stack">
      {toasts.map((t) => (
        <button
          key={t.id}
          type="button"
          className="toast"
          onClick={() => onDismiss(t.id)}
          title="Click to dismiss"
        >
          <span className="toast-dot" />
          <span className="toast-text">{t.text}</span>
          <span className="toast-hash mono">
            {t.tx.hash.slice(0, 6)}…{t.tx.hash.slice(-4)}
          </span>
        </button>
      ))}
    </div>
  );
}
