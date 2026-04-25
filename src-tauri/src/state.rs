use crate::prices::PriceFetcher;
use crate::types::{AppSettings, ConnectionStatus};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state held by the Tauri app. All fields are cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<RwLock<AppSettings>>,
    pub settings_path: Arc<PathBuf>,
    pub prices: PriceFetcher,
    /// One JoinHandle per running per-chain worker, keyed by chain id.
    /// `Mutex` (async) because we need to await on shutdown signalling cleanly.
    pub workers: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Cancellation flag observed by all worker loops.
    pub shutdown: Arc<RwLock<bool>>,
    /// Last known connection status per chain. Surfaced to the UI on demand
    /// and updated on every `mempool://status` emission.
    pub connection: Arc<RwLock<HashMap<String, ConnectionStatus>>>,
}

impl AppState {
    pub fn new(settings_path: PathBuf) -> Self {
        let settings = load_settings(&settings_path);
        let initial_conn = settings
            .chains
            .iter()
            .map(|c| {
                (
                    c.id.clone(),
                    ConnectionStatus {
                        chain: c.id.clone(),
                        connected: false,
                        message: "Idle".into(),
                        code: Some("status.idle".into()),
                        params: None,
                    },
                )
            })
            .collect();
        Self {
            settings: Arc::new(RwLock::new(settings)),
            settings_path: Arc::new(settings_path),
            prices: PriceFetcher::new(),
            workers: Arc::new(Mutex::new(HashMap::new())),
            shutdown: Arc::new(RwLock::new(false)),
            connection: Arc::new(RwLock::new(initial_conn)),
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
        self.connection.write().insert(status.chain.clone(), status);
    }

    pub fn snapshot_connections(&self) -> Vec<ConnectionStatus> {
        self.connection.read().values().cloned().collect()
    }
}

fn load_settings(path: &PathBuf) -> AppSettings {
    let mut s = match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|_| default_settings()),
        Err(_) => default_settings(),
    };
    s.normalize();
    s
}

fn default_settings() -> AppSettings {
    let mut s = AppSettings::default();
    s.filters.buffer_size = 500;
    s.filters.min_value_eth = 1.0;
    s
}
