//! Control-socket request handling.
//!
//! Every handler runs against the live daemon: mutations go through the same
//! repositories and application services the gateway uses, emit domain events,
//! and refresh in-memory gateway state where relevant. Nothing here writes the
//! SQLite database behind the daemon's back.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use mcpmux_control::{
    codes, AuthUrlResponse, BaseDirsAddParams, BaseDirsListParams, BaseDirsRemoveParams,
    ClientsCreateParams, ClientsDeleteParams, ClientsListParams, ConfigExportParams,
    ConfigExportResponse, ConfigExportSpaceParams, ConfigExportSpaceResponse, ConfigImportParams,
    ConfigImportResponse, ConfigValidateParams, ConfigValidateResponse, DaemonStatus,
    FeatureSetsAddMemberParams, FeatureSetsCreateParams, FeatureSetsDeleteParams,
    FeatureSetsGetParams, FeatureSetsListParams, FeatureSetsRemoveMemberParams,
    FeatureSetsUpdateParams, InstalledServerRef, LogsListParams, Method, PortResponse,
    PortSetParams, RegistryListParams, RegistrySearchParams, RequestEnvelope, ServersAddParams,
    ServersAuthenticateParams, ServersConfigureParams, ServersFeaturesParams, ServersInspectParams,
    ServersListParams, ServersRemoveParams, ServersTargetParams, SpacesCreateParams,
    SpacesDeleteParams, SpacesGetParams, SpacesSetDefaultParams, WorkspaceConfigParams,
    WorkspaceConfigResponse, WorkspacesBindParams, WorkspacesListParams, WorkspacesUnbindParams,
};
use mcpmux_core::{
    ConfigExporter, ConfigFormat, DomainEvent, EventSender, FeatureType, LogLevel, MemberMode,
    PermissionAppService, ResolvedServer, ResolvedTransport, ServerAppService, ServerDefinition,
    TransportConfig, WorkspaceBinding,
};
use mcpmux_gateway::pool::transport::resolution::build_transport_config;
use mcpmux_gateway::{
    ConnectionContext, ConnectionResult, ConnectionStatus, FeatureService, GatewayState,
    PoolService, ServerKey, ServerManager,
};
use mcpmux_runtime::Runtime;
use mcpmux_storage::{InboundClient, InboundClientRepository, RegistrationType};
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::warn;
use uuid::Uuid;

/// Error with a stable wire code.
#[derive(Debug)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: codes::NOT_FOUND,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: codes::INVALID_PARAMS,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            code: codes::CONFLICT,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: codes::INTERNAL,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(error.to_string())
    }
}

/// Everything a handler needs from the running daemon.
pub struct ControlState {
    pub runtime: Arc<Runtime>,
    /// The live gateway's state (auth mode, sessions).
    pub gateway_state: Arc<tokio::sync::RwLock<GatewayState>>,
    /// Sender for the gateway's domain-event channel. Every mutation emits
    /// here so connected MCP sessions see `list_changed`; the event bridge
    /// then forwards each event once to `events.subscribe` listeners.
    pub gateway_events: EventSender,
    pub pool_service: Arc<PoolService>,
    pub feature_service: Arc<FeatureService>,
    pub server_manager: Arc<ServerManager>,
    pub pid: u32,
    pub version: &'static str,
    /// Locally bound gateway origin, e.g. `http://127.0.0.1:45818`.
    pub gateway_origin: String,
    pub port: u16,
}

impl ControlState {
    fn server_app_service(&self) -> ServerAppService {
        ServerAppService::new(
            self.runtime.repositories.installed_server.clone(),
            Some(self.runtime.repositories.server_feature_core.clone()),
            Some(self.runtime.repositories.credential.clone()),
            self.gateway_events.clone(),
        )
        .with_outbound_oauth_repo(self.runtime.repositories.backend_oauth.clone())
    }

    fn permission_app_service(&self) -> PermissionAppService {
        PermissionAppService::new(
            self.runtime.repositories.feature_set.clone(),
            self.gateway_events.clone(),
        )
    }

    /// The unified inbound-client repository (storage). The gateway validates
    /// API keys against this repository's `inbound_client_api_keys` table, so
    /// CLI-registered clients MUST be created here — not through the legacy
    /// `ClientAppService`, whose `access_key` the gateway never checks.
    fn inbound_client_repo(&self) -> InboundClientRepository {
        InboundClientRepository::new(self.runtime.database.clone())
    }

    /// Emit on the gateway's channel (not the runtime bus) so MCPNotifier
    /// reacts; see [`Self::gateway_events`].
    fn emit(&self, event: DomainEvent) {
        self.gateway_events.emit(event);
    }
}

fn params<T: DeserializeOwned>(request: &RequestEnvelope) -> Result<T, ApiError> {
    let value = if request.params.is_null() {
        serde_json::json!({})
    } else {
        request.params.clone()
    };
    serde_json::from_value(value).map_err(|e| ApiError::invalid(e.to_string()))
}

/// Resolve the target Space: an explicit id (validated to exist) or the
/// daemon's default Space.
async fn resolve_space(state: &ControlState, space_id: Option<&str>) -> Result<Uuid, ApiError> {
    match space_id {
        Some(raw) => {
            let id =
                Uuid::parse_str(raw).map_err(|e| ApiError::invalid(format!("space_id: {e}")))?;
            match state.runtime.repositories.space.get(&id).await? {
                Some(_) => Ok(id),
                None => Err(ApiError::not_found(format!("space {raw} not found"))),
            }
        }
        None => state
            .runtime
            .repositories
            .space
            .get_default()
            .await?
            .map(|space| space.id)
            .ok_or_else(|| ApiError::not_found("no default space")),
    }
}

/// The Space a FeatureSet's members live in: its own `space_id`. An explicit
/// `--space` must agree with it; only a FeatureSet without a Space falls back
/// to `--space` / the default Space.
async fn feature_set_space(
    state: &ControlState,
    feature_set: &mcpmux_core::FeatureSet,
    requested: Option<&str>,
) -> Result<Uuid, ApiError> {
    let Some(own) = feature_set.space_id.as_deref() else {
        return resolve_space(state, requested).await;
    };
    let own = Uuid::parse_str(own).map_err(|e| {
        ApiError::internal(format!(
            "feature set {} has a bad space id: {e}",
            feature_set.id
        ))
    })?;
    if let Some(raw) = requested {
        let requested =
            Uuid::parse_str(raw).map_err(|e| ApiError::invalid(format!("space_id: {e}")))?;
        if requested != own {
            return Err(ApiError::invalid(format!(
                "feature set {} belongs to space {own}, not {requested}",
                feature_set.id
            )));
        }
    }
    Ok(own)
}

async fn find_installed(
    state: &ControlState,
    space: Uuid,
    server_id: &str,
) -> Result<mcpmux_core::InstalledServer, ApiError> {
    state
        .runtime
        .repositories
        .installed_server
        .get_by_server_id(&space.to_string(), server_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "server {server_id} is not installed in space {space}"
            ))
        })
}

