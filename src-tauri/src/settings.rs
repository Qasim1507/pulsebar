//! Non-secret settings live in a JSON file; secrets (client secret, OAuth
//! tokens) live in the OS credential store — macOS Keychain or Windows
//! Credential Manager — which encrypts them at rest.

use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const KEYCHAIN_SERVICE: &str = "com.pulsebar.widget";
pub const KEY_CLIENT_SECRET: &str = "whoop_client_secret";
pub const KEY_TOKENS: &str = "whoop_tokens";

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub client_id: String,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub max_hr: u16,
    pub demo_mode: bool,
    pub stress_notify: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            device_id: None,
            device_name: None,
            max_hr: 190,
            demo_mode: true,
            stress_notify: true,
        }
    }
}

pub fn load(path: &Path) -> Settings {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

// The credential store can block (and on macOS may show a prompt), so these
// run on the blocking pool.

pub async fn secret_get(key: &'static str) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        keyring::Entry::new(KEYCHAIN_SERVICE, key).ok()?.get_password().ok()
    })
    .await
    .ok()
    .flatten()
}

pub async fn secret_set(key: &'static str, value: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        keyring::Entry::new(KEYCHAIN_SERVICE, key)
            .and_then(|e| e.set_password(&value))
            .map_err(|e| format!("Could not save to the system credential store: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn secret_delete(key: &'static str) {
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, key) {
            let _ = entry.delete_credential();
        }
    })
    .await;
}
