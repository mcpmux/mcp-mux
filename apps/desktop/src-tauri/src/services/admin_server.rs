//! Web admin HTTP server startup (loopback :45819 by default).

use super::admin_write_runtime::DesktopGatewayWriteRuntime;
use crate::state::AppState;
use crate::{
    commands::{gateway::GatewayAppState, server_manager::ServerManagerState},
    get_bundle_version,
};
use async_trait::async_trait;
#[cfg(debug_assertions)]
use mcpmux_core::service::app_settings_service::keys;
use mcpmux_core::service::is_port_available;
use mcpmux_core::{AppSettingsService, ApplicationServices, EventBus};
use mcpmux_gateway::admin::event_hub::AdminEventHub;
use mcpmux_gateway::admin::runtime::GatewayRuntime;
use mcpmux_gateway::admin::ui_events::AdminUiEventBus;
use mcpmux_gateway::admin::{AdminBridgeCtx, BackendBuildStamp};
use mcpmux_gateway::pool::ConnectionStatus as GatewayConnectionStatus;
use mcpmux_gateway::{AdminConfig, AdminServer, AdminServerHandle};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Tracks the running admin server and shared gateway liveness flag.
pub struct AdminServerState {
    /// Background task handle for graceful shutdown.
    pub handle: Option<AdminServerHandle>,
    /// Updated when the MCP gateway starts or stops (admin `/api/v1/health`).
    pub gateway_running: Arc<AtomicBool>,
    /// Direct UI events from Tauri `app.emit` paths (oauth, session overrides).
    pub ui_event_bus: Arc<AdminUiEventBus>,
    /// Merged SSE fan-in hub (EventBus + gateway domain + direct emits).
    pub event_hub: Arc<AdminEventHub>,
}

impl AdminServerState {
    /// Create admin server state with shared event buses for SSE fan-in.
    pub fn new() -> Self {
        let ui_event_bus = Arc::new(AdminUiEventBus::new());
        let event_hub = Arc::new(AdminEventHub::new(ui_event_bus.clone()));
        Self {
            handle: None,
            gateway_running: Arc::new(AtomicBool::new(false)),
            ui_event_bus,
            event_hub,
        }
    }
}

impl Default for AdminServerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether the admin HTTP server should start (settings + dev overrides).
async fn resolve_admin_enabled_for_startup(app_state: &AppState) -> bool {
    if std::env::var("MCPMUX_DEV_DISABLE_ADMIN").as_deref() == Ok("1") {
        info!("[Admin] Skipped (MCPMUX_DEV_DISABLE_ADMIN=1)");
        return false;
    }

    let settings = AppSettingsService::new(app_state.settings_repository.clone());

    #[cfg(debug_assertions)]
    {
        if std::env::var("MCPMUX_DEV_ADMIN").as_deref() == Ok("1") {
            info!("[Admin] Enabled for this dev session (MCPMUX_DEV_ADMIN=1)");
            return true;
        }

        match mcpmux_core::AppSettingsRepository::get(
            app_state.settings_repository.as_ref(),
            keys::gateway::ADMIN_ENABLED,
        )
        .await
        {
            Ok(None) => {
                if let Err(e) = settings.set_admin_enabled(true).await {
                    warn!("[Admin] Failed to persist dev default admin_enabled: {}", e);
                } else {
                    info!("[Admin] Dev default: enabled web admin (setting was unset)");
                }
                return true;
            }
            Ok(Some(_)) => {}
            Err(e) => warn!("[Admin] Could not read admin_enabled: {}", e),
        }
    }

    settings.get_admin_enabled().await
}

/// Resolve the built frontend directory for static SPA serving.
pub fn resolve_frontend_dist(app: &AppHandle) -> PathBuf {
    if let Ok(resource) = app.path().resource_dir() {
        let dist = resource.join("dist");
        if dist.join("index.html").is_file() {
            return dist;
        }
    }

    let dev_dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist");
    if dev_dist.join("index.html").is_file() {
        return dev_dist;
    }

    dev_dist
}

/// Build `ApplicationServices` for admin handlers from desktop `AppState`.
fn build_application_services(
    app_state: &AppState,
    event_bus: Arc<EventBus>,
) -> anyhow::Result<Arc<ApplicationServices>> {
    Ok(Arc::new(app_state.build_application_services(event_bus)?))
}

