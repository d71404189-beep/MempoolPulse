import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { AppSettings, LicenseStatus } from "../types";
import { t, type Lang } from "../i18n";

interface Props {
  onActivated: (settings: AppSettings) => void;
  lang: Lang;
  /** Passed from App when license_status returns "hardware_changed" */
  hardwareChanged?: boolean;
}

type UIState = "idle" | "loading" | "error" | "success";

export default function LicenseGate({ onActivated, lang, hardwareChanged = false }: Props) {
  const [key, setKey] = useState("");
  const [uiState, setUiState] = useState<UIState>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const tr = (k: Parameters<typeof t>[1]) => t(lang, k);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!key.trim()) return;

    setUiState("loading");
    setErrorMsg(null);

    try {
      const status: LicenseStatus = await invoke("verify_license", { key: key.trim() });

      if (!status.valid) {
        setUiState("error");
        // Map backend sentinel to a user-friendly message
        if (status.message === "hardware_changed") {
          setErrorMsg(
            lang === "ru"
              ? "Этот ключ привязан к другому устройству. Активация израсходует один слот."
              : "This key is bound to another device. Activating will use one slot."
          );
        } else {
          setErrorMsg(status.message);
        }
        return;
      }

      setUiState("success");
      // Small visual pause so the user sees the success state
      await new Promise((r) => setTimeout(r, 600));
      const settings: AppSettings = await invoke("get_settings");
      onActivated(settings);
    } catch (err) {
      setUiState("error");
      setErrorMsg(
        lang === "ru"
          ? "Ошибка сети. Проверьте подключение к интернету."
          : "Network error. Check your internet connection."
      );
    }
  };

  return (
    <div className="license-gate">
      <div className="license-card">
        {/* Logo / Brand */}
        <div className="license-brand">
          <span className="brand-dot" />
          <span className="brand-name">MempoolPulse</span>
        </div>

        {/* Hardware-changed warning banner */}
        {hardwareChanged && (
          <div className="license-warn">
            {lang === "ru"
              ? "⚠️ Обнаружено новое устройство. Введите ключ для повторной активации."
              : "⚠️ New hardware detected. Please re-enter your license key."}
          </div>
        )}

        <h2 className="license-title">{tr("license.title")}</h2>
        <p className="license-intro">
          {tr("license.intro")}{" "}
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

        <form onSubmit={handleSubmit} className="license-form">
          <input
            autoFocus
            className={`license-input${uiState === "error" ? " license-input--error" : ""}${uiState === "success" ? " license-input--success" : ""}`}
            placeholder={tr("license.placeholder")}
            value={key}
            onChange={(e) => {
              setKey(e.target.value);
              if (uiState === "error") {
                setUiState("idle");
                setErrorMsg(null);
              }
            }}
            spellCheck={false}
            autoComplete="off"
            disabled={uiState === "loading" || uiState === "success"}
          />

          <button
            className={`btn license-btn${uiState === "loading" ? " license-btn--loading" : ""}${uiState === "success" ? " license-btn--success" : ""}`}
            type="submit"
            disabled={uiState === "loading" || uiState === "success" || !key.trim()}
          >
            {uiState === "loading" && (
              <span className="license-spinner" aria-hidden="true" />
            )}
            {uiState === "success"
              ? (lang === "ru" ? "✓ Активировано" : "✓ Activated")
              : uiState === "loading"
              ? tr("license.verifying")
              : tr("license.activate")}
          </button>
        </form>

        {/* Error message */}
        {uiState === "error" && errorMsg && (
          <div className="license-error" role="alert">
            <span className="license-error-icon">✕</span>
            {errorMsg}
          </div>
        )}

        {/* Help link */}
        <p className="license-help">
          {lang === "ru" ? "Нет ключа? " : "Don't have a key? "}
          <a
            href="https://gumroad.com/l/mempoolpulse"
            onClick={(e) => {
              e.preventDefault();
              void openUrl("https://gumroad.com/l/mempoolpulse");
            }}
          >
            {lang === "ru" ? "Купить лицензию" : "Purchase a license"}
          </a>
        </p>
      </div>
    </div>
  );
}
