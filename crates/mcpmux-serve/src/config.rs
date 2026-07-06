//! Configuration for the headless `mcpmux serve` binary.
//!
//! Precedence: built-in defaults < TOML file (`--config` / `MCPMUX_CONFIG`) <
//! environment variables. Every field has a safe default so `mcpmux serve`
//! with no arguments runs a loopback, auth-required gateway.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Raw config as parsed from TOML (all optional; env + defaults fill the rest).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub data_dir: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub public_base_url: Option<String>,
    pub auth_disabled: Option<bool>,
    pub additional_allowed_hosts: Option<Vec<String>>,
    pub allow_any_host: Option<bool>,
    pub log: Option<String>,
}

/// Fully resolved runtime configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub host: String,
    pub port: u16,
    pub public_base_url: Option<String>,
    pub auth_disabled: bool,
    pub additional_allowed_hosts: Vec<String>,
    pub allow_any_host: bool,
    pub log: String,
}

fn env_bool(key: &str) -> Option<bool> {
    std::env::var(key).ok().map(|v| {
        let v = v.trim().to_ascii_lowercase();
        v == "1" || v == "true" || v == "yes" || v == "on"
    })
}

impl Config {
    /// Resolve config from an optional TOML file path plus environment overrides.
    pub fn load(config_path: Option<PathBuf>) -> Result<Self> {
        let file: FileConfig = match config_path {
            Some(path) => {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading config file {path:?}"))?;
                toml::from_str(&text).with_context(|| format!("parsing config file {path:?}"))?
            }
            None => FileConfig::default(),
        };

        let data_dir = std::env::var("MCPMUX_DATA_DIR")
            .ok()
            .or(file.data_dir)
            .map(PathBuf::from)
            .unwrap_or_else(default_data_dir);

        let host = std::env::var("MCPMUX_HOST")
            .ok()
            .or(file.host)
            .unwrap_or_else(|| "127.0.0.1".to_string());

        let port = match std::env::var("MCPMUX_PORT").ok() {
            Some(p) => p.trim().parse().context("MCPMUX_PORT must be a u16")?,
            None => file.port.unwrap_or(mcpmux_core::DEFAULT_GATEWAY_PORT),
        };

        let public_base_url = std::env::var("MCPMUX_PUBLIC_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or(file.public_base_url);

        let auth_disabled = env_bool("MCPMUX_AUTH_DISABLED")
            .or(file.auth_disabled)
            .unwrap_or(false);

        let additional_allowed_hosts = std::env::var("MCPMUX_ALLOWED_HOSTS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .or(file.additional_allowed_hosts)
            .unwrap_or_default();

        let allow_any_host = env_bool("MCPMUX_ALLOW_ANY_HOST")
            .or(file.allow_any_host)
            .unwrap_or(false);

        let log = std::env::var("MCPMUX_LOG")
            .ok()
            .or_else(|| std::env::var("RUST_LOG").ok())
            .or(file.log)
            .unwrap_or_else(|| "info".to_string());

        Ok(Self {
            data_dir,
            host,
            port,
            public_base_url,
            auth_disabled,
            additional_allowed_hosts,
            allow_any_host,
            log,
        })
    }

    /// True when the resolved host is a non-loopback (network) bind.
    pub fn is_network_bind(&self) -> bool {
        let h = self.host.trim();
        !(h.is_empty() || h == "127.0.0.1" || h == "::1" || h == "localhost")
    }

    /// Enforce the no-unauthenticated-network-bind invariant at load time, so a
    /// misconfigured container refuses to start rather than exposing an
    /// unauthenticated gateway on the network.
    pub fn validate(&self) -> Result<()> {
        if self.auth_disabled && self.is_network_bind() {
            anyhow::bail!(
                "refusing to start: auth_disabled=true with a network bind ({}). \
                 Authentication is mandatory when the gateway is reachable off-host. \
                 Set auth_disabled=false or bind to 127.0.0.1.",
                self.host
            );
        }
        Ok(())
    }
}

/// Default data directory: `$MCPMUX_DATA_DIR` handled by the caller; here we
/// fall back to a `mcpmux` folder under the OS data dir, or `./mcpmux-data`.
fn default_data_dir() -> PathBuf {
    if let Some(dir) = dirs_data_dir() {
        dir.join("mcpmux")
    } else {
        PathBuf::from("./mcpmux-data")
    }
}

/// Minimal, dependency-free data-dir probe (avoids pulling in the `dirs` crate
/// just for the serve binary). Honors common env vars per platform.
fn dirs_data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(host: &str, auth_disabled: bool) -> Config {
        Config {
            data_dir: PathBuf::from("."),
            host: host.to_string(),
            port: 45818,
            public_base_url: None,
            auth_disabled,
            additional_allowed_hosts: vec![],
            allow_any_host: false,
            log: "info".to_string(),
        }
    }

    #[test]
    fn network_bind_is_detected() {
        for h in ["127.0.0.1", "::1", "localhost", ""] {
            assert!(!cfg(h, false).is_network_bind(), "{h} should be loopback");
        }
        for h in ["0.0.0.0", "::", "192.168.1.5"] {
            assert!(cfg(h, false).is_network_bind(), "{h} should be network");
        }
    }

    #[test]
    fn validate_rejects_unauthenticated_network_bind() {
        assert!(cfg("0.0.0.0", true).validate().is_err());
        // Auth on + network is fine.
        assert!(cfg("0.0.0.0", false).validate().is_ok());
        // Auth off on loopback is the allowed convenience.
        assert!(cfg("127.0.0.1", true).validate().is_ok());
    }

    #[test]
    fn file_config_parses_toml() {
        let toml = r#"
            host = "0.0.0.0"
            port = 45999
            auth_disabled = false
            additional_allowed_hosts = ["mybox.local", "mcp.home"]
        "#;
        let fc: FileConfig = toml::from_str(toml).unwrap();
        assert_eq!(fc.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(fc.port, Some(45999));
        assert_eq!(fc.additional_allowed_hosts.unwrap().len(), 2);
    }
}
