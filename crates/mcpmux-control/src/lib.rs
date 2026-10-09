//! Versioned local control protocol for the McpMux daemon (`mcpmuxd`) and the
//! operator CLI (`mcpmux`).
//!
//! The CLI never writes the SQLite database directly. Every operation is a
//! newline-delimited JSON request/response over a Unix domain socket owned by
//! the daemon. This crate holds only the wire types and framing helpers so the
//! daemon and the CLI cannot drift apart.
//!
//! Wire format: one JSON object per line, terminated by `\n`.
//!
//! ```json
//! {"version":1,"request_id":"...","method":"spaces.list","params":{}}
//! {"version":1,"request_id":"...","ok":true,"data":[...]}
//! {"version":1,"request_id":"...","ok":false,"error":{"code":"...","message":"..."}}
//! ```
//!
//! `events.subscribe` upgrades the connection to a one-way stream of
//! [`EventEnvelope`] lines (no `request_id` correlation after the ack).

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

/// Protocol version. Bump on any breaking wire change.
pub const PROTOCOL_VERSION: u32 = 1;

/// Hard cap on a single frame (request or response) to bound memory use.
pub const MAX_FRAME_LEN: usize = 8 * 1024 * 1024;

/// Stable machine-readable error codes returned in [`ErrorBody::code`].
pub mod codes {
    /// The request's declared protocol version is not supported.
    pub const INCOMPATIBLE_VERSION: &str = "incompatible_version";
    /// The `method` was unknown or not implemented.
    pub const UNKNOWN_METHOD: &str = "unknown_method";
    /// The `params` payload failed to deserialize or validate.
    pub const INVALID_PARAMS: &str = "invalid_params";
    /// A referenced entity (space, server, feature set, ...) does not exist.
    pub const NOT_FOUND: &str = "not_found";
    /// The operation conflicts with existing state (duplicate, in use, ...).
    pub const CONFLICT: &str = "conflict";
    /// The daemon failed to complete an otherwise valid request.
    pub const INTERNAL: &str = "internal";
    /// The frame was malformed or exceeded [`crate::MAX_FRAME_LEN`].
    pub const BAD_FRAME: &str = "bad_frame";
}

/// A single request line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    /// Protocol version the client speaks.
    pub version: u32,
    /// Client-chosen correlation id, echoed back verbatim.
    pub request_id: String,
    /// Dotted method name, e.g. `spaces.list`. See [`Method`].
    pub method: String,
    /// Method-specific parameters. Absent params are equivalent to `{}`.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A successful or failed response line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    /// Protocol version the daemon speaks (always the daemon's, not the
    /// client's — a version-mismatch error must still carry a parseable body).
    pub version: u32,
    /// Echo of [`RequestEnvelope::request_id`].
    pub request_id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl ResponseEnvelope {
    /// Build a success response carrying `data`.
    pub fn ok<T: Serialize>(request_id: impl Into<String>, data: &T) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            ok: true,
            data: serde_json::to_value(data).ok(),
            error: None,
        }
    }

    /// Build an error response carrying a stable [`codes`] value.
    pub fn error(
        request_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            ok: false,
            data: None,
            error: Some(ErrorBody {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

/// Machine-readable error payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

/// A server-pushed event line on an `events.subscribe` connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub version: u32,
    pub event: mcpmux_core::DomainEvent,
}

impl EventEnvelope {
    pub fn new(event: mcpmux_core::DomainEvent) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            event,
        }
    }
}