/// Dispatch one request to its handler. `events.subscribe` is special-cased by
/// the connection loop and never reaches here.
pub async fn dispatch(
    state: &ControlState,
    request: &RequestEnvelope,
) -> Result<serde_json::Value, ApiError> {
    let method = Method::parse(&request.method).ok_or_else(|| ApiError {
        code: codes::UNKNOWN_METHOD,
        message: format!("unknown method '{}'", request.method),
    })?;

    match method {
        Method::Ping => Ok(json!({"pong": true})),
        Method::Status => status(state).await,
        Method::Health => health(state).await,
        Method::Doctor => doctor(state).await,
        Method::SpacesList => spaces_list(state).await,
        Method::SpacesGet => spaces_get(state, params(request)?).await,
        Method::SpacesCreate => spaces_create(state, params(request)?).await,
        Method::SpacesDelete => spaces_delete(state, params(request)?).await,
        Method::SpacesSetDefault => spaces_set_default(state, params(request)?).await,
        Method::BaseDirsList => base_dirs_list(state, params(request)?).await,
        Method::BaseDirsAdd => base_dirs_add(state, params(request)?).await,
        Method::BaseDirsRemove => base_dirs_remove(state, params(request)?).await,
        Method::FeatureSetsList => feature_sets_list(state, params(request)?).await,
        Method::FeatureSetsGet => feature_sets_get(state, params(request)?).await,
        Method::FeatureSetsCreate => feature_sets_create(state, params(request)?).await,
        Method::FeatureSetsUpdate => feature_sets_update(state, params(request)?).await,
        Method::FeatureSetsDelete => feature_sets_delete(state, params(request)?).await,
        Method::FeatureSetsAddMember => feature_sets_add_member(state, params(request)?).await,
        Method::FeatureSetsRemoveMember => {
            feature_sets_remove_member(state, params(request)?).await
        }
        Method::ServersList => servers_list(state, params(request)?).await,
        Method::ServersInspect => servers_inspect(state, params(request)?).await,
        Method::ServersFeatures => servers_features(state, params(request)?).await,
        Method::RegistryList => registry_list(state, params(request)?).await,
        Method::RegistrySearch => registry_search(state, params(request)?).await,
        Method::ServersAdd => servers_add(state, params(request)?).await,
        Method::ServersConfigure => servers_configure(state, params(request)?).await,
        Method::ServersEnable => servers_enable(state, params(request)?).await,
        Method::ServersDisable => servers_disable(state, params(request)?).await,
        Method::ServersRemove => servers_remove(state, params(request)?).await,
        Method::ServersAuthenticate => servers_authenticate(state, params(request)?).await,
        Method::WorkspacesList => workspaces_list(state, params(request)?).await,
        Method::WorkspacesBind => workspaces_bind(state, params(request)?).await,
        Method::WorkspacesUnbind => workspaces_unbind(state, params(request)?).await,
        Method::ClientsList => clients_list(state, params(request)?).await,
        Method::ClientsCreate => clients_create(state, params(request)?).await,
        Method::ClientsDelete => clients_delete(state, params(request)?).await,
        Method::LogsList => logs_list(state, params(request)?).await,
        Method::ConfigExport => config_export(state, params(request)?).await,
        Method::ConfigExportSpace => config_export_space(state, params(request)?).await,
        Method::ConfigImport => config_import(state, params(request)?).await,
        Method::ConfigValidate => config_validate(params(request)?).await,
        Method::WorkspaceConfig => workspace_config(state, params(request)?).await,
        Method::PortGet => port_get(state).await,
        Method::PortSet => port_set(state, params(request)?).await,
        Method::PortClear => port_clear(state).await,
        Method::EventsSubscribe => Err(ApiError::internal(
            "events.subscribe must be handled by the connection loop",
        )),
    }
}

// =============================================================================
// Status / health
// =============================================================================

async fn status(state: &ControlState) -> Result<serde_json::Value, ApiError> {
    let installed = state.runtime.repositories.installed_server.list().await?;
    let enabled = installed.iter().filter(|s| s.enabled).count();
    let connected = state.server_manager.connected_count().await;
    // Report what the running gateway enforces, not a settings row.
    let auth_disabled = state.gateway_state.read().await.auth_disabled();

    let status = DaemonStatus {
        pid: state.pid,
        version: state.version.to_string(),
        data_dir: state.runtime.data_dir.to_string_lossy().to_string(),
        gateway_url: state.gateway_origin.clone(),
        port: state.port,
        auth_disabled,
        connected_servers: connected,
        enabled_servers: enabled,
    };
    Ok(serde_json::to_value(status).expect("DaemonStatus is serializable"))
}

async fn health(state: &ControlState) -> Result<serde_json::Value, ApiError> {
    Ok(json!({
        "status": "ok",
        "version": state.version,
        "pid": state.pid,
    }))
}

// =============================================================================
// Doctor
// =============================================================================

/// Diagnose the running daemon's deployment: data dir, key material, database,
/// listener, registry reachability, and whether configured stdio server
/// executables resolve on this host.
async fn doctor(state: &ControlState) -> Result<serde_json::Value, ApiError> {
    use mcpmux_control::{CheckStatus, DoctorCheck, DoctorReport};

    let mut checks: Vec<DoctorCheck> = Vec::new();

    // Data directory.
    checks.push(if state.runtime.data_dir.is_dir() {
        DoctorCheck {
            id: "data_dir".into(),
            status: CheckStatus::Ok,
            message: format!(
                "data directory present: {}",
                state.runtime.data_dir.display()
            ),
            hint: None,
        }
    } else {
        DoctorCheck {
            id: "data_dir".into(),
            status: CheckStatus::Fail,
            message: format!(
                "data directory missing: {}",
                state.runtime.data_dir.display()
            ),
            hint: Some("re-run mcpmuxd with a valid --data-dir".into()),
        }
    });

    // Exclusive lock is held by this process for as long as it runs.
    checks.push(DoctorCheck {
        id: "data_dir_lock".into(),
        status: CheckStatus::Ok,
        message: format!(
            "exclusive lock held at {}",
            state.runtime.lock.path().display()
        ),
        hint: None,
    });

    // Key material: file provider only — check permissions where applicable.
    checks.push(check_key_files(&state.runtime.keys_dir));

    // Database.
    checks.push(if state.runtime.db_path.is_file() {
        DoctorCheck {
            id: "database".into(),
            status: CheckStatus::Ok,
            message: format!("database present: {}", state.runtime.db_path.display()),
            hint: None,
        }
    } else {
        DoctorCheck {
            id: "database".into(),
            status: CheckStatus::Fail,
            message: format!("database missing: {}", state.runtime.db_path.display()),
            hint: Some("the daemon creates it on first start; check data-dir permissions".into()),
        }
    });

    // Listener: the gateway is up by the time the control socket exists.
    checks.push(DoctorCheck {
        id: "gateway_listener".into(),
        status: CheckStatus::Ok,
        message: format!("gateway listening on {}", state.gateway_origin),
        hint: None,
    });

    // Registry reachability (best-effort, no network failure is fatal).
    let offline = state.runtime.server_discovery.is_offline().await;
    checks.push(DoctorCheck {
        id: "registry".into(),
        status: if offline {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        message: if offline {
            "registry unreachable; using cached definitions".to_string()
        } else {
            "registry reachable".to_string()
        },
        hint: if offline {
            Some("check network access or --registry-url; cached servers still work".into())
        } else {
            None
        },
    });

    // Configured stdio servers: does each command resolve on PATH?
    let installed = state.runtime.repositories.installed_server.list().await?;
    let mut missing: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for server in &installed {
        if let Some(definition) = server.get_definition() {
            if let mcpmux_core::TransportConfig::Stdio { command, .. } = &definition.transport {
                checked += 1;
                if !command_resolves(command) {
                    missing.push(format!("{} ({command})", server.server_id));
                }
            }
        }
    }
    checks.push(if missing.is_empty() {
        DoctorCheck {
            id: "server_executables".into(),
            status: CheckStatus::Ok,
            message: format!("{checked} stdio server command(s) resolve on PATH"),
            hint: None,
        }
    } else {
        DoctorCheck {
            id: "server_executables".into(),
            status: CheckStatus::Fail,
            message: format!("unresolved server command(s): {}", missing.join(", ")),
            hint: Some(
                "install the missing runtime (npx/uvx/docker) or set PATH for the service".into(),
            ),
        }
    });

    let healthy = !checks.iter().any(|c| c.status == CheckStatus::Fail);
    let report = DoctorReport { checks, healthy };
    Ok(serde_json::to_value(report).expect("DoctorReport is serializable"))
}

/// Inspect the key directory and any key files present. A `file` provider that
/// has not created keys yet is `Skip`, not a failure.
fn check_key_files(keys_dir: &std::path::Path) -> mcpmux_control::DoctorCheck {
    use mcpmux_control::{CheckStatus, DoctorCheck};

    if !keys_dir.is_dir() {
        return DoctorCheck {
            id: "key_files".into(),
            status: CheckStatus::Skip,
            message: "no file-backed keys (OS keychain/DPAPI provider in use)".into(),
            hint: None,
        };
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut insecure: Vec<String> = Vec::new();
        for name in ["master.key", "jwt.key"] {
            let path = keys_dir.join(name);
            if let Ok(meta) = std::fs::metadata(&path) {
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    insecure.push(format!("{name} is {mode:o}"));
                }
            }
        }
        if insecure.is_empty() {
            DoctorCheck {
                id: "key_files".into(),
                status: CheckStatus::Ok,
                message: format!("file-backed keys in {} are owner-only", keys_dir.display()),
                hint: None,
            }
        } else {
            DoctorCheck {
                id: "key_files".into(),
                status: CheckStatus::Fail,
                message: format!("insecure key permissions: {}", insecure.join(", ")),
                hint: Some("run: chmod 600 <data-dir>/keys/*.key".into()),
            }
        }
    }

    #[cfg(not(unix))]
    {
        DoctorCheck {
            id: "key_files".into(),
            status: CheckStatus::Skip,
            message: "key permission audit is Unix-only".into(),
            hint: None,
        }
    }
}

