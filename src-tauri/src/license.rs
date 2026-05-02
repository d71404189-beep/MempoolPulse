//! License verification + Hardware ID fingerprinting.
//!
//! ## Flow
//!
//! 1. On first launch `license_status` returns `valid: false` → frontend shows
//!    LicenseGate.
//! 2. User enters their Gumroad key → frontend calls `verify_license`.
//! 3. We compute the current machine's HWID, verify the key against Gumroad
//!    with `increment_uses_count=true`, and if successful persist both the key
//!    and the HWID to `settings.json`.
//! 4. On every subsequent launch we compare the saved HWID to the current one.
//!    Match → skip network, open straight to the app.
//!    Mismatch (new machine / reinstall) → force re-activation, which burns
//!    one more use on Gumroad.
//!
//! ## Gumroad setup recommendation
//!
//! In your Gumroad product settings → "License keys" section, set
//! **"Activations limit"** to **2**. This allows a user to activate on their
//! main machine and one reinstall / hardware change before you're notified of
//! unusual activity. If you want strictly one machine at a time, set it to 1
//! and tell buyers to contact you for a transfer.
//!
//! ## HWID sources (cross-platform)
//!
//! We collect several signals and hash them together with SHA-256 so the
//! result is stable across reboots but unique per physical machine:
//!   - Total physical memory (rounded to GiB)
//!   - Number of CPU cores reported by the OS
//!   - OS name + long OS version string
//!   - MAC address of the first non-loopback network interface
//!
//! No single signal is perfect, but the combination makes accidental collisions
//! extremely unlikely without requiring elevated privileges.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use sysinfo::System;

/// Compile-time product permalink.
///   `GUMROAD_PRODUCT_PERMALINK=mempoolpulse cargo tauri build`
const PRODUCT_PERMALINK: Option<&str> = option_env!("GUMROAD_PRODUCT_PERMALINK");

// ---------------------------------------------------------------------------
// Hardware ID
// ---------------------------------------------------------------------------

/// Collect a stable hardware fingerprint and return it as a lowercase hex
/// SHA-256 digest. The raw signals are joined with `|` before hashing so
/// positional swaps between fields don't accidentally produce the same hash.
pub fn compute_hwid() -> String {
    let mut sys = System::new_all();
    sys.refresh_all();

    // -- CPU core count -------------------------------------------------------
    let cpu_cores = sys.cpus().len();

    // -- Physical memory (rounded to nearest GiB so small driver-level
    //    fluctuations don't change the fingerprint) ---------------------------
    let ram_gib = (sys.total_memory() + 512 * 1024 * 1024) / (1024 * 1024 * 1024);

    // -- OS identity ----------------------------------------------------------
    let os_name = System::name().unwrap_or_else(|| "unknown".into());
    let os_ver = System::os_version().unwrap_or_else(|| "unknown".into());

    // -- MAC address of first non-loopback interface --------------------------
    // We use std::process to run a platform-appropriate command rather than
    // pulling in a heavy networking crate. Falls back to an empty string so
    // the HWID still computes — just with one less signal.
    let mac = first_mac_address();

    // Concatenate all signals
    let raw = format!("{}|{}|{}|{}|{}", cpu_cores, ram_gib, os_name, os_ver, mac);

    // SHA-256
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

/// Returns the MAC address of the first active non-loopback interface, or an
/// empty string if none can be determined.
fn first_mac_address() -> String {
    // Cross-platform: parse `ipconfig /all` on Windows, `ifconfig` or
    // `ip link` on Unix. We keep it simple — just look for the first
    // hex-colon or hex-dash sequence that looks like a MAC.
    #[cfg(target_os = "windows")]
    let output = std::process::Command::new("ipconfig")
        .arg("/all")
        .output();

    #[cfg(not(target_os = "windows"))]
    let output = std::process::Command::new("sh")
        .args(["-c", "ip link show 2>/dev/null || ifconfig 2>/dev/null"])
        .output();

    let text = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return String::new(),
    };

    // Match XX:XX:XX:XX:XX:XX or XX-XX-XX-XX-XX-XX
    let re_pat = regex_mac(&text);
    re_pat.unwrap_or_default()
}

