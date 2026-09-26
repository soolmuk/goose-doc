//! Settings persistence. Only user-facing choices live here, so the panel
//! reopens with what the user last used.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::addr;

/// Panel and serve settings. No secrets are stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Bind address. Defaults to every interface, so a server is reachable from
    /// other machines.
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Docs root to serve, if the user chose a local directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs_dir: Option<PathBuf>,
    /// Bundle version to serve, if the user chose one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs_version: Option<String>,
    /// Open the browser automatically when the server starts.
    #[serde(default)]
    pub open_browser: bool,
}

fn default_bind() -> String {
    addr::DEFAULT_BIND.to_string()
}

fn default_port() -> u16 {
    crate::config::DEFAULT_PORT
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            port: default_port(),
            docs_dir: None,
            docs_version: None,
            open_browser: false,
        }
    }
}

impl Settings {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let settings: Settings = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(settings)
    }

    /// Load settings, falling back to defaults when the file is absent.
    pub fn load_or_default(path: &std::path::Path) -> Self {
        Self::load(path).unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let raw = serde_yaml::to_string(self).context("failed to serialize settings")?;
        fs::write(path, raw).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("goose-doc-settings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("config.yaml")
    }

    #[test]
    fn defaults_bind_every_interface() {
        let settings = Settings::default();
        assert_eq!(settings.bind, "0.0.0.0");
        assert_eq!(settings.port, 10650);
    }

    #[test]
    fn round_trips_through_a_file() {
        let path = temp_path("roundtrip");
        let settings = Settings {
            bind: "192.168.1.5".to_string(),
            port: 20000,
            docs_dir: Some(PathBuf::from("/opt/goose-docs")),
            docs_version: Some("1.52.0".to_string()),
            open_browser: true,
        };

        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let path = temp_path("missing");
        let settings = Settings::load_or_default(&path);
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn partial_file_keeps_defaults_for_absent_keys() {
        let path = temp_path("partial");
        Settings::default().save(&path).unwrap();
        fs::write(&path, "port: 12345\n").unwrap();

        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings.port, 12345);
        assert_eq!(settings.bind, "0.0.0.0");
    }

    #[test]
    fn saved_file_has_no_secrets() {
        let path = temp_path("secrets");
        Settings::default().save(&path).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        for key in ["password", "token", "secret", "key"] {
            assert!(!raw.contains(key), "settings should not contain {key}");
        }
    }
}
