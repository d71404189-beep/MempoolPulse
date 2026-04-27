import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { AppSettings, LicenseStatus } from "../types";

interface Props {
  onActivated: (settings: AppSettings) => void;
}

export default function LicenseGate({ onActivated }: Props) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const status: LicenseStatus = await invoke("verify_license", { key });
      if (!status.valid) {
        setError(status.message);
        return;
      }
      const settings: AppSettings = await invoke("get_settings");
      onActivated(settings);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="license-gate">
      <form className="license-card" onSubmit={submit}>
        <h1>MempoolPulse</h1>
        <p>
          Enter your license key to unlock the live mempool feed.<br />
          You can purchase a key from{" "}
          <a
            href="https://gumroad.com/l/mempoolpulse"
            onClick={(e) => {
              e.preventDefault();
              void openUrl("https://gumroad.com/l/mempoolpulse");
            }}
          >
            gumroad.com/l/mempoolpulse
          </a>
          .
        </p>
        <input
          autoFocus
          placeholder="XXXX-XXXX-XXXX-XXXX"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          spellCheck={false}
        />
        <button className="btn" type="submit" disabled={busy || !key.trim()}>
          {busy ? "Verifying…" : "Activate"}
        </button>
        {error && <div className="error">{error}</div>}
      </form>
    </div>
  );
}
