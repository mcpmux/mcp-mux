//! CLI args for `mcpmuxd`. Subcommands land in Phase 3; this is the
//! minimal `serve`-style invocation.
//!
//! Every flag matches the roadmap's "CLI contract (MVP)" with the
//! exception of `serve` itself being implicit (today the binary IS the
//! serve command).

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "mcpmuxd",
    about = "McpMux headless daemon — long-running Streamable HTTP gateway",
    version
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Persistent data directory. Overrides `$XDG_STATE_HOME/mcpmux`.
    #[arg(long, env = "MCPMUX_DATA_DIR", value_name = "PATH", global = true)]
    pub data_dir: Option<PathBuf>,

    /// Port the gateway binds to. Defaults to the gateway's built-in
    /// default (45818). The desktop already persists overrides here; the
    /// daemon will load any persisted override on first start.
    #[arg(long, global = true)]
    pub port: Option<u16>,

    /// Registry API URL used by `ServerDiscoveryService`. Overrides
    /// `MCPMUX_REGISTRY_URL`. Defaults to `https://api.mcpmux.com`.
    #[arg(long, env = "MCPMUX_REGISTRY_URL", value_name = "URL", global = true)]
    pub registry_url: Option<String>,

    /// Master-key provider policy. `auto` matches the desktop default
    /// (DPAPI / OS keychain with file fallback). `keychain` fails clearly
    /// when no Secret Service is available. `file` always uses
    /// `<data_dir>/keys/master.key` (0600).
    #[arg(long, value_enum, default_value_t = KeyProviderArg::Auto, global = true)]
    pub key_provider: KeyProviderArg,

    /// Optional override for the server-log directory. When unset, logs
    /// go to stdout/stderr (the systemd-journal default). Setting this
    /// switches the daemon to daily-rotated files in the given directory.
    #[arg(long, value_name = "PATH", global = true)]
    pub log_dir: Option<PathBuf>,

    /// `RUST_LOG`-style tracing filter, e.g.
    /// `info,mcpmux_gateway=debug` while troubleshooting. Defaults to `info`:
    /// debug output includes request details that don't belong in a
    /// long-lived log.
    #[arg(long, env = "RUST_LOG", default_value = "info", global = true)]
    pub log_filter: String,

    /// Public base URL advertised in OAuth / MCP metadata (e.g.
    /// `https://mcp.example.com` when fronted by a Cloudflare Tunnel).
    /// Defaults to `http://localhost:<port>` when unset.
    #[arg(long, value_name = "URL", global = true)]
    pub public_base_url: Option<String>,

    /// Disable inbound MCP auth for this run only (loopback convenience).
    /// Not persisted: every start without the flag enforces auth.
    #[arg(long, global = true)]
    pub auth_disabled: bool,
}

/// CLI-side mirror of `mcpmux_runtime::KeyProviderPolicy`.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum KeyProviderArg {
    Auto,
    Keychain,
    File,
}

#[cfg(unix)]
impl KeyProviderArg {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Keychain => "keychain",
            Self::File => "file",
        }
    }
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Manage the Linux systemd user service.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum ServiceCommand {
    /// Install, enable, and start the mcpmux.service for the current user.
    Install,
}