struct DesktopGatewayRuntime {
    gateway_port_service: Arc<mcpmux_core::GatewayPortService>,
    gateway_state: Arc<RwLock<GatewayAppState>>,
    server_manager_state: Arc<RwLock<ServerManagerState>>,
}

impl DesktopGatewayRuntime {
    fn new(
        gateway_port_service: Arc<mcpmux_core::GatewayPortService>,
        gateway_state: Arc<RwLock<GatewayAppState>>,
        server_manager_state: Arc<RwLock<ServerManagerState>>,
    ) -> Self {
        Self {
            gateway_port_service,
            gateway_state,
            server_manager_state,
        }
    }
}

#[async_trait]
impl GatewayRuntime for DesktopGatewayRuntime {
    async fn get_gateway_status(
        &self,
        space_id: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        let state = self.gateway_state.read().await;
        let active_sessions = if let Some(ref gateway_state) = state.gateway_state {
            gateway_state.read().await.sessions.len()
        } else {
            0
        };

        let connected_backends = {
            let manager_state = self.server_manager_state.read().await;
            if let Some(ref manager) = manager_state.manager {
                if let Some(space_id) = space_id {
                    let space_id = uuid::Uuid::parse_str(&space_id)?;
                    manager.connected_count_for_space(&space_id).await
                } else {
                    manager.connected_count().await
                }
            } else {
                0
            }
        };

        Ok(json!({
            "running": state.running,
            "url": state.url,
            "active_sessions": active_sessions,
            "connected_backends": connected_backends,
        }))
    }

    async fn probe_gateway_start(&self, port: Option<u16>) -> anyhow::Result<serde_json::Value> {
        let (preferred_port, source) = if let Some(port) = port {
            (port, "override")
        } else if let Some(port) = self.gateway_port_service.load_persisted_port().await {
            (port, "configured")
        } else {
            (mcpmux_core::DEFAULT_GATEWAY_PORT, "default")
        };
        Ok(json!({
            "preferredPort": preferred_port,
            "preferredAvailable": is_port_available(preferred_port),
            "source": source,
        }))
    }

    async fn take_pending_port_conflict(&self) -> anyhow::Result<serde_json::Value> {
        let mut state = self.gateway_state.write().await;
        Ok(state
            .pending_port_conflict
            .take()
            .map(|conflict| {
                json!({
                    "preferredPort": conflict.preferred_port,
                    "source": conflict.source,
                })
            })
            .unwrap_or(serde_json::Value::Null))
    }

    async fn get_gateway_port_settings(&self) -> anyhow::Result<serde_json::Value> {
        let configured_port = self.gateway_port_service.load_persisted_port().await;
        let active_port = {
            let state = self.gateway_state.read().await;
            state
                .url
                .as_deref()
                .and_then(|url| url.split("://").nth(1))
                .and_then(|host_port| host_port.split('/').next())
                .and_then(|host_port| host_port.rsplit(':').next())
                .and_then(|port| port.parse::<u16>().ok())
        };
        Ok(json!({
            "configuredPort": configured_port,
            "defaultPort": mcpmux_core::DEFAULT_GATEWAY_PORT,
            "activePort": active_port,
        }))
    }

    async fn reset_gateway_port(&self) -> anyhow::Result<serde_json::Value> {
        self.gateway_port_service.clear_persisted_port().await?;
        Ok(json!({ "ok": true }))
    }

    async fn list_connected_servers(&self) -> anyhow::Result<serde_json::Value> {
        Ok(json!([]))
    }

    async fn get_pool_stats(&self) -> anyhow::Result<serde_json::Value> {
        let state = self.gateway_state.read().await;
        let stats = match &state.pool_service {
            Some(pool) => pool.stats(),
            None => mcpmux_gateway::PoolStats::default(),
        };
        Ok(json!({
            "total_instances": stats.total_instances,
            "connected_instances": stats.connected_instances,
            "total_space_server_mappings": stats.connecting_instances + stats.failed_instances + stats.oauth_pending_instances,
        }))
    }

    async fn list_reported_workspace_roots(&self) -> anyhow::Result<serde_json::Value> {
        let state = self.gateway_state.read().await;
        Ok(json!(state
            .session_roots
            .as_ref()
            .map(|registry| registry.list_all_roots())
            .unwrap_or_default()))
    }