/// Does `command` resolve to an executable on the login-shell PATH (falling
/// back to the process PATH)? Mirrors the gateway's stdio resolution enough to
/// answer "will this server start?" without spawning it.
fn command_resolves(command: &str) -> bool {
    use mcpmux_gateway::pool::transport::shell_env;

    let shell_path = shell_env::get_shell_path();
    match shell_path {
        Some(path) => which::which_in(command, Some(path), ".")
            .or_else(|_| which::which_in(format!("{command}.exe"), Some(path), "."))
            .is_ok(),
        None => which::which(command).is_ok(),
    }
}

// =============================================================================
// Spaces
// =============================================================================

async fn spaces_list(state: &ControlState) -> Result<serde_json::Value, ApiError> {
    let spaces = state.runtime.space_service.list().await?;
    Ok(serde_json::to_value(spaces).expect("Space is serializable"))
}

async fn spaces_get(
    state: &ControlState,
    p: SpacesGetParams,
) -> Result<serde_json::Value, ApiError> {
    let id = Uuid::parse_str(&p.space_id).map_err(|e| ApiError::invalid(e.to_string()))?;
    let space = state
        .runtime
        .space_service
        .get(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("space {} not found", p.space_id)))?;
    Ok(serde_json::to_value(space).expect("Space is serializable"))
}

async fn spaces_create(
    state: &ControlState,
    p: SpacesCreateParams,
) -> Result<serde_json::Value, ApiError> {
    let space = state.runtime.space_service.create(p.name, p.icon).await?;
    state.emit(DomainEvent::SpaceCreated {
        space_id: space.id,
        name: space.name.clone(),
        icon: space.icon.clone(),
    });
    Ok(serde_json::to_value(space).expect("Space is serializable"))
}

async fn spaces_delete(
    state: &ControlState,
    p: SpacesDeleteParams,
) -> Result<serde_json::Value, ApiError> {
    let id = Uuid::parse_str(&p.space_id).map_err(|e| ApiError::invalid(e.to_string()))?;
    state
        .runtime
        .space_service
        .delete(&id)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    state.emit(DomainEvent::SpaceDeleted { space_id: id });
    Ok(json!({"deleted": p.space_id}))
}

async fn spaces_set_default(
    state: &ControlState,
    p: SpacesSetDefaultParams,
) -> Result<serde_json::Value, ApiError> {
    let id = Uuid::parse_str(&p.space_id).map_err(|e| ApiError::invalid(e.to_string()))?;
    // `SpaceService` does not expose set_default; the repo trait does, and
    // this is exactly the operation the CLI needs (no desktop command exists).
    state
        .runtime
        .repositories
        .space
        .set_default(&id)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?;
    Ok(json!({"default_space_id": p.space_id}))
}

// =============================================================================
// Space base directories
// =============================================================================

async fn base_dirs_list(
    state: &ControlState,
    p: BaseDirsListParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, Some(&p.space_id)).await?;
    let dirs = state
        .runtime
        .repositories
        .space_base_dir
        .list_by_space(&space)
        .await?;
    Ok(serde_json::to_value(dirs).expect("SpaceBaseDir is serializable"))
}

async fn base_dirs_add(
    state: &ControlState,
    p: BaseDirsAddParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, Some(&p.space_id)).await?;
    let normalized = match mcpmux_core::validate_workspace_root(&p.path) {
        mcpmux_core::WorkspaceRootValidation::Ok { normalized } => normalized,
        mcpmux_core::WorkspaceRootValidation::Empty => {
            return Err(ApiError::invalid("path is empty"))
        }
        mcpmux_core::WorkspaceRootValidation::Invalid { reason } => {
            return Err(ApiError::invalid(reason))
        }
    };
    let dir = state
        .runtime
        .repositories
        .space_base_dir
        .add(&space, &normalized)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(serde_json::to_value(dir).expect("SpaceBaseDir is serializable"))
}

async fn base_dirs_remove(
    state: &ControlState,
    p: BaseDirsRemoveParams,
) -> Result<serde_json::Value, ApiError> {
    state
        .runtime
        .repositories
        .space_base_dir
        .remove(&p.id)
        .await?;
    Ok(json!({"removed": p.id}))
}

// =============================================================================
// Feature sets
// =============================================================================

async fn feature_sets_list(
    state: &ControlState,
    p: FeatureSetsListParams,
) -> Result<serde_json::Value, ApiError> {
    let sets = match p.space_id {
        Some(raw) => {
            let space = resolve_space(state, Some(&raw)).await?;
            state
                .runtime
                .repositories
                .feature_set
                .list_by_space(&space.to_string())
                .await?
        }
        None => state.runtime.repositories.feature_set.list().await?,
    };
    Ok(serde_json::to_value(sets).expect("FeatureSet is serializable"))
}

async fn feature_sets_get(
    state: &ControlState,
    p: FeatureSetsGetParams,
) -> Result<serde_json::Value, ApiError> {
    let set = state
        .runtime
        .repositories
        .feature_set
        .get_with_members(&p.id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("feature set {} not found", p.id)))?;
    Ok(serde_json::to_value(set).expect("FeatureSet is serializable"))
}

async fn feature_sets_create(
    state: &ControlState,
    p: FeatureSetsCreateParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, Some(&p.space_id)).await?;
    let set = state
        .permission_app_service()
        .create_feature_set(&space.to_string(), &p.name, p.description, p.icon)
        .await?;
    Ok(serde_json::to_value(set).expect("FeatureSet is serializable"))
}

async fn feature_sets_update(
    state: &ControlState,
    p: FeatureSetsUpdateParams,
) -> Result<serde_json::Value, ApiError> {
    let set = state
        .permission_app_service()
        .update_feature_set(&p.id, p.name, p.description, p.icon)
        .await?;
    Ok(serde_json::to_value(set).expect("FeatureSet is serializable"))
}

async fn feature_sets_delete(
    state: &ControlState,
    p: FeatureSetsDeleteParams,
) -> Result<serde_json::Value, ApiError> {
    state
        .permission_app_service()
        .delete_feature_set(&p.id)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(json!({"deleted": p.id}))
}

