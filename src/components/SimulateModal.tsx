import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PendingTx } from "../types";
import { t, type Lang } from "../i18n";

interface DecodedTransfer {
  token: string;
  from: string;
  to: string;
  value_raw: string;
}

interface SimulationResult {
  success: boolean;
  gas_used: number;
  effective_gas_price_gwei: number;
  revert_reason: string | null;
  transfers: DecodedTransfer[];
  log_count: number;
  block_number: number;
}

type Phase =
  | { kind: "idle" }
  | { kind: "downloading"; received: number; total: number | null }
  | { kind: "extracting" }
  | { kind: "running" }
  | { kind: "done"; result: SimulationResult }
  | { kind: "error"; message: string };

interface Props {
  tx: PendingTx;
  lang: Lang;
  onClose: () => void;
}

export default function SimulateModal({ tx, lang, onClose }: Props) {
  const tr = (k: Parameters<typeof t>[1]) => t(lang, k);
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const startedRef = useRef(false);

  useEffect(() => {
    if (startedRef.current) return;
    startedRef.current = true;

    const unlistenPromise = listen<{
      received: number;
      total: number | null;
      phase: "downloading" | "extracting" | "ready";
    }>("anvil://progress", (e) => {
      const p = e.payload;
      if (p.phase === "downloading") {
        setPhase({ kind: "downloading", received: p.received, total: p.total });
      } else if (p.phase === "extracting") {
        setPhase({ kind: "extracting" });
      }
    });

    void (async () => {
      try {
        const cached = await invoke<boolean>("anvil_cached");
        if (!cached) {
          setPhase({ kind: "downloading", received: 0, total: null });
        } else {
          setPhase({ kind: "running" });
        }
        const result = await invoke<SimulationResult>("simulate_tx", {
          chainId: tx.chain,
          txHash: tx.hash,
        });
        setPhase({ kind: "done", result });
      } catch (e) {
        setPhase({ kind: "error", message: String(e) });
      }
    })();

    return () => {
      void unlistenPromise.then((u) => u());
    };
  }, [tx.chain, tx.hash]);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal sim-modal" onClick={(e) => e.stopPropagation()}>
        <header className="sim-head">
          <h3>{tr("simulate.title")}</h3>
          <button className="modal-close" onClick={onClose} aria-label="Close">
            ×
          </button>
        </header>
        <div className="sim-meta">
          <span className="muted">{tr("simulate.tx")}</span>
          <code className="mono">{tx.hash}</code>
        </div>
        <div className="sim-body">{renderPhase(phase, tr)}</div>
        <footer className="sim-foot">
          <button className="btn-secondary" onClick={onClose}>
            {tr("simulate.close")}
          </button>
        </footer>
      </div>
    </div>
  );
}

function renderPhase(phase: Phase, tr: (k: Parameters<typeof t>[1]) => string) {
  switch (phase.kind) {
    case "idle":
      return <p className="muted">{tr("simulate.starting")}</p>;
    case "downloading": {
      const pct =
        phase.total && phase.total > 0
          ? Math.floor((phase.received / phase.total) * 100)
          : null;
      return (
        <div className="sim-progress">
          <p>{tr("simulate.downloading")}</p>
          <div className="sim-bar">
            <div
              className="sim-bar-fill"
              style={{ width: pct == null ? "30%" : `${pct}%` }}
            />
          </div>
          <p className="muted small">
            {formatMb(phase.received)}
            {phase.total ? ` / ${formatMb(phase.total)}` : ""}
            {pct != null ? ` (${pct}%)` : ""}
          </p>
        </div>
      );
    }
    case "extracting":
      return <p>{tr("simulate.extracting")}</p>;
    case "running":
      return (
        <div className="sim-running">
          <p>{tr("simulate.running")}</p>
          <p className="muted small">{tr("simulate.running.detail")}</p>
        </div>
      );
    case "error":
      return (
        <div className="sim-error">
          <p>
            <strong>{tr("simulate.error")}</strong>
          </p>
          <pre className="sim-err-msg">{phase.message}</pre>
        </div>
      );
    case "done":
      return <SimulationView result={phase.result} tr={tr} />;
  }
}

function SimulationView({
  result,
  tr,
}: {
  result: SimulationResult;
  tr: (k: Parameters<typeof t>[1]) => string;
}) {
  return (
    <div className="sim-result">
      <div className={`sim-status ${result.success ? "ok" : "fail"}`}>
        {result.success ? tr("simulate.success") : tr("simulate.failed")}
        {!result.success && result.revert_reason && (
          <span className="muted small"> — {result.revert_reason}</span>
        )}
      </div>
      <dl className="sim-grid">
        <dt>{tr("simulate.gas_used")}</dt>
        <dd>{result.gas_used.toLocaleString()}</dd>
        <dt>{tr("simulate.gas_price")}</dt>
        <dd>{result.effective_gas_price_gwei.toFixed(2)} gwei</dd>
        <dt>{tr("simulate.fork_block")}</dt>
        <dd>{result.block_number ? `#${result.block_number}` : "—"}</dd>
        <dt>{tr("simulate.logs")}</dt>
        <dd>{result.log_count}</dd>
      </dl>
      {result.transfers.length > 0 ? (
        <div className="sim-transfers">
          <h4>{tr("simulate.transfers")}</h4>
          <ul>
            {result.transfers.map((t, i) => (
              <li key={i}>
                <code className="mono">{shortAddr(t.token)}</code>{" "}
                <span className="muted">{tr("simulate.transfer.from")}</span>{" "}
                <code className="mono">{shortAddr(t.from)}</code>{" "}
                <span className="muted">{tr("simulate.transfer.to")}</span>{" "}
                <code className="mono">{shortAddr(t.to)}</code>{" "}
                <span className="muted">{tr("simulate.transfer.value")}</span>{" "}
                <code>{shortValue(t.value_raw)}</code>
              </li>
            ))}
          </ul>
        </div>
      ) : (
        <p className="muted small">{tr("simulate.no_transfers")}</p>
      )}
    </div>
  );
}

function shortAddr(a: string) {
  if (!a) return "";
  if (a.length < 12) return a;
  return `${a.slice(0, 6)}…${a.slice(-4)}`;
}

function shortValue(raw: string) {
  // raw is a base-10 uint256 string. Show the full thing if short, otherwise
  // shorten with leading digits + magnitude hint (10^N) so the user gets a
  // sense of scale without us guessing token decimals.
  if (raw.length <= 8) return raw;
  const head = raw.slice(0, 4);
  return `${head}…(10^${raw.length - 1})`;
}

function formatMb(bytes: number) {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
