mod decoder;
mod license;
mod mempool;
mod prices;
mod state;
mod types;

use crate::state::AppState;
use crate::types::{AppSettings, ConnectionStatus, LicenseStatus};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> AppSettings {
    state.settings.read().clone()
}

#[tauri::command]
async fn save_settings(
    new_settings: AppSettings,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let needs_restart = {
        let mut current = state.settings.write();
        // Restart workers if any chain config changed (URL, enabled flag, etc.).
        let restart = serde_json::to_value(&current.chains).ok()
            != serde_json::to_value(&new_settings.chains).ok();
        *current = new_settings;
        current.normalize();
        restart
    };
    state.save_settings().map_err(|e| e.to_string())?;
    if needs_restart {
        mempool::restart(app, (*state).clone()).await;
    }
    Ok(())
}

#[tauri::command]
async fn start_streaming(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    mempool::restart(app, (*state).clone()).await;
    Ok(())
}

#[tauri::command]
async fn stop_streaming(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    *state.shutdown.write() = true;
    let mut guard = state.workers.lock().await;
    for (_, handle) in guard.drain() {
        handle.abort();
    }
    mempool::emit_all_stopped(&app, &*state);
    Ok(())
}

#[tauri::command]
fn connection_status(state: tauri::State<'_, AppState>) -> Vec<ConnectionStatus> {
    state.snapshot_connections()
}

#[tauri::command]
async fn verify_license(
    key: String,
    state: tauri::State<'_, AppState>,
) -> Result<LicenseStatus, String> {
    let (valid, message) = license::verify(&key).await;
    if valid {
        state.settings.write().license_key = key;
        let _ = state.save_settings();
    }
    Ok(LicenseStatus { valid, message })
}

#[tauri::command]
fn license_status(state: tauri::State<'_, AppState>) -> LicenseStatus {
    let key = state.settings.read().license_key.clone();
    LicenseStatus {
        valid: !key.is_empty(),
        message: if key.is_empty() {
            "No license key saved.".into()
        } else {
            "License key present.".into()
        },
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let settings_path = settings_path(app.handle());
            let state = AppState::new(settings_path);
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            start_streaming,
            stop_streaming,
            connection_status,
            verify_license,
            license_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn settings_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join("settings.json")
}