/// Add a server's discovered features to a FeatureSet. This is how an operator
/// grants tools to clients: a freshly-installed server's features are cached
/// but not visible until included in some FeatureSet a session resolves to.
async fn feature_sets_add_member(
    state: &ControlState,
    p: FeatureSetsAddMemberParams,
) -> Result<serde_json::Value, ApiError> {
    let feature_set = state
        .runtime
        .repositories
        .feature_set
        .get(&p.feature_set_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("feature set {} not found", p.feature_set_id))
        })?;
    let space = feature_set_space(state, &feature_set, p.space_id.as_deref()).await?;

    let feature_type = match p.feature_type.as_deref() {
        Some(raw) => Some(
            FeatureType::parse(raw)
                .ok_or_else(|| ApiError::invalid(format!("unknown feature type '{raw}'")))?,
        ),
        None => None,
    };
    let mode = match p.mode.as_deref() {
        None | Some("include") => MemberMode::Include,
        Some("exclude") => MemberMode::Exclude,
        Some(other) => {
            return Err(ApiError::invalid(format!(
                "mode must be 'include' or 'exclude', got '{other}'"
            )))
        }
    };

    let features = state
        .runtime
        .repositories
        .server_feature_core
        .list_for_server(&space.to_string(), &p.server_id)
        .await?;

    let mut added = 0;
    for feature in features {
        if let Some(ref want) = feature_type {
            if &feature.feature_type != want {
                continue;
            }
        }
        if let Some(ref needle) = p.name {
            if !feature.feature_name.contains(needle.as_str()) {
                continue;
            }
        }
        state
            .runtime
            .repositories
            .feature_set
            .add_feature_member(&feature_set.id, &feature.id.to_string(), mode)
            .await?;
        added += 1;
    }

    if added == 0 {
        return Err(ApiError::not_found(format!(
            "no discovered features for {} in space {} matched",
            p.server_id, space
        )));
    }

    state.emit(DomainEvent::FeatureSetMembersChanged {
        space_id: space,
        feature_set_id: feature_set.id.clone(),
        added_count: added,
        removed_count: 0,
    });

    Ok(json!({
        "feature_set_id": feature_set.id,
        "server_id": p.server_id,
        "added": added,
    }))
}

/// Remove feature members from a FeatureSet, either by feature row id or by
/// (server, feature name).
async fn feature_sets_remove_member(
    state: &ControlState,
    p: FeatureSetsRemoveMemberParams,
) -> Result<serde_json::Value, ApiError> {
    let feature_set = state
        .runtime
        .repositories
        .feature_set
        .get(&p.feature_set_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("feature set {} not found", p.feature_set_id))
        })?;
    let space = feature_set_space(state, &feature_set, p.space_id.as_deref()).await?;

    let feature_id = if p.by_name {
        let server_id = p
            .server_id
            .as_deref()
            .ok_or_else(|| ApiError::invalid("server_id is required with by_name"))?;
        let features = state
            .runtime
            .repositories
            .server_feature_core
            .list_for_server(&space.to_string(), server_id)
            .await?;
        features
            .into_iter()
            .find(|f| f.feature_name == p.feature_id)
            .map(|f| f.id.to_string())
            .ok_or_else(|| ApiError::not_found(format!("feature {} not found", p.feature_id)))?
    } else {
        p.feature_id.clone()
    };

    state
        .runtime
        .repositories
        .feature_set
        .remove_feature_member(&feature_set.id, &feature_id)
        .await?;

    state.emit(DomainEvent::FeatureSetMembersChanged {
        space_id: space,
        feature_set_id: feature_set.id.clone(),
        added_count: 0,
        removed_count: 1,
    });

    Ok(json!({
        "feature_set_id": feature_set.id,
        "removed": feature_id,
    }))
}

/// Resolve feature-set ids for a binding, rejecting unknown ones.
async fn validate_feature_set_ids(state: &ControlState, ids: &[String]) -> Result<(), ApiError> {
    for id in ids {
        if state
            .runtime
            .repositories
            .feature_set
            .get(id)
            .await?
            .is_none()
        {
            return Err(ApiError::not_found(format!("feature set {id} not found")));
        }
    }
    Ok(())
}

// =============================================================================
// Installed servers
// =============================================================================

async fn servers_list(
    state: &ControlState,
    p: ServersListParams,
) -> Result<serde_json::Value, ApiError> {
    let rows = match &p.space_id {
        Some(raw) => {
            let space = resolve_space(state, Some(raw)).await?;
            state
                .runtime
                .repositories
                .installed_server
                .list_for_space(&space.to_string())
                .await?
        }
        None => state.runtime.repositories.installed_server.list().await?,
    };

    let mut out = Vec::with_capacity(rows.len());
    for server in rows {
        let status = match Uuid::parse_str(&server.space_id) {
            Ok(space) => {
                let key = ServerKey::new(space, server.server_id.clone());
                state.server_manager.get_status(&key).await
            }
            Err(_) => None,
        };
        let (status, message) = match status {
            Some((status, _, _, message)) => (status, message),
            None => (ConnectionStatus::Disconnected, None),
        };
        out.push(json!({
            "id": server.id,
            "space_id": server.space_id,
            "server_id": server.server_id,
            "name": server.display_name(),
            "enabled": server.enabled,
            "oauth_connected": server.oauth_connected,
            "status": status,
            "message": message,
        }));
    }
    Ok(serde_json::Value::Array(out))
}

async fn servers_inspect(
    state: &ControlState,
    p: ServersInspectParams,
) -> Result<serde_json::Value, ApiError> {
    let all = state.runtime.repositories.installed_server.list().await?;
    let rows: Vec<_> = all
        .into_iter()
        .filter(|s| s.server_id == p.server_id)
        .collect();
    if rows.is_empty() {
        return Err(ApiError::not_found(format!(
            "no installation of {} in any space",
            p.server_id
        )));
    }

    let mut out = Vec::with_capacity(rows.len());
    for server in rows {
        let definition: Option<ServerDefinition> = server.get_definition();
        let (transport, inputs) = match &definition {
            Some(def) => {
                let transport = match &def.transport {
                    TransportConfig::Stdio { .. } => "stdio",
                    TransportConfig::Http { .. } => "http",
                };
                let inputs: Vec<&str> = def
                    .transport
                    .metadata()
                    .inputs
                    .iter()
                    .map(|i| i.id.as_str())
                    .collect();
                (transport, inputs)
            }
            None => ("unknown", Vec::new()),
        };
        // Never echo input/env/header *values* — they may hold secrets.
        out.push(json!({
            "id": server.id,
            "space_id": server.space_id,
            "server_id": server.server_id,
            "name": server.display_name(),
            "enabled": server.enabled,
            "oauth_connected": server.oauth_connected,
            "transport": transport,
            "declared_inputs": inputs,
            "configured_inputs": server.input_values.keys().collect::<Vec<_>>(),
            "env_overrides": server.env_overrides.keys().collect::<Vec<_>>(),
            "extra_headers": server.extra_headers.keys().collect::<Vec<_>>(),
            "args_append": server.args_append,
        }));
    }
    Ok(serde_json::Value::Array(out))
}

/// List the cached (discovered) features for a server in a Space.
async fn servers_features(
    state: &ControlState,
    p: ServersFeaturesParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let feature_type = match p.feature_type.as_deref() {
        Some(raw) => Some(
            FeatureType::parse(raw)
                .ok_or_else(|| ApiError::invalid(format!("unknown feature type '{raw}'")))?,
        ),
        None => None,
    };

    let features = state
        .runtime
        .repositories
        .server_feature_core
        .list_for_server(&space.to_string(), &p.server_id)
        .await?;

    let out: Vec<_> = features
        .into_iter()
        .filter(|f| feature_type.as_ref().is_none_or(|t| &f.feature_type == t))
        .map(|f| {
            json!({
                "id": f.id,
                "feature_type": f.feature_type,
                "feature_name": f.feature_name,
                "qualified_name": f.qualified_name(),
                "available": f.is_available,
            })
        })
        .collect();
    Ok(serde_json::Value::Array(out))
}

