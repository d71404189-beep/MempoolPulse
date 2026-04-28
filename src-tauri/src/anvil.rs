//! Lazy-downloads and caches the `anvil` binary from foundry-rs releases for
//! the v1.5 transaction-simulation feature.
//!
//! The Tauri installer ships ~2 MB. We avoid bundling foundry directly
//! (~25 MB per platform) by downloading it on the user's first simulation
//! click and caching it under the app data directory. Subsequent simulations
//! reuse the cached binary.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

/// Pinned foundry release tag. Bump (and re-test the simulation flow) when
/// foundry ships a new stable.
pub const FOUNDRY_VERSION: &str = "v1.7.0";

/// Tauri event channel used to push download progress (0..1) to the
/// frontend modal.
pub const PROGRESS_EVENT: &str = "anvil://progress";

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: Option<u64>,
    /// One of: "downloading", "extracting", "ready".
    pub phase: &'static str,
}

fn asset_name() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("foundry_v1.7.0_darwin_arm64.tar.gz")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("foundry_v1.7.0_darwin_amd64.tar.gz")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("foundry_v1.7.0_linux_amd64.tar.gz")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("foundry_v1.7.0_linux_arm64.tar.gz")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("foundry_v1.7.0_win32_amd64.zip")
    } else {
        None
    }
}

fn anvil_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "anvil.exe"
    } else {
        "anvil"
    }
}

pub fn cache_dir(app: &AppHandle) -> Result<PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .context("app_data_dir unavailable")?;
    Ok(base.join("anvil-cache").join(FOUNDRY_VERSION))
}

pub fn anvil_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(cache_dir(app)?.join(anvil_filename()))
}

pub fn is_cached(app: &AppHandle) -> bool {
    anvil_path(app).map(|p| p.exists()).unwrap_or(false)
}

/// Downloads (if needed) and extracts anvil. Idempotent — if the binary is
/// already cached, returns immediately. Emits PROGRESS_EVENT during download.
pub async fn ensure(app: AppHandle) -> Result<PathBuf> {
    if is_cached(&app) {
        return anvil_path(&app);
    }
    let asset = asset_name().context("Anvil simulation is not supported on this platform")?;
    let url = format!(
        "https://github.com/foundry-rs/foundry/releases/download/{FOUNDRY_VERSION}/{asset}"
    );
    let dir = cache_dir(&app)?;
    std::fs::create_dir_all(&dir)?;

    let _ = app.emit(
        PROGRESS_EVENT,
        DownloadProgress {
            received: 0,
            total: None,
            phase: "downloading",
        },
    );

    // Stream the download so we can emit progress periodically. The archives
    // are ~25-30 MB so a streaming approach keeps the UI responsive on slow
    // connections.
    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await?
        .error_for_status()?;
    let total = resp.content_length();
    let mut received: u64 = 0;
    let mut buf: Vec<u8> = Vec::with_capacity(total.unwrap_or(20 * 1024 * 1024) as usize);
    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;
    let mut last_emit = std::time::Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        received += chunk.len() as u64;
        buf.extend_from_slice(&chunk);
        if last_emit.elapsed() >= std::time::Duration::from_millis(200) {
            let _ = app.emit(
                PROGRESS_EVENT,
                DownloadProgress {
                    received,
                    total,
                    phase: "downloading",
                },
            );
            last_emit = std::time::Instant::now();
        }
    }

    let _ = app.emit(
        PROGRESS_EVENT,
        DownloadProgress {
            received,
            total,
            phase: "extracting",
        },
    );

    // Extract to a .tmp sibling first, then rename once fully written. This
    // is atomic on both Unix and Windows (ReplaceFile) and avoids leaving a
    // corrupt partial binary under `dest` if the process is killed mid-write,
    // which would otherwise make `is_cached()` return a false positive on
    // the next launch.
    let dest = dir.join(anvil_filename());
    let tmp = dir.join(format!("{}.tmp", anvil_filename()));
    let _ = std::fs::remove_file(&tmp);
    if asset.ends_with(".zip") {
        extract_anvil_from_zip(&buf, &tmp)?;
    } else {
        extract_anvil_from_targz(&buf, &tmp)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .context("chmod +x on anvil")?;
    }
    std::fs::rename(&tmp, &dest)
        .with_context(|| format!("rename {} → {}", tmp.display(), dest.display()))?;

    let _ = app.emit(
        PROGRESS_EVENT,
        DownloadProgress {
            received,
            total,
            phase: "ready",
        },
    );

    Ok(dest)
}

fn extract_anvil_from_targz(buf: &[u8], dest: &Path) -> Result<()> {
    let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(buf));
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let name = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_default();
        if name == "anvil" {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(anyhow!("anvil binary not found in foundry tar.gz"))
}

fn extract_anvil_from_zip(buf: &[u8], dest: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(buf))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let last = name
            .rsplit(|c| c == '/' || c == '\\')
            .next()
            .unwrap_or(&name);
        if last == "anvil.exe" || last == "anvil" {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(anyhow!("anvil binary not found in foundry zip"))
}
