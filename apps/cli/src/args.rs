//! `mcpmux` — operator CLI for a running McpMux daemon.
//!
//! The CLI never touches the database. Every command is a request over the
//! daemon's local Unix control socket; if the daemon is not running, the CLI
//! fails with an actionable message rather than opening SQLite itself.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Output mode. `human` is the default; `json` is stable for scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputMode {
    Human,
    Json,
}

#[derive(Debug, Clone, Parser)]
#[command(
    name = "mcpmux-cli",
    about = "McpMux operator CLI (talks to a running mcpmuxd)",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Persistent data directory. Must match the daemon's `--data-dir`.
    #[arg(long, env = "MCPMUX_DATA_DIR", value_name = "PATH", global = true)]
    pub data_dir: Option<PathBuf>,

    /// Explicit control-socket path. Overrides the XDG-derived default.
    #[arg(long, value_name = "PATH", global = true)]
    pub socket: Option<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputMode::Human, global = true)]
    pub output: OutputMode,

    /// Skip the confirmation prompt for destructive commands.
    #[arg(long, global = true)]
    pub yes: bool,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Daemon lifecycle and status.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Probe the daemon's health endpoint.
    Health,
    /// Diagnose the daemon deployment (data dir, keys, db, registry, servers).
    Doctor,
    /// Read server logs.
    Logs {
        /// Server (registry) id whose logs to read.
        #[arg(long, value_name = "ID")]
        server: String,
        /// Space id. Defaults to the daemon's default space.
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
        /// Maximum number of lines.
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Minimum level: trace|debug|info|warn|error.
        #[arg(long)]
        level: Option<String>,
        /// Stream new events until interrupted (prints domain events).
        #[arg(long)]
        follow: bool,
    },

    /// Manage Spaces.
    Spaces {
        #[command(subcommand)]
        command: SpacesCommand,
    },
    /// Manage Feature Sets.
    #[command(name = "feature-sets")]
    FeatureSets {
        #[command(subcommand)]
        command: FeatureSetsCommand,
    },
    /// Manage installed MCP servers.
    Servers {
        #[command(subcommand)]
        command: ServersCommand,
    },
    /// Browse the server catalog available to install.
    Registry {
        #[command(subcommand)]
        command: RegistryCommand,
    },
    /// Manage workspace bindings.
    Workspaces {
        #[command(subcommand)]
        command: WorkspacesCommand,
    },
    /// Manage inbound MCP clients.
    Clients {
        #[command(subcommand)]
        command: ClientsCommand,
    },
    /// Export and validate MCP client configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Print a per-workspace MCP client snippet.
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum DaemonCommand {
    /// Show daemon status (pid, version, gateway URL, connected servers).
    Status,
    /// Manage the persisted gateway port override.
    #[command(subcommand)]
    Port(PortCommand),
    /// Restart the daemon by SIGTERM-ing the live process and re-execing it
    /// with the same arguments. Fails if the daemon is not supervised by this
    /// CLI (e.g. when running under systemd, use `systemctl --user restart
    /// mcpmux.service` instead).
    Restart,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PortCommand {
    /// Print the persisted gateway port and the port currently bound.
    Get,
    /// Persist a custom gateway port. Restart the daemon for it to take effect.
    Set {
        /// Port number to persist.
        #[arg(value_name = "PORT")]
        port: u16,
    },
    /// Clear the persisted override and revert to the built-in default.
    Clear,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SpacesCommand {
    /// List Spaces.
    List,
    /// Create a Space.
    Create {
        name: String,
        #[arg(long)]
        icon: Option<String>,
    },
    /// Delete a Space.
    Delete { space_id: String },
    /// Make a Space the default.
    #[command(name = "set-default")]
    SetDefault { space_id: String },
    /// Manage a Space's base directories.
    #[command(name = "base-dirs")]
    BaseDirs {
        #[command(subcommand)]
        command: BaseDirsCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum BaseDirsCommand {
    /// List a Space's base directories.
    List { space_id: String },
    /// Claim a base directory for a Space.
    Add { space_id: String, path: String },
    /// Remove a base directory by row id.
    Remove { id: String },
}

#[derive(Debug, Clone, Subcommand)]
pub enum FeatureSetsCommand {
    /// List Feature Sets (optionally for one Space).
    List {
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
    /// Show one Feature Set with its members.
    Get { id: String },
    /// Create a Feature Set in a Space.
    Create {
        name: String,
        #[arg(long, value_name = "SPACE")]
        space: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        icon: Option<String>,
    },
    /// Update a Feature Set's metadata.
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        icon: Option<String>,
    },
    /// Delete a Feature Set.
    Delete { id: String },
    /// Include (or exclude) a server's discovered tools in a Feature Set.
    ///
    /// This is what makes an installed server's tools visible to clients
    /// resolving to that set's Space.
    Include {
        /// Feature Set id.
        id: String,
        /// Registry server id whose features to include.
        #[arg(long, value_name = "ID")]
        server: String,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
        /// Only one type: tool|prompt|resource (default: all).
        #[arg(long = "type")]
        feature_type: Option<String>,
        /// Only features whose name contains this substring.
        #[arg(long)]
        name: Option<String>,
    },
    /// Remove a feature from a Feature Set by row id, or by name with
    /// `--server`.
    Remove {
        id: String,
        /// Feature row id, or feature name when `--server` is given.
        feature: String,
        #[arg(long, value_name = "ID")]
        server: Option<String>,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum RegistryCommand {
    /// List catalog servers (optionally filtered).
    List {
        /// Free-text filter over id/name/description.
        #[arg(long)]
        query: Option<String>,
        /// Only servers in this category.
        #[arg(long)]
        category: Option<String>,
        /// Force a registry refresh instead of using the 5-minute cache.
        #[arg(long)]
        refresh: bool,
    },
    /// Search the catalog (alias of `list --query`).
    Search {
        query: String,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        refresh: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum ServersCommand {
    /// List installed servers.
    List {
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
    /// Show one server's installation details (values redacted).
    Inspect { server_id: String },
    /// List a server's discovered tools, prompts, and resources.
    Features {
        server_id: String,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
        /// Only one type: tool|prompt|resource.
        #[arg(long = "type")]
        feature_type: Option<String>,
    },
    /// Install a server from the registry into a Space.
    Add {
        server_id: String,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
    /// Update an installed server's inputs, env, args, or headers.
    Configure {
        server_id: String,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
        /// JSON file with `{ "inputs": {...}, "env": {...}, "args": [...], "headers": {...} }`.
        #[arg(long, value_name = "FILE")]
        file: PathBuf,
    },
    /// Enable a server and connect it.
    Enable {
        server_id: String,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
    /// Disable a server and disconnect it.
    Disable {
        server_id: String,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
    /// Uninstall a server.
    Remove {
        server_id: String,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
    /// Start the outbound OAuth flow and print the authorization URL.
    Auth {
        server_id: String,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum WorkspacesCommand {
    /// List workspace bindings.
    List {
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
    /// Bind a workspace root (or id key) to a Space and Feature Sets.
    Bind {
        /// Absolute workspace root path, or an id key with `--type id`.
        path: String,
        #[arg(long, value_name = "SPACE")]
        space: String,
        /// Feature Set id; repeatable.
        #[arg(long = "feature-set", value_name = "ID")]
        feature_sets: Vec<String>,
        /// `path` (default) or `id`.
        #[arg(long = "type", default_value = "path")]
        binding_type: String,
    },
    /// Remove a workspace binding by id.
    Unbind { id: String },
}

#[derive(Debug, Clone, Subcommand)]
pub enum ClientsCommand {
    /// List inbound clients.
    List,
    /// Register an inbound client and print its access key once.
    Create {
        name: String,
        #[arg(long = "type", default_value = "custom")]
        client_type: String,
    },
    /// Delete a client.
    Delete { id: String },
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    /// Export the gateway's tool list for one server in a client format.
    Export {
        /// cursor|vscode|claude
        #[arg(long, value_name = "FORMAT")]
        format: String,
        #[arg(long, value_name = "ID")]
        server: String,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
    },
    /// Export every server in a Space as a portable mcpServers document.
    #[command(name = "export-space")]
    ExportSpace {
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
        /// Write to this file instead of stdout.
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Import an mcpServers document into a Space.
    Import {
        #[arg(value_name = "FILE")]
        file: PathBuf,
        #[arg(long, value_name = "SPACE")]
        space: Option<String>,
        /// Report what would change without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Validate a JSON MCP client config file.
    Validate {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum WorkspaceCommand {
    /// Print a project-local config snippet with the workspace header.
    Config {
        #[arg(long, value_name = "PATH")]
        path: String,
        /// cursor|claude-code|vscode|opencode|zed
        #[arg(long, value_name = "CLIENT")]
        client: String,
    },
}