/// Refresh the catalog (honouring the `refresh` flag and the 5-minute cache)
/// and return a serialisable summary of each matching definition.
async fn registry_catalog(
    state: &ControlState,
    query: Option<&str>,
    category: Option<&str>,
    refresh: bool,
) -> Result<Vec<serde_json::Value>, ApiError> {
    if refresh {
        if let Err(e) = state.runtime.server_discovery.refresh().await {
            warn!(error = %e, "[control] registry refresh failed; using cache");
        }
    } else if let Err(e) = state.runtime.server_discovery.refresh_if_needed().await {
        warn!(error = %e, "[control] registry refresh-if-needed failed; using cache");
    }

    let mut servers = match query {
        Some(q) if !q.trim().is_empty() => state.runtime.server_discovery.search(q).await,
        _ => state.runtime.server_discovery.list().await,
    };

    if let Some(category) = category {
        let needle = category.to_lowercase();
        servers.retain(|s| {
            s.categories
                .iter()
                .any(|c| c.to_lowercase() == needle || c.to_lowercase().contains(&needle))
        });
    }

    // Mark which definitions already have an installation in any Space.
    let installed_ids: std::collections::HashSet<String> = state
        .runtime
        .repositories
        .installed_server
        .list()
        .await?
        .into_iter()
        .map(|s| s.server_id)
        .collect();

    Ok(servers
        .into_iter()
        .map(|s| {
            let transport = match &s.transport {
                TransportConfig::Stdio { .. } => "stdio",
                TransportConfig::Http { .. } => "http",
            };
            let auth = match &s.auth {
                None => "none",
                Some(mcpmux_core::domain::AuthConfig::None) => "none",
                Some(mcpmux_core::domain::AuthConfig::ApiKey { .. }) => "api_key",
                Some(mcpmux_core::domain::AuthConfig::OptionalApiKey { .. }) => "optional_api_key",
                Some(mcpmux_core::domain::AuthConfig::Basic { .. }) => "basic",
                Some(mcpmux_core::domain::AuthConfig::Oauth) => "oauth",
            };
            let source = match &s.source {
                mcpmux_core::ServerSource::UserSpace { .. } => "user_space",
                mcpmux_core::ServerSource::Bundled => "bundled",
                mcpmux_core::ServerSource::Registry { .. } => "registry",
            };
            json!({
                "id": s.id,
                "name": s.name,
                "alias": s.alias,
                "description": s.description,
                "transport": transport,
                "auth": auth,
                "categories": s.categories,
                "publisher": s.publisher.as_ref().map(|p| p.name.clone()),
                "source": source,
                "installed": installed_ids.contains(&s.id),
            })
        })
        .collect())
}

async fn registry_list(
    state: &ControlState,
    p: RegistryListParams,
) -> Result<serde_json::Value, ApiError> {
    let catalog =
        registry_catalog(state, p.query.as_deref(), p.category.as_deref(), p.refresh).await?;
    Ok(serde_json::Value::Array(catalog))
}

async fn registry_search(
    state: &ControlState,
    p: RegistrySearchParams,
) -> Result<serde_json::Value, ApiError> {
    let catalog = registry_catalog(state, Some(&p.query), p.category.as_deref(), p.refresh).await?;
    Ok(serde_json::Value::Array(catalog))
}

async fn servers_add(
    state: &ControlState,
    p: ServersAddParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;

    // Refresh from the registry, but fall back to the on-disk cache when the
    // host is offline — the definition may still be cached from a prior run.
    if let Err(e) = state.runtime.server_discovery.refresh_if_needed().await {
        warn!(error = %e, "[control] registry refresh failed; using cache");
    }
    let definition = state
        .runtime
        .server_discovery
        .get(&p.server_id)
        .await
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "server definition {} not found (registry offline and not cached?)",
                p.server_id
            ))
        })?;
    if !mcpmux_core::transport_matches_shown(&definition, p.expected_transport.as_ref()) {
        return Err(ApiError::conflict(mcpmux_core::DEFINITION_CHANGED));
    }

    let installed = state
        .server_app_service()
        .install(space, &p.server_id, &definition, p.inputs)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;

    Ok(serde_json::to_value(InstalledServerRef {
        id: installed.id.to_string(),
        space_id: installed.space_id,
        server_id: installed.server_id,
        enabled: installed.enabled,
    })
    .expect("InstalledServerRef is serializable"))
}

async fn servers_configure(
    state: &ControlState,
    p: ServersConfigureParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let installed = find_installed(state, space, &p.server_id).await?;

    // `update_config` replaces whole maps, so merge the caller's changes
    // into the stored values first: a partial file must not erase keys
    // (e.g. a stored API key) it does not mention.
    let inputs = merge_settings(installed.input_values.clone(), p.inputs);
    let env = p
        .env
        .map(|changes| merge_settings(installed.env_overrides.clone(), Some(changes)));
    let headers = p
        .headers
        .map(|changes| merge_settings(installed.extra_headers.clone(), Some(changes)));
    let updated = state
        .server_app_service()
        .update_config(space, &p.server_id, inputs, env, p.args, headers)
        .await?;

    Ok(serde_json::to_value(InstalledServerRef {
        id: updated.id.to_string(),
        space_id: updated.space_id,
        server_id: updated.server_id,
        enabled: updated.enabled,
    })
    .expect("InstalledServerRef is serializable"))
}

/// Apply key-level changes to a stored map: `Some(value)` sets the key,
/// `None` removes it, unmentioned keys are kept.
fn merge_settings(
    mut current: HashMap<String, String>,
    changes: Option<HashMap<String, Option<String>>>,
) -> HashMap<String, String> {
    for (key, value) in changes.unwrap_or_default() {
        match value {
            Some(value) => {
                current.insert(key, value);
            }
            None => {
                current.remove(&key);
            }
        }
    }
    current
}

async fn servers_enable(
    state: &ControlState,
    p: ServersTargetParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let installed = find_installed(state, space, &p.server_id).await?;
    let definition = installed.get_definition().ok_or_else(|| {
        ApiError::invalid(format!("server {} has no cached definition", p.server_id))
    })?;

    state
        .runtime
        .repositories
        .installed_server
        .set_enabled(&installed.id, true)
        .await?;

    let key = ServerKey::new(space, p.server_id.clone());
    state.server_manager.set_connecting(&key).await;

    let transport = build_transport_config(
        &definition.transport,
        &installed,
        Some(state.runtime.data_dir.as_path()),
    );
    let ctx = ConnectionContext::auto(space, p.server_id.clone(), transport);
    let status = match state.pool_service.connect_server(&ctx).await {
        ConnectionResult::Connected { features, .. } => {
            state.server_manager.set_connected(&key, features).await;
            ConnectionStatus::Connected
        }
        ConnectionResult::OAuthRequired { .. } => {
            state.server_manager.set_auth_required(&key, None).await;
            mark_unavailable(state, space, &p.server_id).await;
            ConnectionStatus::AuthRequired
        }
        ConnectionResult::Failed { error } => {
            state.server_manager.set_error(&key, error.clone()).await;
            mark_unavailable(state, space, &p.server_id).await;
            return Err(ApiError::conflict(error));
        }
    };

    Ok(json!({
        "id": installed.id,
        "space_id": space,
        "server_id": p.server_id,
        "enabled": true,
        "status": status,
    }))
}

async fn servers_disable(
    state: &ControlState,
    p: ServersTargetParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let installed = find_installed(state, space, &p.server_id).await?;

    state.pool_service.remove_instance(space, &p.server_id);
    state
        .pool_service
        .oauth_manager()
        .cancel_flow_for_space(space, &p.server_id);

    let key = ServerKey::new(space, p.server_id.clone());
    state.server_manager.set_disconnected(&key).await;

    state
        .runtime
        .repositories
        .installed_server
        .set_enabled(&installed.id, false)
        .await?;
    mark_unavailable(state, space, &p.server_id).await;

    Ok(json!({
        "id": installed.id,
        "space_id": space,
        "server_id": p.server_id,
        "enabled": false,
    }))
}

