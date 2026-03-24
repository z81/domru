use std::path::Path;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

use crate::client::DomofonClient;
use crate::types::{AppConfig, CallEvent, SipCredentials, TokensData};

const DATA_DIR: &str = "./data";
const TOKENS_FILE: &str = "tokens.json";
const CONFIG_FILE: &str = "config.json";
const SIP_DEVICE_FILE: &str = "sip-device.json";
const BROADCAST_CAPACITY: usize = 64;

pub type SharedState = Arc<AppState>;

pub struct AppState {
    pub client: RwLock<DomofonClient>,
    pub config: RwLock<AppConfig>,
    pub call_tx: broadcast::Sender<CallEvent>,
    /// Channel to tell SIP client to answer (200 OK) the current INVITE.
    pub sip_answer_tx: mpsc::Sender<()>,
    pub sip_answer_rx: Mutex<mpsc::Receiver<()>>,
    /// Last raw SIP INVITE message (for building 200 OK response).
    pub last_invite: RwLock<Option<String>>,
}

impl AppState {
    pub fn new(client: DomofonClient, config: AppConfig) -> SharedState {
        let (call_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let (sip_answer_tx, sip_answer_rx) = mpsc::channel(4);
        Arc::new(Self {
            client: RwLock::new(client),
            config: RwLock::new(config),
            call_tx,
            sip_answer_tx,
            sip_answer_rx: Mutex::new(sip_answer_rx),
            last_invite: RwLock::new(None),
        })
    }
}

// ─── Persistence helpers ─────────────────────────────

fn data_path(filename: &str) -> std::path::PathBuf {
    Path::new(DATA_DIR).join(filename)
}

pub fn load_tokens(path: &Path) -> Option<TokensData> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(err) => {
            tracing::debug!("Could not read tokens file {}: {}", path.display(), err);
            return None;
        }
    };

    match serde_json::from_str::<TokensData>(&content) {
        Ok(tokens) if !tokens.access_token.is_empty() && !tokens.refresh_token.is_empty() => {
            Some(tokens)
        }
        Ok(_) => {
            tracing::debug!("Tokens file has empty access or refresh token");
            None
        }
        Err(err) => {
            tracing::warn!("Failed to parse tokens file {}: {}", path.display(), err);
            None
        }
    }
}

pub fn save_tokens(path: &Path, tokens: &TokensData) {
    ensure_parent_dir(path);
    match serde_json::to_string_pretty(tokens) {
        Ok(json) => {
            if let Err(err) = std::fs::write(path, json) {
                tracing::error!("Failed to write tokens to {}: {}", path.display(), err);
            }
        }
        Err(err) => {
            tracing::error!("Failed to serialize tokens: {}", err);
        }
    }
}

pub fn load_config(path: &Path) -> AppConfig {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return AppConfig::default(),
    };

    let mut config = match serde_json::from_str::<AppConfig>(&content) {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!("Failed to parse config file {}: {}, using defaults", path.display(), err);
            return AppConfig::default();
        }
    };

    // Fallback to WEBHOOK_URL env if config has empty webhook
    if config.call_webhook_url.is_empty() {
        if let Ok(url) = std::env::var("WEBHOOK_URL") {
            if !url.is_empty() {
                config.call_webhook_url = url;
            }
        }
    }

    config
}

pub fn save_config(path: &Path, config: &AppConfig) {
    ensure_parent_dir(path);
    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if let Err(err) = std::fs::write(path, json) {
                tracing::error!("Failed to write config to {}: {}", path.display(), err);
            }
        }
        Err(err) => {
            tracing::error!("Failed to serialize config: {}", err);
        }
    }
}

pub fn load_sip_credentials(path: &Path) -> Option<SipCredentials> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(err) => {
            tracing::debug!("Could not read SIP credentials file {}: {}", path.display(), err);
            return None;
        }
    };

    match serde_json::from_str::<SipCredentials>(&content) {
        Ok(creds) => Some(creds),
        Err(err) => {
            tracing::warn!("Failed to parse SIP credentials {}: {}", path.display(), err);
            None
        }
    }
}

pub fn save_sip_credentials(path: &Path, creds: &SipCredentials) {
    ensure_parent_dir(path);
    match serde_json::to_string_pretty(creds) {
        Ok(json) => {
            if let Err(err) = std::fs::write(path, json) {
                tracing::error!("Failed to write SIP credentials to {}: {}", path.display(), err);
            }
        }
        Err(err) => {
            tracing::error!("Failed to serialize SIP credentials: {}", err);
        }
    }
}

/// Convenience functions using default DATA_DIR paths.
pub fn tokens_path() -> std::path::PathBuf {
    data_path(TOKENS_FILE)
}

pub fn config_path() -> std::path::PathBuf {
    data_path(CONFIG_FILE)
}

pub fn sip_credentials_path() -> std::path::PathBuf {
    data_path(SIP_DEVICE_FILE)
}

fn ensure_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                tracing::error!("Failed to create directory {}: {}", parent.display(), err);
            }
        }
    }
}