/// Recognised control methods. Unknown strings are rejected by the daemon with
/// [`codes::UNKNOWN_METHOD`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Ping,
    Status,
    Health,
    Doctor,
    SpacesList,
    SpacesGet,
    SpacesCreate,
    SpacesDelete,
    SpacesSetDefault,
    BaseDirsList,
    BaseDirsAdd,
    BaseDirsRemove,
    FeatureSetsList,
    FeatureSetsGet,
    FeatureSetsCreate,
    FeatureSetsUpdate,
    FeatureSetsDelete,
    FeatureSetsAddMember,
    FeatureSetsRemoveMember,
    ServersList,
    ServersInspect,
    ServersFeatures,
    RegistryList,
    RegistrySearch,
    ServersAdd,
    ServersConfigure,
    ServersEnable,
    ServersDisable,
    ServersRemove,
    ServersAuthenticate,
    WorkspacesList,
    WorkspacesBind,
    WorkspacesUnbind,
    ClientsList,
    ClientsCreate,
    ClientsDelete,
    LogsList,
    ConfigExport,
    ConfigExportSpace,
    ConfigImport,
    ConfigValidate,
    WorkspaceConfig,
    EventsSubscribe,
    PortGet,
    PortSet,
    PortClear,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ping => "ping",
            Self::Status => "status",
            Self::Health => "health",
            Self::Doctor => "doctor",
            Self::SpacesList => "spaces.list",
            Self::SpacesGet => "spaces.get",
            Self::SpacesCreate => "spaces.create",
            Self::SpacesDelete => "spaces.delete",
            Self::SpacesSetDefault => "spaces.set-default",
            Self::BaseDirsList => "base-dirs.list",
            Self::BaseDirsAdd => "base-dirs.add",
            Self::BaseDirsRemove => "base-dirs.remove",
            Self::FeatureSetsList => "feature-sets.list",
            Self::FeatureSetsGet => "feature-sets.get",
            Self::FeatureSetsCreate => "feature-sets.create",
            Self::FeatureSetsUpdate => "feature-sets.update",
            Self::FeatureSetsDelete => "feature-sets.delete",
            Self::FeatureSetsAddMember => "feature-sets.add-member",
            Self::FeatureSetsRemoveMember => "feature-sets.remove-member",
            Self::ServersList => "servers.list",
            Self::ServersInspect => "servers.inspect",
            Self::ServersFeatures => "servers.features",
            Self::RegistryList => "registry.list",
            Self::RegistrySearch => "registry.search",
            Self::ServersAdd => "servers.add",
            Self::ServersConfigure => "servers.configure",
            Self::ServersEnable => "servers.enable",
            Self::ServersDisable => "servers.disable",
            Self::ServersRemove => "servers.remove",
            Self::ServersAuthenticate => "servers.authenticate",
            Self::WorkspacesList => "workspaces.list",
            Self::WorkspacesBind => "workspaces.bind",
            Self::WorkspacesUnbind => "workspaces.unbind",
            Self::ClientsList => "clients.list",
            Self::ClientsCreate => "clients.create",
            Self::ClientsDelete => "clients.delete",
            Self::LogsList => "logs.list",
            Self::ConfigExport => "config.export",
            Self::ConfigExportSpace => "config.export-space",
            Self::ConfigImport => "config.import",
            Self::ConfigValidate => "config.validate",
            Self::WorkspaceConfig => "workspace.config",
            Self::EventsSubscribe => "events.subscribe",
            Self::PortGet => "port.get",
            Self::PortSet => "port.set",
            Self::PortClear => "port.clear",
        }
    }

    /// Parse a wire method name. Returns `None` for anything unrecognised.
    pub fn parse(value: &str) -> Option<Self> {
        let method = match value {
            "ping" => Self::Ping,
            "status" => Self::Status,
            "health" => Self::Health,
            "doctor" => Self::Doctor,
            "spaces.list" => Self::SpacesList,
            "spaces.get" => Self::SpacesGet,
            "spaces.create" => Self::SpacesCreate,
            "spaces.delete" => Self::SpacesDelete,
            "spaces.set-default" => Self::SpacesSetDefault,
            "base-dirs.list" => Self::BaseDirsList,
            "base-dirs.add" => Self::BaseDirsAdd,
            "base-dirs.remove" => Self::BaseDirsRemove,
            "feature-sets.list" => Self::FeatureSetsList,
            "feature-sets.get" => Self::FeatureSetsGet,
            "feature-sets.create" => Self::FeatureSetsCreate,
            "feature-sets.update" => Self::FeatureSetsUpdate,
            "feature-sets.delete" => Self::FeatureSetsDelete,
            "feature-sets.add-member" => Self::FeatureSetsAddMember,
            "feature-sets.remove-member" => Self::FeatureSetsRemoveMember,
            "servers.list" => Self::ServersList,
            "servers.inspect" => Self::ServersInspect,
            "servers.features" => Self::ServersFeatures,
            "registry.list" => Self::RegistryList,
            "registry.search" => Self::RegistrySearch,
            "servers.add" => Self::ServersAdd,
            "servers.configure" => Self::ServersConfigure,
            "servers.enable" => Self::ServersEnable,
            "servers.disable" => Self::ServersDisable,
            "servers.remove" => Self::ServersRemove,
            "servers.authenticate" => Self::ServersAuthenticate,
            "workspaces.list" => Self::WorkspacesList,
            "workspaces.bind" => Self::WorkspacesBind,
            "workspaces.unbind" => Self::WorkspacesUnbind,
            "clients.list" => Self::ClientsList,
            "clients.create" => Self::ClientsCreate,
            "clients.delete" => Self::ClientsDelete,
            "logs.list" => Self::LogsList,
            "config.export" => Self::ConfigExport,
            "config.export-space" => Self::ConfigExportSpace,
            "config.import" => Self::ConfigImport,
            "config.validate" => Self::ConfigValidate,
            "workspace.config" => Self::WorkspaceConfig,
            "events.subscribe" => Self::EventsSubscribe,
            "port.get" => Self::PortGet,
            "port.set" => Self::PortSet,
            "port.clear" => Self::PortClear,
            _ => return None,
        };
        Some(method)
    }
}