async fn servers_remove(
    state: &ControlState,
    p: ServersRemoveParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let installed = find_installed(state, space, &p.server_id).await?;

    state.pool_service.remove_instance(space, &p.server_id);
    state
        .pool_service
        .oauth_manager()
        .cancel_flow_for_space(space, &p.server_id);

    // Same teardown as `servers_disable`: otherwise the ServerManager keeps
    // reporting the removed server as connected until the daemon restarts.
    let key = ServerKey::new(space, p.server_id.clone());
    state.server_manager.set_disconnected(&key).await;
    mark_unavailable(state, space, &p.server_id).await;

    state
        .server_app_service()
        .uninstall(space, &p.server_id)
        .await?;

    Ok(json!({
        "removed": p.server_id,
        "id": installed.id,
        "space_id": space,
    }))
}

async fn servers_authenticate(
    state: &ControlState,
    p: ServersAuthenticateParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let installed = find_installed(state, space, &p.server_id).await?;
    let definition = installed.get_definition().ok_or_else(|| {
        ApiError::invalid(format!("server {} has no cached definition", p.server_id))
    })?;

    let key = ServerKey::new(space, p.server_id.clone());
    state.server_manager.set_connecting(&key).await;
    let transport = build_transport_config(
        &definition.transport,
        &installed,
        Some(state.runtime.data_dir.as_path()),
    );
    // auto_reconnect = false so the pool actually starts the OAuth flow.
    let ctx = ConnectionContext::new(space, p.server_id.clone(), transport);

    match state.pool_service.connect_server(&ctx).await {
        ConnectionResult::OAuthRequired { auth_url } => {
            state
                .server_manager
                .set_authenticating(&key, auth_url.clone())
                .await;
            Ok(serde_json::to_value(AuthUrlResponse {
                server_id: p.server_id,
                space_id: space.to_string(),
                authorization_url: auth_url,
            })
            .expect("AuthUrlResponse is serializable"))
        }
        ConnectionResult::Connected { features, .. } => {
            state.server_manager.set_connected(&key, features).await;
            Err(ApiError::conflict(format!(
                "server {} connected without OAuth; nothing to authenticate",
                p.server_id
            )))
        }
        ConnectionResult::Failed { error } => {
            state.server_manager.set_error(&key, error.clone()).await;
            Err(ApiError::conflict(error))
        }
    }
}

async fn mark_unavailable(state: &ControlState, space: Uuid, server_id: &str) {
    if let Err(e) = state
        .feature_service
        .mark_unavailable(&space.to_string(), server_id)
        .await
    {
        warn!(error = %e, "[control] failed to mark features unavailable");
    }
}

// =============================================================================
// Workspace bindings
// =============================================================================

async fn workspaces_list(
    state: &ControlState,
    p: WorkspacesListParams,
) -> Result<serde_json::Value, ApiError> {
    let bindings = match p.space_id {
        Some(raw) => {
            let space = resolve_space(state, Some(&raw)).await?;
            state
                .runtime
                .repositories
                .workspace_binding
                .list_for_space(&space)
                .await?
        }
        None => state.runtime.repositories.workspace_binding.list().await?,
    };
    Ok(serde_json::to_value(bindings).expect("WorkspaceBinding is serializable"))
}

async fn workspaces_bind(
    state: &ControlState,
    p: WorkspacesBindParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, Some(&p.space_id)).await?;
    validate_feature_set_ids(state, &p.feature_set_ids).await?;

    let binding_type = match p.binding_type.as_deref() {
        None | Some("path") => mcpmux_core::BindingType::Path,
        Some("id") => mcpmux_core::BindingType::Id,
        Some(other) => {
            return Err(ApiError::invalid(format!(
                "binding_type must be 'path' or 'id', got '{other}'"
            )))
        }
    };

    let workspace_root = match binding_type {
        mcpmux_core::BindingType::Path => match mcpmux_core::validate_workspace_root(&p.path) {
            mcpmux_core::WorkspaceRootValidation::Ok { normalized } => normalized,
            mcpmux_core::WorkspaceRootValidation::Empty => {
                return Err(ApiError::invalid("path is empty"))
            }
            mcpmux_core::WorkspaceRootValidation::Invalid { reason } => {
                return Err(ApiError::invalid(reason))
            }
        },
        mcpmux_core::BindingType::Id => p.path.clone(),
    };

    let now = chrono::Utc::now();
    let existing = state
        .runtime
        .repositories
        .workspace_binding
        .list()
        .await?
        .into_iter()
        .find(|b| b.binding_type == binding_type && b.workspace_root == workspace_root);

    let binding = WorkspaceBinding {
        id: existing.as_ref().map(|b| b.id).unwrap_or_else(Uuid::new_v4),
        workspace_root: workspace_root.clone(),
        binding_type,
        space_id: space,
        feature_set_ids: p.feature_set_ids,
        created_at: existing.as_ref().map(|b| b.created_at).unwrap_or(now),
        updated_at: now,
    };

    if existing.is_some() {
        state
            .runtime
            .repositories
            .workspace_binding
            .update(&binding)
            .await?;
    } else {
        state
            .runtime
            .repositories
            .workspace_binding
            .create(&binding)
            .await?;
    }

    state.emit(DomainEvent::WorkspaceBindingChanged {
        space_id: space,
        workspace_root,
    });
    Ok(serde_json::to_value(binding).expect("WorkspaceBinding is serializable"))
}

async fn workspaces_unbind(
    state: &ControlState,
    p: WorkspacesUnbindParams,
) -> Result<serde_json::Value, ApiError> {
    let id = Uuid::parse_str(&p.id).map_err(|e| ApiError::invalid(e.to_string()))?;
    let binding = state
        .runtime
        .repositories
        .workspace_binding
        .get(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("binding {} not found", p.id)))?;
    state
        .runtime
        .repositories
        .workspace_binding
        .delete(&id)
        .await?;
    state.emit(DomainEvent::WorkspaceBindingChanged {
        space_id: binding.space_id,
        workspace_root: binding.workspace_root,
    });
    Ok(json!({"deleted": p.id}))
}

// =============================================================================
// Inbound clients
// =============================================================================

async fn clients_list(
    state: &ControlState,
    _p: ClientsListParams,
) -> Result<serde_json::Value, ApiError> {
    let repo = state.inbound_client_repo();
    let mut out = Vec::new();
    for client in repo.list_clients().await? {
        let locked_space_id = repo.get_locked_space(&client.client_id).await?;
        out.push(json!({
            "id": client.client_id,
            "name": client.client_name,
            "type": client.software_id,
            "approved": client.approved,
            "locked_space_id": locked_space_id,
            "created_at": client.created_at,
            "last_seen": client.last_seen,
        }));
    }
    Ok(serde_json::Value::Array(out))
}

async fn clients_create(
    state: &ControlState,
    p: ClientsCreateParams,
) -> Result<serde_json::Value, ApiError> {
    let name = p.name.trim();
    if name.is_empty() {
        return Err(ApiError::invalid("name is required"));
    }

    let repo = state.inbound_client_repo();
    let now = chrono::Utc::now().to_rfc3339();
    let client_id = format!("mcp_{}", &Uuid::new_v4().simple().to_string()[..8]);
    let client = InboundClient {
        client_id: client_id.clone(),
        registration_type: RegistrationType::Preregistered,
        client_name: name.to_string(),
        client_alias: None,
        redirect_uris: vec![],
        grant_types: vec![],
        response_types: vec![],
        token_endpoint_auth_method: "none".to_string(),
        scope: None,
        approved: true,
        logo_uri: None,
        client_uri: None,
        software_id: Some(p.client_type.clone()),
        software_version: None,
        metadata_url: None,
        metadata_cached_at: None,
        metadata_cache_ttl: None,
        last_seen: None,
        created_at: now.clone(),
        updated_at: now,
        reports_roots: false,
        roots_capability_known: false,
    };
    repo.save_client(&client).await?;

    let (key_id, plaintext, key_prefix) = generate_api_key();
    repo.create_api_key(&key_id, &client_id, &plaintext, &key_prefix, None, None)
        .await?;

    // Map the client to the default Space's Starter so it routes out of the
    // box; a failure here must not undo the registration.
    if let Err(e) = auto_map_client(state, &client_id).await {
        warn!(error = %e, "[control] auto-map for {client_id} failed (non-fatal)");
    }

    state.emit(DomainEvent::ClientRegistered {
        client_id: client_id.clone(),
        client_name: name.to_string(),
        registration_type: Some("api_key".to_string()),
    });

    // The API key is shown exactly once; only its SHA-256 hash is stored.
    Ok(json!({
        "id": client_id,
        "name": name,
        "type": p.client_type,
        "api_key": plaintext,
        "key_prefix": key_prefix,
    }))
}