    async fn list_meta_tool_grants(&self) -> anyhow::Result<serde_json::Value> {
        let state = self.gateway_state.read().await;
        let Some(ref broker) = state.approval_broker else {
            return Ok(json!([]));
        };
        Ok(json!(broker
            .list_always_allow()
            .into_iter()
            .map(|(client_id, tool_name)| json!({
                "client_id": client_id,
                "tool_name": tool_name,
            }))
            .collect::<Vec<_>>()))
    }

    async fn get_oauth_clients(&self) -> anyhow::Result<serde_json::Value> {
        let state = self.gateway_state.read().await;
        let Some(ref gateway_state) = state.gateway_state else {
            return Err(anyhow::anyhow!("Gateway not running"));
        };
        let gateway_state = gateway_state.read().await;
        let Some(repository) = gateway_state.inbound_client_repository() else {
            return Err(anyhow::anyhow!("Database not available"));
        };
        let clients = repository.list_clients().await?;
        let approved = clients
            .into_iter()
            .filter(|client| client.approved)
            .map(|client| {
                json!({
                    "client_id": client.client_id,
                    "registration_type": client.registration_type.as_str(),
                    "client_name": client.client_name,
                    "client_alias": client.client_alias,
                    "redirect_uris": client.redirect_uris,
                    "scope": client.scope,
                    "approved": client.approved,
                    "logo_uri": client.logo_uri,
                    "client_uri": client.client_uri,
                    "software_id": client.software_id,
                    "software_version": client.software_version,
                    "metadata_url": client.metadata_url,
                    "metadata_cached_at": client.metadata_cached_at,
                    "metadata_cache_ttl": client.metadata_cache_ttl,
                    "last_seen": client.last_seen,
                    "created_at": client.created_at,
                    "reports_roots": client.reports_roots,
                    "roots_capability_known": client.roots_capability_known,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!(approved))
    }

    async fn get_oauth_client_grants(
        &self,
        client_id: String,
        space_id: String,
    ) -> anyhow::Result<serde_json::Value> {
        let state = self.gateway_state.read().await;
        let Some(ref grant_service) = state.grant_service else {
            return Err(anyhow::anyhow!("Gateway not running"));
        };
        Ok(json!(
            grant_service
                .get_grants_for_space(&client_id, &space_id)
                .await?
        ))
    }

    async fn get_server_statuses(&self, space_id: String) -> anyhow::Result<serde_json::Value> {
        let space_uuid = uuid::Uuid::parse_str(&space_id)
            .map_err(|e| anyhow::anyhow!("Invalid space_id: {e}"))?;

        let manager_state = self.server_manager_state.read().await;
        let Some(ref manager) = manager_state.manager else {
            return Err(anyhow::anyhow!("ServerManager not initialized"));
        };

        let statuses = manager.get_all_statuses(space_uuid).await;
        let mut result = serde_json::Map::new();
        for (server_id, (status, flow_id, has_connected_before, message)) in statuses {
            result.insert(
                server_id.clone(),
                json!({
                    "server_id": server_id,
                    "status": gateway_status_to_ui(status),
                    "flow_id": flow_id,
                    "has_connected_before": has_connected_before,
                    "message": message,
                }),
            );
        }
        Ok(json!(result))
    }
}

/// Map gateway pool status to the UI-facing string (`oauth_required`, not `auth_required`).
fn gateway_status_to_ui(status: GatewayConnectionStatus) -> &'static str {
    match status {
        GatewayConnectionStatus::Disconnected => "disconnected",
        GatewayConnectionStatus::Connecting => "connecting",
        GatewayConnectionStatus::Connected => "connected",
        GatewayConnectionStatus::Refreshing => "refreshing",
        GatewayConnectionStatus::AuthRequired => "oauth_required",
        GatewayConnectionStatus::Authenticating => "authenticating",
        GatewayConnectionStatus::Error => "error",
    }
}

/// Stop the web admin server if it is running.
pub async fn stop_admin_server(admin_state: &Arc<tokio::sync::RwLock<AdminServerState>>) {
    let handle = {
        let mut guard = admin_state.write().await;
        guard.handle.take()
    };
    if let Some(handle) = handle {
        handle.shutdown();
        if let Err(e) = handle.task.await {
            warn!("[Admin] Admin server task join error: {:?}", e);
        }
        info!("[Admin] Stopped");
    }
}

/// Apply current settings: stop any running admin server, then start if enabled.
pub async fn reload_admin_server(
    app: AppHandle,
    admin_state: Arc<tokio::sync::RwLock<AdminServerState>>,
    gateway_state: Arc<RwLock<GatewayAppState>>,
    server_manager_state: Arc<RwLock<ServerManagerState>>,
    event_bus: Arc<EventBus>,
) {
    stop_admin_server(&admin_state).await;
    start_admin_server_if_enabled(
        app,
        admin_state,
        gateway_state,
        server_manager_state,
        event_bus,
    )
    .await;
}

/// Start the admin server when `gateway.admin_enabled` is true.
pub async fn start_admin_server_if_enabled(
    app: AppHandle,
    admin_state: Arc<tokio::sync::RwLock<AdminServerState>>,
    gateway_state: Arc<RwLock<GatewayAppState>>,
    server_manager_state: Arc<RwLock<ServerManagerState>>,
    event_bus: Arc<EventBus>,
) {
    let app_state: tauri::State<'_, AppState> = app.state();
    let settings = AppSettingsService::new(app_state.settings_repository.clone());
    if !resolve_admin_enabled_for_startup(app_state.inner()).await {
        info!("[Admin] Web admin disabled (gateway.admin_enabled=false)");
        return;
    }

    let port = settings.get_admin_port().await;
    let trust_cf_access = settings.get_admin_trust_cf_access().await;
    let cf_team_domain = settings.get_admin_cf_team_domain().await;

    let config = AdminConfig {
        host: "127.0.0.1".to_string(),
        port,
        trust_cf_access,
        cf_team_domain,
        cf_access_audience: None,
        cf_validator_override: None,
    };

    let gateway_running = {
        let guard = admin_state.read().await;
        guard.gateway_running.clone()
    };
    let event_hub = {
        let guard = admin_state.read().await;
        guard.event_hub.clone()
    };

    let services = match build_application_services(&app_state, event_bus) {
        Ok(s) => s,
        Err(e) => {
            warn!("[Admin] Failed to build ApplicationServices: {}", e);
            return;
        }
    };

    let cf_validator = match AdminServer::build_cf_validator(&config).await {
        Ok(v) => v,
        Err(e) => {
            warn!("[Admin] CF Access validator init failed: {}", e);
            return;
        }
    };

    let frontend_dist = resolve_frontend_dist(&app);
    let dist_ready = frontend_dist.join("index.html").is_file();
    let auto_launch_enabled = app
        .try_state::<tauri_plugin_autostart::AutoLaunchManager>()
        .and_then(|manager| manager.is_enabled().ok());
    let gateway_runtime = Arc::new(DesktopGatewayRuntime::new(
        app_state.gateway_port_service.clone(),
        gateway_state.clone(),
        server_manager_state.clone(),
    ));
    let gateway_writes = Arc::new(DesktopGatewayWriteRuntime::new(
        app.clone(),
        gateway_state.clone(),
    ));
    let bridge = Arc::new(AdminBridgeCtx {
        services: services.clone(),
        spaces_dir: app_state.spaces_dir().to_path_buf(),
        data_dir: app_state.data_dir().to_path_buf(),
        gateway_port_service: app_state.gateway_port_service.clone(),
        server_discovery: app_state.server_discovery.clone(),
        settings_repository: app_state.settings_repository.clone(),
        workspace_binding_repository: app_state.workspace_binding_repository.clone(),
        workspace_appearance_repository: app_state.workspace_appearance_repository.clone(),
        server_feature_repository: app_state.server_feature_repository_core.clone(),
        server_log_manager: app_state.server_log_manager.clone(),
        space_service: Arc::new(mcpmux_core::SpaceService::new(
            app_state.space_service.space_repository(),
        )),
        gateway_runtime,
        gateway_writes,
        feature_set_repository: app_state.feature_set_repository.clone(),
        auto_launch_enabled,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        bundle_version: get_bundle_version(),
        backend_build: BackendBuildStamp {
            git_sha: option_env!("MCPMUX_BUILD_GIT_SHA")
                .unwrap_or("dev")
                .to_string(),
            git_branch: option_env!("MCPMUX_BUILD_GIT_BRANCH")
                .unwrap_or("dev")
                .to_string(),
            commit_time: option_env!("MCPMUX_BUILD_COMMIT_TIME")
                .unwrap_or("")
                .to_string(),
            build_time: option_env!("MCPMUX_BUILD_TIME").unwrap_or("").to_string(),
        },
    });
    let backend_git_sha = option_env!("MCPMUX_BUILD_GIT_SHA")
        .unwrap_or("dev")
        .to_string();
    let frontend_dist_log = frontend_dist.clone();
    let server = match AdminServer::new(
        config.clone(),
        services,
        bridge,
        event_hub,
        gateway_running,
        frontend_dist,
        cf_validator,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            warn!("[Admin] Failed to build admin server: {}", e);
            return;
        }
    };

    let handle = server.spawn();
    info!(
        "[Admin] Started on http://{}:{} (cf_access={}, static_spa={})",
        config.host, config.port, config.trust_cf_access, dist_ready
    );
    info!(
        "[Admin] Backend | sha: {} | branch: {} | committed: {} | built: {}",
        backend_git_sha,
        option_env!("MCPMUX_BUILD_GIT_BRANCH").unwrap_or("dev"),
        option_env!("MCPMUX_BUILD_COMMIT_TIME").unwrap_or(""),
        option_env!("MCPMUX_BUILD_TIME").unwrap_or(""),
    );
    if dist_ready {
        log_spa_build_stamp(&frontend_dist_log, &backend_git_sha);
    }
    info!(
        "[Admin] Dev HMR UI: http://127.0.0.1:1420 (Vite proxies /api → :{}) — run pnpm dev:web:admin or pnpm dev:admin",
        config.port
    );
    if !dist_ready {
        info!(
            "[Admin] Production-parity UI: run `pnpm build:web:admin` then open http://127.0.0.1:{}/",
            config.port
        );
    }

    let mut guard = admin_state.write().await;
    guard.handle = Some(handle);
}

/// SPA build metadata written by `pnpm build:web:admin` into `dist/build-stamp.json`.
#[derive(Debug, Deserialize)]
struct SpaBuildStamp {
    git_sha: String,
    git_branch: String,
    commit_time: String,
    #[serde(default)]
    commit_at: String,
    build_time: String,
    #[serde(default)]
    build_at: String,
}

/// Log SPA bundle stamp and warn when it diverges from the running backend binary.
fn log_spa_build_stamp(dist: &Path, backend_sha: &str) {
    let path = dist.join("build-stamp.json");
    let Ok(contents) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(stamp) = serde_json::from_str::<SpaBuildStamp>(&contents) else {
        warn!("[Admin] SPA bundle build-stamp.json is invalid or unreadable");
        return;
    };
    let committed = if stamp.commit_at.is_empty() {
        stamp.commit_time.as_str()
    } else {
        stamp.commit_at.as_str()
    };
    let built = if stamp.build_at.is_empty() {
        stamp.build_time.as_str()
    } else {
        stamp.build_at.as_str()
    };
    info!(
        "[Admin] SPA bundle | sha: {} | branch: {} | committed: {} | built: {}",
        stamp.git_sha, stamp.git_branch, committed, built
    );
    if !backend_sha.is_empty() && stamp.git_sha != backend_sha {
        warn!(
            "[Admin] SPA bundle sha {} != backend sha {} — run `pnpm build:web:admin`",
            stamp.git_sha, backend_sha
        );
    }
}

/// Sync gateway liveness into the admin health endpoint.
pub fn set_gateway_running(admin_state: &AdminServerState, running: bool) {
    admin_state
        .gateway_running
        .store(running, Ordering::Relaxed);
}

/// Register gateway domain events with the admin SSE hub.
pub async fn register_gateway_sse(
    admin_state: &AdminServerState,
    gateway_state: &Arc<RwLock<mcpmux_gateway::GatewayState>>,
) {
    let tx = gateway_state.read().await.domain_event_sender();
    admin_state.event_hub.register_gateway_events(tx).await;
}

/// Clear gateway domain event fan-in when the MCP gateway stops.
pub async fn clear_gateway_sse(admin_state: &AdminServerState) {
    admin_state.event_hub.clear_gateway_events().await;
}
