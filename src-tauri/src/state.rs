use crate::prices::PriceFetcher;
use crate::types::{AppSettings, ConnectionStatus};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state held by the Tauri app. All fields are cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<RwLock<AppSettings>>,
    pub settings_path: Arc<PathBuf>,
    pub prices: PriceFetcher,
    /// Holds the JoinHandle of the running mempool worker so we can cancel it
    /// when settings change. `Mutex` (async) because we need to await on
    /// shutdown signalling cleanly.
    pub worker: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Cancellation flag observed by the worker loop.
    pub shutdown: Arc<RwLock<bool>>,
    /// Last known connection status, surfaced to the UI on demand. Stores
    /// both an English fallback message and a localization `code`+`params`
    /// pair so the frontend can render in the user's language.
    pub connection: Arc<RwLock<ConnectionStatus>>,
}

impl AppState {
    pub fn new(settings_path: PathBuf) -> Self {
        let settings = load_settings(&settings_path);
        Self {
            settings: Arc::new(RwLock::new(settings)),
            settings_path: Arc::new(settings_path),
            prices: PriceFetcher::new(),
            worker: Arc::new(Mutex::new(None)),
            shutdown: Arc::new(RwLock::new(false)),
            connection: Arc::new(RwLock::new(ConnectionStatus {
                connected: false,
                message: "Idle".into(),
                code: Some("status.idle".into()),
                params: None,
            })),
        }
    }

    pub fn save_settings(&self) -> anyhow::Result<()> {
        let snapshot = { self.settings.read().clone() };
        let json = serde_json::to_string_pretty(&snapshot)?;
        if let Some(parent) = self.settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(self.settings_path.as_path(), json)?;
        Ok(())
    }

    pub fn set_connection(&self, status: ConnectionStatus) {
        *self.connection.write() = status;
    }
}

fn load_settings(path: &PathBuf) -> AppSettings {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|_| default_settings()),
        Err(_) => default_settings(),
    }
}

fn default_settings() -> AppSettings {
    let mut s = AppSettings::default();
    s.filters.buffer_size = 500;
    s.filters.min_value_eth = 1.0;
    s
}