async fn clients_delete(
    state: &ControlState,
    p: ClientsDeleteParams,
) -> Result<serde_json::Value, ApiError> {
    let repo = state.inbound_client_repo();
    let deleted = repo.delete_client(&p.id).await?;
    if !deleted {
        return Err(ApiError::not_found(format!("client {} not found", p.id)));
    }

    // Remove the `<client_id> → Starter` id binding `clients create` added,
    // as the desktop does: otherwise it lingers in `workspaces list`, and a
    // future client reusing the id would inherit its routing.
    let bindings = &state.runtime.repositories.workspace_binding;
    match bindings.find_by_id_key(&p.id).await {
        Ok(Some(binding)) => match bindings.delete(&binding.id).await {
            Ok(()) => state.emit(DomainEvent::WorkspaceBindingChanged {
                space_id: binding.space_id,
                workspace_root: binding.workspace_root,
            }),
            Err(e) => warn!(error = %e, "[control] failed to remove the id binding for {}", p.id),
        },
        Ok(None) => {}
        Err(e) => warn!(error = %e, "[control] failed to look up the id binding for {}", p.id),
    }

    state.emit(DomainEvent::ClientDeleted {
        client_id: p.id.clone(),
    });
    Ok(json!({"deleted": p.id}))
}

/// Generate a strong API key: `mcpk_` + 256 bits of UUID randomness. Returns
/// `(key_id, plaintext, key_prefix)`; only the hash is ever stored.
fn generate_api_key() -> (String, String, String) {
    let key_id = Uuid::new_v4().to_string();
    let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let plaintext = format!("mcpk_{secret}");
    let key_prefix: String = plaintext.chars().take(13).collect();
    (key_id, plaintext, key_prefix)
}

/// Auto-create a clientId-keyed `id` mapping to the default Space's Starter so
/// a freshly-registered client routes somewhere sensible.
async fn auto_map_client(state: &ControlState, client_id: &str) -> anyhow::Result<()> {
    let space = state
        .runtime
        .repositories
        .space
        .get_default()
        .await?
        .ok_or_else(|| anyhow::anyhow!("no default Space configured"))?;
    let starter = state
        .runtime
        .repositories
        .feature_set
        .get_starter_for_space(&space.id.to_string())
        .await?
        .ok_or_else(|| anyhow::anyhow!("Space has no Starter FeatureSet"))?;
    let binding = WorkspaceBinding::new_id(client_id.to_string(), space.id, vec![starter.id]);
    state
        .runtime
        .repositories
        .workspace_binding
        .create(&binding)
        .await?;
    Ok(())
}

// =============================================================================
// Logs
// =============================================================================

async fn logs_list(state: &ControlState, p: LogsListParams) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let level = match p.level.as_deref() {
        Some(raw) => Some(
            LogLevel::parse(raw)
                .ok_or_else(|| ApiError::invalid(format!("unknown log level '{raw}'")))?,
        ),
        None => None,
    };
    let logs = state
        .runtime
        .server_log_manager
        .read_logs(
            &space.to_string(),
            &p.server_id,
            p.limit.unwrap_or(100),
            level,
        )
        .await?;
    Ok(serde_json::to_value(logs).expect("ServerLog is serializable"))
}

// =============================================================================
// Config export / validate
// =============================================================================

fn parse_config_format(raw: &str) -> Result<ConfigFormat, ApiError> {
    match raw.to_lowercase().as_str() {
        "cursor" => Ok(ConfigFormat::Cursor),
        "vscode" | "vscode-continue" | "continue" => Ok(ConfigFormat::VsCodeContinue),
        "claude" | "claude-desktop" => Ok(ConfigFormat::ClaudeDesktop),
        other => Err(ApiError::invalid(format!("unknown format '{other}'"))),
    }
}

fn format_name(format: ConfigFormat) -> &'static str {
    match format {
        ConfigFormat::Cursor => "cursor",
        ConfigFormat::VsCodeContinue => "vscode",
        ConfigFormat::ClaudeDesktop => "claude",
    }
}

fn resolve_placeholders(template: &str, values: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in values {
        result = result.replace(&format!("${{input:{key}}}"), value);
    }
    result
}

fn resolved_server(installed: &mcpmux_core::InstalledServer) -> Option<ResolvedServer> {
    let definition = installed.get_definition()?;
    let transport = match &definition.transport {
        TransportConfig::Stdio {
            command, args, env, ..
        } => {
            let mut resolved_env: HashMap<String, String> = env
                .iter()
                .map(|(k, v)| (k.clone(), resolve_placeholders(v, &installed.input_values)))
                .collect();
            resolved_env.extend(installed.env_overrides.clone());

            let mut resolved_args: Vec<String> = args
                .iter()
                .map(|a| resolve_placeholders(a, &installed.input_values))
                .collect();
            resolved_args.extend(installed.args_append.clone());

            ResolvedTransport::Stdio {
                command: resolve_placeholders(command, &installed.input_values),
                args: resolved_args,
                env: resolved_env,
            }
        }
        TransportConfig::Http { url, headers, .. } => {
            let mut resolved_headers: HashMap<String, String> = headers
                .iter()
                .map(|(k, v)| (k.clone(), resolve_placeholders(v, &installed.input_values)))
                .collect();
            resolved_headers.extend(installed.extra_headers.clone());
            ResolvedTransport::Http {
                url: resolve_placeholders(url, &installed.input_values),
                headers: resolved_headers,
            }
        }
    };
    Some(ResolvedServer {
        server_id: installed.server_id.clone(),
        transport,
    })
}

async fn config_export(
    state: &ControlState,
    p: ConfigExportParams,
) -> Result<serde_json::Value, ApiError> {
    let format = parse_config_format(&p.format)?;
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let installed = find_installed(state, space, &p.server_id).await?;
    if !installed.enabled {
        return Err(ApiError::conflict(format!(
            "server {} is disabled; enable it before exporting",
            p.server_id
        )));
    }
    let resolved = resolved_server(&installed).ok_or_else(|| {
        ApiError::invalid(format!("server {} has no cached definition", p.server_id))
    })?;
    let content = ConfigExporter::new()
        .export_json(format, std::slice::from_ref(&resolved))
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(serde_json::to_value(ConfigExportResponse {
        format: format_name(format).to_string(),
        server_id: p.server_id,
        content,
    })
    .expect("ConfigExportResponse is serializable"))
}

/// Export every server installed in a Space as a portable `mcpServers`
/// document. Mirrors the user-space config format so it round-trips through
/// `config import`. Credentials are NOT included.
async fn config_export_space(
    state: &ControlState,
    p: ConfigExportSpaceParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let installed = state
        .runtime
        .repositories
        .installed_server
        .list_for_space(&space.to_string())
        .await?;

    let mut servers = serde_json::Map::new();
    for server in &installed {
        let Some(definition) = server.get_definition() else {
            continue;
        };
        let mut entry = match &definition.transport {
            TransportConfig::Stdio {
                command, args, env, ..
            } => {
                let mut entry = serde_json::Map::new();
                entry.insert("command".into(), json!(command));
                if !args.is_empty() {
                    entry.insert("args".into(), json!(args));
                }
                if !env.is_empty() {
                    entry.insert("env".into(), json!(env));
                }
                entry
            }
            TransportConfig::Http { url, headers, .. } => {
                let mut entry = serde_json::Map::new();
                entry.insert("url".into(), json!(url));
                if !headers.is_empty() {
                    entry.insert("headers".into(), json!(headers));
                }
                entry
            }
        };
        entry.insert("name".into(), json!(definition.name));
        if let Some(alias) = &definition.alias {
            entry.insert("alias".into(), json!(alias));
        }
        servers.insert(definition.id.clone(), serde_json::Value::Object(entry));
    }

    let count = servers.len();
    let document = json!({ "mcpServers": servers });
    let content =
        serde_json::to_string_pretty(&document).map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(serde_json::to_value(ConfigExportSpaceResponse {
        space_id: space.to_string(),
        content,
        server_count: count,
    })
    .expect("ConfigExportSpaceResponse is serializable"))
}