// =============================================================================
// Method parameters
// =============================================================================

/// `status` takes no parameters.
pub type StatusParams = Empty;

/// `health` takes no parameters.
pub type HealthParams = Empty;

/// `doctor` takes no parameters.
pub type DoctorParams = Empty;

/// An empty params object. Accepts `null`, `{}`, or absent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Empty {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacesGetParams {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacesCreateParams {
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacesDeleteParams {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacesSetDefaultParams {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseDirsListParams {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseDirsAddParams {
    pub space_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseDirsRemoveParams {
    pub id: String,
}

/// `feature-sets.list` filters by space when `space_id` is set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureSetsListParams {
    #[serde(default)]
    pub space_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSetsGetParams {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSetsCreateParams {
    pub space_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSetsUpdateParams {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSetsDeleteParams {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSetsAddMemberParams {
    /// Feature set to add the member to.
    pub feature_set_id: String,
    /// Server whose discovered features should be added.
    pub server_id: String,
    #[serde(default)]
    pub space_id: Option<String>,
    /// Restrict to one feature type (`tool`, `prompt`, `resource`); all when
    /// omitted.
    #[serde(default)]
    pub feature_type: Option<String>,
    /// Optional feature-name filter; when set, only matching features are added.
    #[serde(default)]
    pub name: Option<String>,
    /// `include` (default) or `exclude`.
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSetsRemoveMemberParams {
    pub feature_set_id: String,
    /// Feature row id or feature name to remove.
    pub feature_id: String,
    #[serde(default)]
    pub space_id: Option<String>,
    /// When true, `feature_id` is matched against `feature_name` rather than
    /// the feature row id. The server id must also be given in that case.
    #[serde(default)]
    pub by_name: bool,
    #[serde(default)]
    pub server_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServersListParams {
    #[serde(default)]
    pub space_id: Option<String>,
}

/// `registry.list` lists the catalog available to install. `query` and
/// `category` are optional filters; `refresh` forces a registry fetch instead
/// of using the 5-minute cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryListParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub refresh: bool,
}

/// `registry.search` is `registry.list` with a required `query`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySearchParams {
    pub query: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersInspectParams {
    pub server_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersFeaturesParams {
    pub server_id: String,
    #[serde(default)]
    pub space_id: Option<String>,
    /// Restrict to one feature type (`tool`, `prompt`, `resource`).
    #[serde(default)]
    pub feature_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAddParams {
    /// Registry (or user-space) definition id.
    pub server_id: String,
    /// Target Space. When omitted the default Space is used.
    #[serde(default)]
    pub space_id: Option<String>,
    /// Registry definition inputs, e.g. credentials required at install time.
    #[serde(default)]
    pub inputs: std::collections::HashMap<String, String>,
}

/// `servers.configure` updates an installed server. Any omitted field is left
/// unchanged. `inputs`, `env` and `headers` are merged key by key into the
/// stored values: a string sets a key, `null` removes it, and keys not
/// mentioned are kept. `args` replaces the whole list.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServersConfigureParams {
    /// Registry definition id (as used by `servers list`).
    pub server_id: String,
    #[serde(default)]
    pub space_id: Option<String>,
    #[serde(default)]
    pub inputs: Option<std::collections::HashMap<String, Option<String>>>,
    #[serde(default)]
    pub env: Option<std::collections::HashMap<String, Option<String>>>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub headers: Option<std::collections::HashMap<String, Option<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersTargetParams {
    pub server_id: String,
    #[serde(default)]
    pub space_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersRemoveParams {
    pub server_id: String,
    #[serde(default)]
    pub space_id: Option<String>,
}

/// `servers.authenticate` starts the outbound OAuth flow for an installed
/// server and returns the authorization URL for the operator to open.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAuthenticateParams {
    pub server_id: String,
    #[serde(default)]
    pub space_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspacesListParams {
    #[serde(default)]
    pub space_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacesBindParams {
    /// Absolute workspace root, or an arbitrary id key when
    /// `binding_type = "id"`.
    pub path: String,
    pub space_id: String,
    #[serde(default)]
    pub feature_set_ids: Vec<String>,
    /// `path` (default) or `id`.
    #[serde(default)]
    pub binding_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacesUnbindParams {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientsListParams {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientsCreateParams {
    pub name: String,
    pub client_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientsDeleteParams {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsListParams {
    pub server_id: String,
    #[serde(default)]
    pub space_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigExportParams {
    /// One of `cursor`, `vscode`, `claude`.
    pub format: String,
    pub server_id: String,
    #[serde(default)]
    pub space_id: Option<String>,
    /// Resolve `${input:…}` values and include env/header/argument
    /// overrides verbatim. Off by default: the export then keeps the
    /// placeholders and shows overrides as `<redacted>`.
    #[serde(default)]
    pub include_secrets: bool,
}

/// `port.set` persists a custom gateway port. The daemon must be restarted for
/// the change to take effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortSetParams {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValidateParams {
    /// Absolute path to a JSON file to validate.
    pub file: String,
}

/// `config.export-space` exports every server installed in a Space as a
/// portable `mcpServers` JSON document (transport only — no credentials).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigExportSpaceParams {
    #[serde(default)]
    pub space_id: Option<String>,
}

/// `config.import` loads an `mcpServers` JSON document into a Space. The write
/// is applied through the core user-space sync service, which performs a 3-way
/// diff and auto-enables new servers. A backup of the space file is written
/// first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigImportParams {
    /// Absolute path to the JSON file to import.
    pub file: String,
    #[serde(default)]
    pub space_id: Option<String>,
    /// Report what would change without writing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfigParams {
    /// Absolute workspace root path.
    pub path: String,
    /// Client to emit the snippet for: `cursor`, `vscode`, or `claude`.
    pub client: String,
}

/// `events.subscribe` takes no parameters.
pub type EventsSubscribeParams = Empty;

// =============================================================================
// Response payloads
// =============================================================================

/// `status` response: process-level daemon metadata. Contains no secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub pid: u32,
    pub version: String,
    pub data_dir: String,
    pub gateway_url: String,
    pub port: u16,
    pub auth_disabled: bool,
    pub connected_servers: usize,
    pub enabled_servers: usize,
}

/// `servers.add` / `servers.configure` identify the installed row by its
/// stable id plus the registry server id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledServerRef {
    pub id: String,
    pub space_id: String,
    pub server_id: String,
    pub enabled: bool,
}

/// `servers.authenticate` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUrlResponse {
    pub server_id: String,
    pub space_id: String,
    pub authorization_url: String,
}

/// `port.get` / `port.set` / `port.clear` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortResponse {
    /// Port persisted via `port.set`. `None` means the daemon falls back to
    /// [`mcpmux_core::DEFAULT_GATEWAY_PORT`] (45818).
    pub persisted_port: Option<u16>,
    /// Port the gateway is currently bound to in this running daemon. Equals
    /// `persisted_port` when set, otherwise the default.
    pub active_port: u16,
    /// Built-in default (always `mcpmux_core::DEFAULT_GATEWAY_PORT`).
    pub default_port: u16,
}

/// Severity of one `doctor` check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// Healthy.
    Ok,
    /// Non-fatal problem the operator should review.
    Warn,
    /// A condition that breaks operation.
    Fail,
    /// Not applicable in this deployment.
    Skip,
}

/// One diagnostic check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    /// Stable check id, e.g. `data_dir_lock`.
    pub id: String,
    pub status: CheckStatus,
    /// Human-readable result.
    pub message: String,
    /// Optional remediation hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// `doctor` response: the checks plus an overall verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
    /// `true` when no check is `Fail`.
    pub healthy: bool,
}

/// `config.export` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigExportResponse {
    pub format: String,
    pub server_id: String,
    pub content: String,
}

/// `config.validate` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValidateResponse {
    pub valid: bool,
    pub servers: Vec<String>,
    pub warnings: Vec<String>,
}

/// `config.export-space` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigExportSpaceResponse {
    pub space_id: String,
    /// Pretty-printed `{"mcpServers": {...}}` document.
    pub content: String,
    /// Number of servers included.
    pub server_count: usize,
}

/// `config.import` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigImportResponse {
    pub space_id: String,
    pub dry_run: bool,
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
    /// Backup path written before a non-dry-run import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    /// What each server in the document runs: its command line (stdio) or
    /// URL (HTTP), keyed by the document's server key. Shown before an
    /// import is confirmed.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub launches: std::collections::BTreeMap<String, String>,
    /// Env and header values stored encrypted as server inputs instead of
    /// in the Space file: input ids keyed by the document's server key.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub stored_as_inputs: std::collections::BTreeMap<String, Vec<String>>,
}

/// `workspace.config` response. The snippet includes the
/// `X-Mcpmux-Workspace` header when one is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfigResponse {
    pub path: String,
    pub client: String,
    pub content: String,
}

// =============================================================================
// Framing
// =============================================================================

/// Framing failure while reading or writing a control frame.
#[derive(Debug, Error)]
pub enum FrameError {
    #[error("control connection closed")]
    Closed,
    #[error("control frame exceeded {MAX_FRAME_LEN} bytes")]
    TooLarge,
    #[error("control frame was not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("control I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Serialize `value` as one newline-terminated JSON frame.
pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
    T: Serialize,
{
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    tokio::io::AsyncWriteExt::write_all(writer, &bytes).await?;
    tokio::io::AsyncWriteExt::flush(writer).await?;
    Ok(())
}

/// Read exactly one newline-terminated JSON frame. Rejects frames larger than
/// [`MAX_FRAME_LEN`] before buffering beyond the cap.
pub async fn read_frame<R, T>(reader: &mut R) -> Result<T, FrameError>
where
    R: tokio::io::AsyncBufRead + Unpin,
    T: DeserializeOwned,
{
    let mut buf = Vec::new();
    loop {
        let available = tokio::io::AsyncBufReadExt::fill_buf(reader).await?;
        if available.is_empty() {
            return Err(FrameError::Closed);
        }
        if let Some(pos) = available.iter().position(|b| *b == b'\n') {
            if buf.len() + pos > MAX_FRAME_LEN {
                return Err(FrameError::TooLarge);
            }
            buf.extend_from_slice(&available[..pos]);
            tokio::io::AsyncBufReadExt::consume(reader, pos + 1);
            break;
        }
        if buf.len() + available.len() > MAX_FRAME_LEN {
            return Err(FrameError::TooLarge);
        }
        let len = available.len();
        buf.extend_from_slice(available);
        tokio::io::AsyncBufReadExt::consume(reader, len);
    }

    Ok(serde_json::from_slice(&buf)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_round_trips_through_wire_name() {
        for method in [
            Method::Ping,
            Method::Status,
            Method::SpacesList,
            Method::ServersAdd,
            Method::WorkspacesBind,
            Method::EventsSubscribe,
        ] {
            assert_eq!(Method::parse(method.as_str()), Some(method));
        }
        assert_eq!(Method::parse("nope.nope"), None);
    }

    #[test]
    fn error_response_is_not_ok() {
        let response = ResponseEnvelope::error("r1", codes::NOT_FOUND, "missing");
        assert!(!response.ok);
        assert!(response.data.is_none());
        assert_eq!(response.error.unwrap().code, codes::NOT_FOUND);
    }

    #[tokio::test]
    async fn frames_round_trip() {
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            request_id: "abc".to_string(),
            method: "ping".to_string(),
            params: serde_json::json!({}),
        };
        let mut wire = Vec::new();
        write_frame(&mut wire, &request).await.unwrap();

        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(wire));
        let decoded: RequestEnvelope = read_frame(&mut reader).await.unwrap();
        assert_eq!(decoded.request_id, "abc");
        assert_eq!(decoded.method, "ping");
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected() {
        let mut wire = vec![b'a'; MAX_FRAME_LEN + 10];
        wire.push(b'\n');
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(wire));
        let result: Result<serde_json::Value, _> = read_frame(&mut reader).await;
        assert!(matches!(result, Err(FrameError::TooLarge)));
    }
}
