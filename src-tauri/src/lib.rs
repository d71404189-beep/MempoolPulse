mod anvil;
mod decoder;
mod license;
mod mempool;
mod non_evm;
mod prices;
mod simulate;
mod state;
mod types;

use crate::simulate::SimulationResult;
use crate::state::AppState;
use crate::types::{AppSettings, ConnectionStatus, LicenseStatus};
use std::path::PathBuf;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

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
    let saved_hwid = state.settings.read().activated_hwid.clone();
    let result = license::verify(&key, &saved_hwid).await;
    if result.valid {
        let mut s = state.settings.write();
        s.license_key = key;
        s.activated_hwid = result.hwid.clone();
        drop(s);
        let _ = state.save_settings();
    }
    Ok(LicenseStatus {
        valid: result.valid,
        message: result.message,
    })
}

#[tauri::command]
fn anvil_cached(app: AppHandle) -> bool {
    anvil::is_cached(&app)
}

#[tauri::command]
async fn simulate_tx(
    chain_id: String,
    tx_hash: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SimulationResult, String> {
    let chain = {
        let s = state.settings.read();
        s.chains
            .iter()
            .find(|c| c.id == chain_id)
            .cloned()
            .ok_or_else(|| format!("Unknown chain id: {chain_id}"))?
    };
    simulate::simulate(app, chain, tx_hash)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn license_status(state: tauri::State<'_, AppState>) -> LicenseStatus {
    let s = state.settings.read();
    let (valid, needs_reactivation) =
        license::local_check(&s.license_key, &s.activated_hwid);
    if needs_reactivation {
        return LicenseStatus {
            valid: false,
            message: "hardware_changed".into(),
        };
    }
    LicenseStatus {
        valid,
        message: if valid {
            "License active.".into()
        } else {
            "No license key saved.".into()
        },
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_tray::init())
        .setup(|app| {
            let settings_path = settings_path(app.handle());
            let state = AppState::new(settings_path);
            app.manage(state);

            // Build tray icon menu
            let show = MenuItem::with_id(app, "show", "Показать", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("MempoolPulse")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // Двойной клик по иконке трея — показать окно
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            // Перехватываем закрытие окна — сворачиваем в трей вместо выхода
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            start_streaming,
            stop_streaming,
            connection_status,
            verify_license,
            license_status,
            anvil_cached,
            simulate_tx,
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