/// Import an `mcpServers` document into a Space via the core user-space sync
/// service (3-way diff, auto-enable). Writes a backup of the space file first.
async fn config_import(
    state: &ControlState,
    p: ConfigImportParams,
) -> Result<serde_json::Value, ApiError> {
    let space = resolve_space(state, p.space_id.as_deref()).await?;
    let path = PathBuf::from(&p.file);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| ApiError::invalid(format!("cannot read {}: {e}", p.file)))?;
    // Reject malformed input before touching storage.
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| ApiError::invalid(format!("{} is not valid JSON: {e}", p.file)))?;
    let servers = parsed
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ApiError::invalid("file must contain an 'mcpServers' object"))?;
    if servers.is_empty() {
        return Err(ApiError::invalid(
            "'mcpServers' is empty; nothing to import",
        ));
    }

    let space_config =
        mcpmux_core::get_space_config_path(&state.runtime.spaces_dir, &space.to_string())
            .map_err(|e| ApiError::invalid(e.to_string()))?;
    let sync = mcpmux_core::application::UserSpaceSyncService::new(
        state.runtime.repositories.installed_server.clone(),
    );

    // Diff the document against what is installed from the Space file today
    // without writing anything. This is the dry-run answer, and it also
    // rejects a document the sync would refuse (e.g. two keys normalizing to
    // one server id) before the Space file is touched.
    let plan = sync
        .plan_from_content(&space.to_string(), &space_config, &raw)
        .await
        .map_err(|e| ApiError::invalid(format!("{e:#}")))?;
    if p.dry_run {
        return Ok(serde_json::to_value(ConfigImportResponse {
            space_id: space.to_string(),
            dry_run: true,
            added: plan.added,
            updated: plan.updated,
            removed: plan.removed,
            backup: None,
        })
        .expect("ConfigImportResponse is serializable"));
    }

    // Back up the Space's live config file (if any), then replace it with the
    // imported document and sync through the same path the desktop uses.
    let backup = if space_config.is_file() {
        let bak = space_config.with_extension("json.mcpmux-bak");
        std::fs::copy(&space_config, &bak)
            .map_err(|e| ApiError::internal(format!("backup failed: {e}")))?;
        Some(bak)
    } else {
        None
    };
    std::fs::write(&space_config, &raw)
        .map_err(|e| ApiError::internal(format!("cannot write {}: {e}", space_config.display())))?;

    let result = match sync.sync_from_file(&space.to_string(), &space_config).await {
        Ok(result) => result,
        Err(e) => {
            // Put the previous Space file back so it keeps matching the
            // installed servers.
            let restored = match &backup {
                Some(bak) => std::fs::copy(bak, &space_config).map(|_| ()),
                None => std::fs::remove_file(&space_config),
            };
            if let Err(restore_err) = restored {
                warn!(error = %restore_err, path = %space_config.display(),
                    "[control] failed to restore the Space file after a failed import");
            }
            return Err(ApiError::invalid(format!("{e:#}")));
        }
    };

    Ok(serde_json::to_value(ConfigImportResponse {
        space_id: space.to_string(),
        dry_run: false,
        added: result.added,
        updated: result.updated,
        removed: result.removed,
        backup: backup.map(|bak| bak.to_string_lossy().to_string()),
    })
    .expect("ConfigImportResponse is serializable"))
}

async fn config_validate(p: ConfigValidateParams) -> Result<serde_json::Value, ApiError> {
    let path = PathBuf::from(&p.file);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| ApiError::invalid(format!("cannot read {}: {e}", p.file)))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| ApiError::invalid(format!("{} is not valid JSON: {e}", p.file)))?;

    let mut warnings = Vec::new();
    let obj = match value.as_object() {
        Some(obj) => obj,
        None => {
            return Ok(serde_json::to_value(ConfigValidateResponse {
                valid: false,
                servers: Vec::new(),
                warnings: vec!["root is not a JSON object".to_string()],
            })
            .expect("ConfigValidateResponse is serializable"))
        }
    };

    // Accept any of the known server-map keys; the file may target any client.
    let mut servers = Vec::new();
    for key in ["mcpServers", "servers", "mcp", "context_servers"] {
        if let Some(map) = obj.get(key).and_then(|v| v.as_object()) {
            servers.extend(map.keys().cloned());
        }
    }
    if servers.is_empty() {
        warnings.push("no MCP servers found under mcpServers/servers/mcp/context_servers".into());
    }
    servers.sort();
    servers.dedup();

    Ok(serde_json::to_value(ConfigValidateResponse {
        valid: !servers.is_empty(),
        servers,
        warnings,
    })
    .expect("ConfigValidateResponse is serializable"))
}

// =============================================================================
// Workspace config snippet
// =============================================================================

async fn workspace_config(
    state: &ControlState,
    p: WorkspaceConfigParams,
) -> Result<serde_json::Value, ApiError> {
    let spec = mcpmux_core::find_client(&p.client).ok_or_else(|| {
        let supported: Vec<&str> = mcpmux_core::CLIENTS.iter().map(|c| c.id).collect();
        ApiError::invalid(format!(
            "unknown client '{}'; supported: {}",
            p.client,
            supported.join(", ")
        ))
    })?;
    let normalized = match mcpmux_core::validate_workspace_root(&p.path) {
        mcpmux_core::WorkspaceRootValidation::Ok { normalized } => normalized,
        mcpmux_core::WorkspaceRootValidation::Empty => {
            return Err(ApiError::invalid("path is empty"))
        }
        mcpmux_core::WorkspaceRootValidation::Invalid { reason } => {
            return Err(ApiError::invalid(reason))
        }
    };
    let mcp_url = format!("{}/mcp", state.gateway_origin.trim_end_matches('/'));
    let content = mcpmux_core::snippet(spec, &mcp_url, &normalized).map_err(ApiError::internal)?;

    Ok(serde_json::to_value(WorkspaceConfigResponse {
        path: normalized,
        client: spec.id.to_string(),
        content,
    })
    .expect("WorkspaceConfigResponse is serializable"))
}

// =============================================================================
// Gateway port management
// =============================================================================

async fn port_get(state: &ControlState) -> Result<serde_json::Value, ApiError> {
    let persisted = state
        .runtime
        .gateway_port_service
        .load_persisted_port()
        .await;
    Ok(serde_json::to_value(PortResponse {
        persisted_port: persisted,
        active_port: state.port,
        default_port: mcpmux_core::DEFAULT_GATEWAY_PORT,
    })
    .expect("PortResponse is serializable"))
}

async fn port_set(state: &ControlState, p: PortSetParams) -> Result<serde_json::Value, ApiError> {
    state
        .runtime
        .gateway_port_service
        .save_port(p.port)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(serde_json::to_value(PortResponse {
        persisted_port: Some(p.port),
        active_port: state.port,
        default_port: mcpmux_core::DEFAULT_GATEWAY_PORT,
    })
    .expect("PortResponse is serializable"))
}

async fn port_clear(state: &ControlState) -> Result<serde_json::Value, ApiError> {
    state
        .runtime
        .gateway_port_service
        .clear_persisted_port()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(serde_json::to_value(PortResponse {
        persisted_port: None,
        active_port: state.port,
        default_port: mcpmux_core::DEFAULT_GATEWAY_PORT,
    })
    .expect("PortResponse is serializable"))
}