/// Minimal MAC-address extractor without pulling in the `regex` crate.
fn regex_mac(text: &str) -> Option<String> {
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        for part in parts {
            // Normalize separators
            let norm = part.replace('-', ":");
            let segs: Vec<&str> = norm.split(':').collect();
            if segs.len() == 6 && segs.iter().all(|s| s.len() == 2 && s.chars().all(|c| c.is_ascii_hexdigit())) {
                // Skip all-zero (loopback / virtual) and broadcast
                if norm != "00:00:00:00:00:00" && norm != "ff:ff:ff:ff:ff:ff" {
                    return Some(norm.to_lowercase());
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Gumroad API types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GumroadResponse {
    success: bool,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    purchase: Option<GumroadPurchase>,
}

#[derive(Deserialize)]
struct GumroadPurchase {
    #[serde(default)]
    refunded: bool,
    #[serde(default)]
    chargebacked: bool,
    #[serde(default)]
    disputed: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result returned to the frontend.
#[derive(serde::Serialize)]
pub struct VerifyResult {
    pub valid: bool,
    pub message: String,
    /// The HWID that was used during this verification. The caller must persist
    /// this alongside the license key so the HWID check works on next launch.
    pub hwid: String,
}

/// Verify a Gumroad license key and bind it to the current machine.
///
/// - First activation: calls Gumroad with `increment_uses_count=true` so the
///   use counter ticks up.
/// - Re-activation on same HWID: calls with `increment_uses_count=false`
///   (e.g., app re-installed but same machine).
/// - Activation on a *different* HWID: calls with `increment_uses_count=true`
///   (new machine counts as a new activation slot).
pub async fn verify(key: &str, saved_hwid: &str) -> VerifyResult {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return VerifyResult {
            valid: false,
            message: "License key is empty.".into(),
            hwid: String::new(),
        };
    }

    let current_hwid = compute_hwid();

    // Development shortcut — no product permalink configured.
    let permalink = match PRODUCT_PERMALINK {
        Some(p) if !p.is_empty() => p,
        _ => {
            return VerifyResult {
                valid: true,
                message: "Development mode — no product permalink configured.".into(),
                hwid: current_hwid,
            };
        }
    };

    // Only increment uses_count when activating on a NEW machine.
    // Same machine re-activation (e.g., after reinstall) should not burn a slot.
    let increment = saved_hwid.is_empty() || saved_hwid != current_hwid;

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return VerifyResult {
                valid: false,
                message: format!("HTTP client error: {e}"),
                hwid: current_hwid,
            }
        }
    };

    let resp = client
        .post("https://api.gumroad.com/v2/licenses/verify")
        .form(&[
            ("product_permalink", permalink),
            ("license_key", trimmed),
            ("increment_uses_count", if increment { "true" } else { "false" }),
        ])
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            return VerifyResult {
                valid: false,
                message: format!("Network error: {e}. Check your internet connection."),
                hwid: current_hwid,
            }
        }
    };

    let parsed: GumroadResponse = match resp.json().await {
        Ok(p) => p,
        Err(e) => {
            return VerifyResult {
                valid: false,
                message: format!("Could not parse Gumroad response: {e}"),
                hwid: current_hwid,
            }
        }
    };

    if !parsed.success {
        return VerifyResult {
            valid: false,
            message: parsed
                .message
                .unwrap_or_else(|| "License key not recognised.".into()),
            hwid: current_hwid,
        };
    }

    if let Some(p) = parsed.purchase {
        if p.refunded || p.chargebacked || p.disputed {
            return VerifyResult {
                valid: false,
                message: "This license has been refunded or charged back.".into(),
                hwid: current_hwid,
            };
        }
    }

    VerifyResult {
        valid: true,
        message: "License activated successfully.".into(),
        hwid: current_hwid,
    }
}

/// Check whether the locally saved key + HWID are still valid for this machine.
/// Returns `(is_valid, needs_reactivation)`.
///
/// - `(true, false)`  → same machine, skip network, proceed normally.
/// - `(false, true)`  → key present but HWID changed → force re-activation UI.
/// - `(false, false)` → no key saved → first launch.
pub fn local_check(saved_key: &str, saved_hwid: &str) -> (bool, bool) {
    if saved_key.is_empty() {
        return (false, false); // first launch
    }
    if saved_hwid.is_empty() {
        return (false, true); // key exists but no HWID recorded (old install)
    }
    let current = compute_hwid();
    if current == saved_hwid {
        (true, false) // same machine ✓
    } else {
        (false, true) // different machine → re-activate
    }
}
