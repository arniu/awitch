//! Loads `config.toml` — startup-only settings.
//!
//! `config.toml` holds what the gateway must know before it can start.
//! It is read once at startup and never written by the gateway.
//!
//! Rule: A setting the gateway might read or change while running does
//! not go here — it goes in the `settings` table in data.db.

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use serde::Deserialize;

/// The environment override for `key`. Unset and set-but-empty both read as
/// absent, so `AWITCH_PORT=` never overrides.
fn env_override(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Config directory: `AWITCH_CONFIG_DIR` env var, or `~/.awitch/`.
pub fn config_dir() -> PathBuf {
    env_override("AWITCH_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".awitch"))
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: ServerSection,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerSection {
    pub host: String,
    pub port: u16,
    pub control_host: String,
    pub control_port: u16,
}

impl Default for ServerSection {
    fn default() -> Self {
        ServerSection {
            host: default_host(),
            port: default_port(),
            control_host: default_host(),
            control_port: default_control_port(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

const fn default_port() -> u16 {
    10689
}

const fn default_control_port() -> u16 {
    10690
}

impl Config {
    /// Load the config under `config_dir`.
    ///
    /// - Missing file or `[server]` section → `ServerSection::default()`;
    /// - Missing fields within `[server]` → that field's default;
    /// - Malformed or unreadable → error.
    pub fn load(config_dir: &Path) -> anyhow::Result<Config> {
        let path = config_dir.join("config.toml");
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
        };

        toml::from_str(&raw).map_err(|e| anyhow!("{}: invalid config.toml: {e}", path.display()))
    }

    /// The data bind/dial host, overridden by `AWITCH_HOST`.
    pub fn host(&self) -> String {
        env_override("AWITCH_HOST").unwrap_or_else(|| self.server.host.clone())
    }

    /// The data port, overridden by `AWITCH_PORT`.
    pub fn port(&self) -> u16 {
        env_override("AWITCH_PORT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.server.port)
    }

    /// The control bind/dial host, overridden by `AWITCH_CONTROL_HOST`.
    pub fn control_host(&self) -> String {
        env_override("AWITCH_CONTROL_HOST").unwrap_or_else(|| self.server.control_host.clone())
    }

    /// The control port, overridden by `AWITCH_CONTROL_PORT`.
    pub fn control_port(&self) -> u16 {
        env_override("AWITCH_CONTROL_PORT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.server.control_port)
    }
}
