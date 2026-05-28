//! Gateway management commands
//!
//! IPC commands for controlling the local MCP gateway server.

use crate::commands::server_manager::ServerManagerState;
use crate::AppState;
use mcpmux_core::service::{allocate_dynamic_port, is_port_available};
use mcpmux_core::DomainEvent;
use mcpmux_gateway::admin::ui_events::AdminUiEventBus;
use crate::services::ui_events::OAUTH_CONSENT_REQUEST_CHANNEL;
use mcpmux_gateway::{
    ConnectionContext, ConnectionResult, FeatureService, InstalledServerInfo, OAuthCompleteEvent,
    PoolService, ResolvedTransport, ServerKey, ServerManager,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

/// Gateway status response
#[derive(Debug, Serialize)]
pub struct GatewayStatus {
    /// Whether the gateway is running
    pub running: bool,
    /// Gateway URL if running
    pub url: Option<String>,
    /// Number of active client sessions
    pub active_sessions: usize,
    /// Number of connected backend servers
    pub connected_backends: usize,
}

/// Backend server status (from pool)
#[derive(Debug, Serialize)]
pub struct BackendStatusResponse {
    pub server_id: String,
    pub status: String,
    pub tools_count: usize,
}

/// Information about an auto-start attempt that was aborted because the
/// preferred port was busy. The frontend reads this on mount and triggers
/// the port-conflict confirm dialog.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPortConflict {
    pub preferred_port: u16,
    pub source: &'static str,
}

/// Gateway state managed by Tauri
#[derive(Default)]
pub struct GatewayAppState {
    /// Gateway running flag
    pub running: bool,
    /// Gateway URL advertised to clients.
    pub url: Option<String>,
    /// Locally bound port. This remains the actual listener port even when
    /// `url` is a public tunnel origin such as https://mcp.example.com.
    pub bound_port: Option<u16>,
    /// Gateway task + graceful-shutdown signal. `shutdown()` + awaiting
    /// `task` (with a timeout) lets the OS reclaim the listener socket
    /// cleanly; `.abort()` alone can leave an orphaned kernel-level bind.
    pub handle: Option<mcpmux_gateway::GatewayServerHandle>,
    /// Gateway state reference for accessing backends
    pub gateway_state: Option<Arc<RwLock<mcpmux_gateway::GatewayState>>>,
    /// Server connection pool service (initialized when gateway starts)
    pub pool_service: Option<Arc<PoolService>>,
    /// Feature service for feature discovery/caching
    pub feature_service: Option<Arc<FeatureService>>,
    /// Event emitter for triggering MCP notifications (legacy - prefer grant_service)
    pub event_emitter: Option<Arc<mcpmux_gateway::EventEmitter>>,
    /// Grant service for centralized grant management with auto-notifications
    pub grant_service: Option<Arc<mcpmux_gateway::GrantService>>,
    /// Approval broker for meta-tool writes (publisher attached on gateway start)
    pub approval_broker: Option<Arc<mcpmux_gateway::services::ApprovalBroker>>,
    /// Set when auto-start couldn't bind the preferred port; the UI will
    /// read this on mount and prompt the user.
    pub pending_port_conflict: Option<PendingPortConflict>,
    /// Live map of `mcp-session-id → reported workspace roots`. Populated
    /// by the gateway handler when clients declare the `roots` capability.
    /// Surfaced to the desktop Workspaces tab so users can see + act on
    /// every folder connected clients are currently operating in.
    pub session_roots: Option<Arc<mcpmux_gateway::services::SessionRootsRegistry>>,
}

/// Gracefully shuts down a running gateway and waits for the axum task
/// to finish so the TCP listener is released back to the OS.
///
/// Without this, `handle.abort()` alone can leave an orphaned
/// kernel-level bind — a listener socket that netstat still reports even
/// though no process exists — preventing the next `start_gateway` from
/// binding the same port.
///
/// Flow:
/// 1. Send the graceful-shutdown signal (axum drains in-flight requests).
/// 2. Await the task up to 2s so Rust Drop closes the listener fd.
/// 3. If the task hasn't returned by then, abort as a last resort.
pub(crate) async fn shutdown_gateway_handle(mut handle: mcpmux_gateway::GatewayServerHandle) {
    let abort = handle.task.abort_handle();
    handle.shutdown();
    match tokio::time::timeout(std::time::Duration::from_secs(2), handle.task).await {
        Ok(Ok(Ok(()))) => info!("[Gateway] Gateway task exited cleanly"),
        Ok(Ok(Err(e))) => warn!(
            "[Gateway] Gateway task returned error during shutdown: {}",
            e
        ),
        Ok(Err(e)) if e.is_cancelled() => info!("[Gateway] Gateway task was already cancelled"),
        Ok(Err(e)) => warn!("[Gateway] Gateway task join error: {}", e),
        Err(_) => {
            warn!(
                "[Gateway] Graceful shutdown timed out after 2s — aborting task \
                 (listener socket may briefly linger in kernel)"
            );
            abort.abort();
        }
    }
}

/// Bring the main webview window forward so the user sees a popup the
/// gateway just emitted. Best-effort — silently no-ops when the window
/// doesn't exist (rare, e.g. during teardown). Used by the approval
/// publisher and the WorkspaceNeedsBinding bridge so an LLM tool call or
/// a fresh client connection automatically draws the user's eye to the
/// mcpmux app instead of the dialog rendering invisibly under another
/// window.
const GATEWAY_PUBLIC_BASE_URL_KEY: &str = "gateway.public_base_url";
const GATEWAY_NETWORK_ACCESS_KEY: &str = "gateway.network_access_enabled";

pub(crate) fn normalize_public_base_url(raw: &str) -> Result<Option<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let parsed = url::Url::parse(trimmed).map_err(|e| format!("Invalid public base URL: {}", e))?;

    if parsed.scheme() != "https" {
        return Err("Public base URL must start with https://".to_string());
    }
    if parsed.host_str().is_none() {
        return Err("Public base URL must include a host".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Public base URL must not include credentials".to_string());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("Public base URL must not include a query string or fragment".to_string());
    }
    if parsed.path() != "/" && !parsed.path().is_empty() {
        return Err(
            "Public base URL must be an origin only, for example https://mcp.example.com"
                .to_string(),
        );
    }

    let origin = match parsed.port() {
        Some(port) => format!(
            "{}://{}:{}",
            parsed.scheme(),
            parsed.host_str().unwrap(),
            port
        ),
        None => format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap()),
    };
    Ok(Some(origin.trim_end_matches('/').to_string()))
}

pub(crate) async fn load_public_base_url_from_repo(
    settings_repository: &Arc<dyn mcpmux_core::AppSettingsRepository>,
) -> Option<String> {
    settings_repository
        .get(GATEWAY_PUBLIC_BASE_URL_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|value| match normalize_public_base_url(&value) {
            Ok(normalized) => normalized,
            Err(e) => {
                warn!(
                    "[Gateway] Ignoring invalid persisted public base URL: {}",
                    e
                );
                None
            }
        })
}

pub(crate) async fn load_public_base_url(app_state: &AppState) -> Option<String> {
    load_public_base_url_from_repo(&app_state.settings_repository).await
}

/// The address the gateway binds to: loopback by default, or `0.0.0.0` (all
/// interfaces) once the user opts into network access so other devices on the
/// LAN can reach it.
pub(crate) fn bind_host_for(network_access: bool) -> &'static str {
    if network_access {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    }
}

pub(crate) async fn load_network_access_from_repo(
    settings_repository: &Arc<dyn mcpmux_core::AppSettingsRepository>,
) -> bool {
    settings_repository
        .get(GATEWAY_NETWORK_ACCESS_KEY)
        .await
        .ok()
        .flatten()
        .map(|value| value == "true")
        .unwrap_or(false)
}

pub(crate) async fn load_network_access(app_state: &AppState) -> bool {
    load_network_access_from_repo(&app_state.settings_repository).await
}

pub(crate) fn advertised_base_url(public_base_url: Option<&str>, port: u16) -> String {
    public_base_url
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(|url| url.trim_end_matches('/').to_string())
        .unwrap_or_else(|| format!("http://localhost:{}", port))
}

pub(crate) fn focus_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    // unminimize + show + set_focus together cover every state the user
    // could have left the window in (minimized, hidden behind another
    // app, hidden by user via the close-to-tray flow).
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Wire the meta-tool approval broker to the desktop event bus so write
/// tools (e.g. `mcpmux_bind_current_workspace`) can prompt the React
/// dialog. Both the manual `start_gateway` command and the lib.rs
/// auto-start path must call this — without it the broker stays
/// publisher-less and every write surfaces as
/// `approval_required: no desktop attached to mcpmux gateway`.
pub(crate) async fn attach_approval_publisher<R: tauri::Runtime>(
    approval_broker: &Arc<mcpmux_gateway::services::ApprovalBroker>,
    app_handle: tauri::AppHandle<R>,
) {
    let publisher: mcpmux_gateway::services::meta_tools::ApprovalPublisher = Arc::new(move |req| {
        let app_handle = app_handle.clone();
        Box::pin(async move {
            // Bring the window forward BEFORE emitting so the dialog
            // animates into a visible window — otherwise it'd render
            // behind whatever the user is currently focused on.
            focus_main_window(&app_handle);
            // Emit the request; the React layer owns rendering +
            // collecting the user's decision. Failure to emit means
            // no desktop frontend is listening — broker maps that to
            // "approval_required" to the calling tool.
            match app_handle.emit("meta-tool-approval-request", &req) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "[meta-tool] failed to emit approval request"
                    );
                    false
                }
            }
        })
    });
    approval_broker.set_publisher(publisher).await;
}

/// Wires up ServerManager state + the OAuth completion handler + the
/// periodic refresh loop after a GatewayServer has been spawned.
///
/// Both the auto-start path (in `lib.rs`) and the `start_gateway` Tauri
/// command must call this — without it, ServerManagerState.manager stays
/// None and the Servers page shows every server stuck on "Connecting..."
/// because `get_server_statuses` can't reach the ServerManager.
///
/// Call order matters: **subscribe to OAuth events before spawning the
/// gateway** (the subscription is passed in already-created), and call
/// this helper before or after `server.spawn()` — but always before any
/// user-facing code queries server statuses.
pub(crate) async fn init_gateway_runtime(
    pool_service: Arc<PoolService>,
    server_manager: Arc<ServerManager>,
    oauth_completion_rx: tokio::sync::broadcast::Receiver<OAuthCompleteEvent>,
    sm_state: Arc<RwLock<ServerManagerState>>,
) {
    // Store ServerManager + PoolService so the Servers page commands can
    // read them. A fresh Arc per start — old handlers on a stopped gateway
    // become orphans and drop naturally.
    {
        let mut sm = sm_state.write().await;
        sm.manager = Some(server_manager.clone());
        sm.pool_service = Some(pool_service.clone());
    }
    info!("[Gateway] ServerManager + PoolService attached to state");

    // OAuth completion handler — reconnects servers after the user finishes
    // the OAuth flow in the browser. Spawned as a detached task; lives as
    // long as the broadcast channel is alive (drops naturally when pool is
    // dropped on next gateway start).
    let sm_for_oauth = server_manager.clone();
    let pool_for_oauth = pool_service.clone();
    tokio::spawn(async move {
        let mut rx = oauth_completion_rx;
        info!("[OAuth Handler] Listening for OAuth completions");
        loop {
            match rx.recv().await {
                Ok(event) => {
                    info!(
                        "[OAuth Handler] Completion received: server={} success={}",
                        event.server_id, event.success
                    );
                    if event.success {
                        let sm = sm_for_oauth.clone();
                        let pool = pool_for_oauth.clone();
                        let server_id = event.server_id.clone();
                        let space_id = event.space_id;
                        tokio::spawn(async move {
                            let key = ServerKey::new(space_id, &server_id);
                            info!("[OAuth Handler] Reconnecting {} after OAuth", server_id);
                            sm.set_connecting(&key).await;
                            match pool.reconnect_instance(space_id, &server_id).await {
                                ConnectionResult::Connected { features, .. } => {
                                    info!(
                                        "[OAuth Handler] Reconnected {} — {} features",
                                        server_id,
                                        features.tools.len()
                                    );
                                    sm.set_connected(&key, features).await;
                                }
                                ConnectionResult::OAuthRequired { .. } => {
                                    warn!(
                                        "[OAuth Handler] {} still needs OAuth after completion",
                                        server_id
                                    );
                                    sm.set_auth_required(
                                        &key,
                                        Some("OAuth still required".to_string()),
                                    )
                                    .await;
                                }
                                ConnectionResult::Failed { error } => {
                                    error!(
                                        "[OAuth Handler] Reconnect failed for {}: {}",
                                        server_id, error
                                    );
                                    sm.set_error(&key, error).await;
                                }
                            }
                        });
                    } else {
                        let key = ServerKey::new(event.space_id, &event.server_id);
                        let err = event.error.unwrap_or_else(|| "OAuth failed".to_string());
                        warn!(
                            "[OAuth Handler] OAuth failed for {}: {}",
                            event.server_id, err
                        );
                        sm_for_oauth.set_auth_required(&key, Some(err)).await;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("[OAuth Handler] Lagged {} messages", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    info!("[OAuth Handler] Channel closed, stopping");
                    break;
                }
            }
        }
    });
    info!("[Gateway] OAuth completion handler spawned");

    // Periodic refresh loop — re-fetches features from each connected
    // server every ~60s so long-running sessions don't drift.
    let _refresh = server_manager.clone().start_periodic_refresh();
    info!("[Gateway] Periodic refresh loop started");
}

/// Start domain event bridge from Gateway to Tauri
///
/// Routes all DomainEvents to appropriate frontend channels.
/// This replaces the old GatewayEvent bridge with a unified DomainEvent system.
pub fn start_domain_event_bridge(
    app_handle: &AppHandle,
    gateway_state: Arc<RwLock<mcpmux_gateway::GatewayState>>,
) {
    let app_handle_clone = app_handle.clone();

    tokio::spawn(async move {
        let mut event_rx = {
            let state = gateway_state.read().await;
            state.subscribe_domain_events()
        };

        info!("[Gateway] Domain event bridge started");

        while let Ok(event) = event_rx.recv().await {
            let event_type = event.type_name();

            // Some domain events imply a popup the user must see (a workspace
            // root needs binding, a backend wants OAuth, etc.). Bring the
            // window forward BEFORE emitting so the popup animates into a
            // visible window instead of rendering behind another app.
            if matches!(event, DomainEvent::WorkspaceNeedsBinding { .. }) {
                focus_main_window(&app_handle_clone);
            }

            // Map domain events to UI channels
            let (channel, payload) = map_domain_event_to_ui(&event);

            trace!(
                event_type = event_type,
                channel = channel,
                "[Gateway] Forwarding domain event to UI"
            );

            // Emit to Tauri webview and admin SSE subscribers.
            let ui_event_bus = {
                let admin_state: tauri::State<
                    '_,
                    Arc<tokio::sync::RwLock<crate::services::AdminServerState>>,
                > = app_handle_clone.state();
                let guard = admin_state.read().await;
                guard.ui_event_bus.clone()
            };
            crate::services::ui_events::emit_ui_channel(
                &app_handle_clone,
                Some(&ui_event_bus),
                channel,
                payload,
            );
        }

        info!("[Gateway] Domain event bridge stopped");
    });
}

/// Map a DomainEvent to UI channel and payload
fn map_domain_event_to_ui(event: &DomainEvent) -> (&'static str, serde_json::Value) {
    match event {
        // Space events
        DomainEvent::SpaceCreated {
            space_id,
            name,
            icon,
        } => (
            "space-changed",
            serde_json::json!({
                "action": "created",
                "space_id": space_id,
                "name": name,
                "icon": icon,
            }),
        ),
        DomainEvent::SpaceUpdated { space_id, name } => (
            "space-changed",
            serde_json::json!({
                "action": "updated",
                "space_id": space_id,
                "name": name,
            }),
        ),
        DomainEvent::SpaceDeleted { space_id } => (
            "space-changed",
            serde_json::json!({
                "action": "deleted",
                "space_id": space_id,
            }),
        ),
        // Server lifecycle events
        DomainEvent::ServerInstalled {
            space_id,
            server_id,
            server_name,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "installed",
                "space_id": space_id,
                "server_id": server_id,
                "server_name": server_name,
            }),
        ),
        DomainEvent::ServerUninstalled {
            space_id,
            server_id,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "uninstalled",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ServerConfigUpdated {
            space_id,
            server_id,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "config_updated",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ServerEnabled {
            space_id,
            server_id,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "enabled",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ServerDisabled {
            space_id,
            server_id,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "disabled",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),

        // Server status events
        DomainEvent::ServerStatusChanged {
            space_id,
            server_id,
            status,
            flow_id,
            has_connected_before,
            message,
            features,
        } => (
            "server-status-changed",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
                "status": status.as_str(),
                "flow_id": flow_id,
                "has_connected_before": has_connected_before,
                "message": message,
                "features": features.as_ref().map(|f| serde_json::json!({
                    "tools_count": f.tools.len(),
                    "prompts_count": f.prompts.len(),
                    "resources_count": f.resources.len(),
                })),
            }),
        ),
        DomainEvent::ServerAuthProgress {
            space_id,
            server_id,
            remaining_seconds,
            flow_id,
        } => (
            "server-auth-progress",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
                "remaining_seconds": remaining_seconds,
                "flow_id": flow_id,
            }),
        ),
        DomainEvent::ServerFeaturesRefreshed {
            space_id,
            server_id,
            features,
            added,
            removed,
        } => (
            "server-features-refreshed",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
                "tools_count": features.tools.len(),
                "prompts_count": features.prompts.len(),
                "resources_count": features.resources.len(),
                "added": added,
                "removed": removed,
            }),
        ),

        // Feature set events
        DomainEvent::FeatureSetCreated {
            space_id,
            feature_set_id,
            name,
            feature_set_type,
        } => (
            "feature-set-changed",
            serde_json::json!({
                "action": "created",
                "space_id": space_id,
                "feature_set_id": feature_set_id,
                "name": name,
                "feature_set_type": feature_set_type,
            }),
        ),
        DomainEvent::FeatureSetUpdated {
            space_id,
            feature_set_id,
            name,
        } => (
            "feature-set-changed",
            serde_json::json!({
                "action": "updated",
                "space_id": space_id,
                "feature_set_id": feature_set_id,
                "name": name,
            }),
        ),
        DomainEvent::FeatureSetDeleted {
            space_id,
            feature_set_id,
        } => (
            "feature-set-changed",
            serde_json::json!({
                "action": "deleted",
                "space_id": space_id,
                "feature_set_id": feature_set_id,
            }),
        ),
        DomainEvent::FeatureSetMembersChanged {
            space_id,
            feature_set_id,
            added_count,
            removed_count,
        } => (
            "feature-set-changed",
            serde_json::json!({
                "action": "members_changed",
                "space_id": space_id,
                "feature_set_id": feature_set_id,
                "added_count": added_count,
                "removed_count": removed_count,
            }),
        ),

        // Client events
        DomainEvent::ClientRegistered {
            client_id,
            client_name,
            registration_type,
        } => (
            "client-changed",
            serde_json::json!({
                "action": "registered",
                "client_id": client_id,
                "client_name": client_name,
                "registration_type": registration_type,
            }),
        ),
        DomainEvent::ClientReconnected {
            client_id,
            client_name,
        } => (
            "client-changed",
            serde_json::json!({
                "action": "reconnected",
                "client_id": client_id,
                "client_name": client_name,
            }),
        ),
        DomainEvent::ClientUpdated { client_id } => (
            "client-changed",
            serde_json::json!({
                "action": "updated",
                "client_id": client_id,
            }),
        ),
        DomainEvent::ClientDeleted { client_id } => (
            "client-changed",
            serde_json::json!({
                "action": "deleted",
                "client_id": client_id,
            }),
        ),
        DomainEvent::ClientTokenIssued { client_id } => (
            "client-changed",
            serde_json::json!({
                "action": "token_issued",
                "client_id": client_id,
            }),
        ),

        // Gateway events
        DomainEvent::GatewayStarted { url, port } => (
            "gateway-changed",
            serde_json::json!({
                "action": "started",
                "url": url,
                "port": port,
            }),
        ),
        DomainEvent::GatewayStopped => (
            "gateway-changed",
            serde_json::json!({
                "action": "stopped",
            }),
        ),

        // MCP capability notifications (informational)
        DomainEvent::ToolsChanged {
            space_id,
            server_id,
        } => (
            "mcp-notification",
            serde_json::json!({
                "type": "tools_changed",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::PromptsChanged {
            space_id,
            server_id,
        } => (
            "mcp-notification",
            serde_json::json!({
                "type": "prompts_changed",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ResourcesChanged {
            space_id,
            server_id,
        } => (
            "mcp-notification",
            serde_json::json!({
                "type": "resources_changed",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::MetaToolInvoked {
            client_id,
            session_id,
            tool_name,
            decision,
            resolved_feature_set_id,
            summary,
        } => (
            // New channel so the Connection Log can render a dedicated row
            // type without interleaving with regular backend events.
            "meta-tool-invoked",
            serde_json::json!({
                "client_id": client_id,
                "session_id": session_id,
                "tool_name": tool_name,
                "decision": decision,
                "resolved_feature_set_id": resolved_feature_set_id,
                "summary": summary,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        ),

        // Workspace binding write → tell the UI to re-load the bindings
        // table. The MCP `list_changed` notifications are handled separately
        // by MCPNotifier subscribing to the same event.
        DomainEvent::WorkspaceBindingChanged {
            space_id,
            workspace_root,
        } => (
            "workspace-binding-changed",
            serde_json::json!({
                "space_id": space_id,
                "workspace_root": workspace_root,
            }),
        ),

        // The set of live reported session roots changed — the Workspaces
        // tab re-fetches so unbound folders stay visible.
        DomainEvent::SessionRootsChanged => ("session-roots-changed", serde_json::json!({})),

        // A session resolved via `source=Default` and no binding exists for
        // any of its reported roots. Front-end shows the binding sheet.
        DomainEvent::WorkspaceNeedsBinding {
            client_id,
            session_id,
            space_id,
            workspace_root,
            collision_client_id,
            space_locked,
        } => (
            "workspace-needs-binding",
            serde_json::json!({
                "client_id": client_id,
                "session_id": session_id,
                "space_id": space_id,
                "workspace_root": workspace_root,
                "collision_client_id": collision_client_id,
                "space_locked": space_locked,
            }),
        ),

        // Per-client grant edited — Clients page re-fetches the toggles for
        // the affected client. MCPNotifier handles the corresponding
        // `list_changed` push to the client's open peers separately.
        DomainEvent::ClientGrantChanged {
            client_id,
            space_id,
        } => (
            "client-grant-changed",
            serde_json::json!({
                "client_id": client_id,
                "space_id": space_id,
            }),
        ),

        // A Space's built-in-server config changed. The gateway-side
        // MCPNotifier handles the `tools/list_changed` push to that Space's
        // MCP clients; this forwards it to the desktop UI so an open Built-in
        // Servers view for that Space reflects the change live.
        DomainEvent::BuiltinServerConfigChanged { space_id } => (
            "builtin-server-config-changed",
            serde_json::json!({ "space_id": space_id }),
        ),
        DomainEvent::WorkspaceAppearanceChanged { workspace_root } => (
            "workspace-appearance-changed",
            serde_json::json!({ "workspace_root": workspace_root }),
        ),

        DomainEvent::ServerVersionChecked {
            space_id,
            server_id,
        } => (
            "server-version-checked",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),

        DomainEvent::ServerUpdateAvailable {
            space_id,
            server_id,
            current_version,
            latest_version,
        } => (
            "server-update-available",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
                "current_version": current_version,
                "latest_version": latest_version,
            }),
        ),
    }
}

/// Wire OAuth consent notifications to the desktop webview and admin SSE bus.
pub async fn wire_consent_ui_notifications(
    app_handle: &AppHandle,
    gateway_state: &Arc<RwLock<mcpmux_gateway::GatewayState>>,
    ui_bus: Option<Arc<AdminUiEventBus>>,
) {
    let app = app_handle.clone();
    let ui_bus_for_hook = ui_bus.clone();
    let hook: mcpmux_gateway::ConsentUiNotifier = Arc::new(move |request_id: &str| {
        crate::services::ui_events::emit_ui_channel(
            &app,
            ui_bus_for_hook.as_deref(),
            OAUTH_CONSENT_REQUEST_CHANNEL,
            serde_json::json!({ "requestId": request_id }),
        );
    });

    gateway_state.write().await.set_consent_ui_hook(hook);
    info!(
        "[Gateway] Consent UI wired (Tauri + SSE={})",
        ui_bus.is_some()
    );
}

/// Create Gateway dependencies from app state using DI builder pattern
///
/// Centralizes dependency construction following Dependency Injection principles.
/// All external dependencies are explicitly injected, making the Gateway testable.
fn create_gateway_dependencies(
    app_state: &AppState,
    _app_handle: tauri::AppHandle,
) -> Result<mcpmux_gateway::GatewayDependencies, String> {
    // Load JWT signing secret (DPAPI on Windows, keychain elsewhere)
    let jwt_secret = match mcpmux_storage::create_jwt_secret_provider(app_state.data_dir()) {
        Ok(provider) => match provider.get_or_create_secret() {
            Ok(secret) => {
                info!("[Gateway] JWT signing secret loaded");
                Some(secret)
            }
            Err(e) => {
                warn!("[Gateway] Failed to load JWT secret: {}", e);
                None
            }
        },
        Err(e) => {
            warn!("[Gateway] Failed to create JWT secret provider: {}", e);
            None
        }
    };

    // Build dependencies using builder pattern (DI)
    let mut builder = mcpmux_gateway::DependenciesBuilder::new()
        .with_installed_server_repo(app_state.installed_server_repository.clone())
        .with_credential_repo(app_state.credential_repository.clone())
        .with_backend_oauth_repo(app_state.backend_oauth_repository.clone())
        .with_feature_repo(app_state.server_feature_repository_core.clone())
        .with_feature_set_repo(app_state.feature_set_repository.clone())
        .with_server_discovery(app_state.server_discovery.clone())
        .with_log_manager(app_state.server_log_manager.clone())
        .with_database(app_state.database())
        .with_state_dir(app_state.data_dir().to_path_buf())
        .with_settings_repo(app_state.settings_repository.clone());

    if let Some(secret) = jwt_secret {
        builder = builder.with_jwt_secret(secret);
    }

    builder.build().map_err(|e: String| e)
}

/// Get gateway status, optionally scoped to a specific space
#[tauri::command]
pub async fn get_gateway_status(
    space_id: Option<String>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    server_manager_state: State<'_, Arc<RwLock<ServerManagerState>>>,
) -> Result<GatewayStatus, String> {
    let state = gateway_state.read().await;

    let active_sessions = if let Some(ref gw_state) = state.gateway_state {
        let gw = gw_state.read().await;
        gw.sessions.len()
    } else {
        0
    };

    // Get connected count from ServerManager, scoped to space if provided
    let connected_backends = {
        let sm_state = server_manager_state.read().await;
        if let Some(ref manager) = sm_state.manager {
            if let Some(ref sid) = space_id {
                let uuid = Uuid::parse_str(sid).map_err(|e| e.to_string())?;
                manager.connected_count_for_space(&uuid).await
            } else {
                manager.connected_count().await
            }
        } else {
            0
        }
    };

    // DEBUG, not INFO: the UI polls this on a timer and on every domain event,
    // so at INFO it floods the log (several lines/sec) and drowns the events
    // that matter. The data is still available with `RUST_LOG=debug`.
    debug!(
        "[Gateway] get_gateway_status: running={}, url={:?}, sessions={}, backends={}, space={:?}",
        state.running, state.url, active_sessions, connected_backends, space_id
    );

    Ok(GatewayStatus {
        running: state.running,
        url: state.url.clone(),
        active_sessions,
        connected_backends,
    })
}

/// Start the gateway server.
///
/// `port` forces a specific port (used for ad-hoc overrides from a test or
/// power-user flow). When `port` is None, the preferred port is whatever
/// the user has configured, falling back to the shipped default.
///
/// `allow_dynamic_fallback` controls what happens when the preferred port
/// is busy:
/// - **None / false (strict, default):** return an error prefixed with
///   `PORT_IN_USE:<port>:<source>`. The UI should probe first and prompt
///   the user before retrying with fallback enabled.
/// - **true:** silently allocate an OS-assigned port instead. Used by the
///   auto-start path where there's no UI to prompt.
#[tauri::command]
pub async fn start_gateway(
    port: Option<u16>,
    allow_dynamic_fallback: Option<bool>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    sm_state: State<'_, Arc<RwLock<ServerManagerState>>>,
    app_state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let mut state = gateway_state.write().await;

    if state.running {
        return Err("Gateway is already running".to_string());
    }

    let (preferred_port, source) = resolve_preferred_port(&app_state, port).await;
    let allow_fallback = allow_dynamic_fallback.unwrap_or(false);

    let final_port = if is_port_available(preferred_port) {
        // Persist first-run default so the Settings UI shows it explicitly.
        if matches!(source, PortSource::Default)
            && app_state
                .gateway_port_service
                .load_persisted_port()
                .await
                .is_none()
        {
            if let Err(e) = app_state
                .gateway_port_service
                .save_port(preferred_port)
                .await
            {
                warn!("[Gateway] Failed to persist default port: {}", e);
            }
        }
        preferred_port
    } else if allow_fallback {
        let dyn_port = allocate_dynamic_port().map_err(|e| e.to_string())?;
        warn!(
            "[Gateway] Preferred port {} unavailable, falling back to dynamic port {} (not persisted — next start retries {})",
            preferred_port, dyn_port, preferred_port
        );
        // Intentionally do NOT persist the fallback port — the user's
        // configured/default preference must survive so the next launch
        // retries it. Persisting here would silently overwrite what the
        // Settings page shows.
        dyn_port
    } else {
        // Strict mode — caller must retry with allow_dynamic_fallback=true or
        // free the port. The UI parses this sentinel to render its popup.
        return Err(format!(
            "PORT_IN_USE:{}:{}",
            preferred_port,
            source.as_str()
        ));
    };

    let public_base_url = load_public_base_url(&app_state).await;
    let url = advertised_base_url(public_base_url.as_deref(), final_port);
    let local_url = format!("http://localhost:{}", final_port);

    info!("Starting gateway on {} (advertising {})", local_url, url);

    // Create dependencies using DI builder pattern
    let dependencies = create_gateway_dependencies(&app_state, app_handle.clone())?;

    // Bind all interfaces when the user opted into network access so other
    // devices on the LAN can reach the gateway; loopback-only otherwise.
    let network_access = load_network_access(&app_state).await;

    // Create gateway config
    let config = mcpmux_gateway::GatewayConfig {
        host: bind_host_for(network_access).to_string(),
        port: final_port,
        public_base_url: public_base_url.clone(),
        enable_cors: true,
    };

    // Create self-contained gateway server with DI
    // Gateway will auto-initialize all services and auto-connect enabled servers
    let server = mcpmux_gateway::GatewayServer::new(config, dependencies);

    // Get references to services before spawning
    let gw_state = server.state();
    let pool_service = server.pool_service();
    let feature_service = server.feature_service();
    let event_emitter = server.event_emitter();
    let server_manager = server.server_manager();
    let grant_service = server.grant_service();
    let session_roots = server.session_roots();

    // Seed the system-wide inbound-auth toggle into the running gateway from
    // persisted settings (default: auth required). Live changes go through
    // `set_gateway_auth_disabled`.
    {
        let disabled = app_state
            .settings_repository
            .get(GATEWAY_AUTH_DISABLED_KEY)
            .await
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);
        if disabled {
            gw_state.write().await.set_auth_disabled(true);
        }
    }

    // Subscribe to OAuth completions BEFORE spawn so we don't miss early
    // events emitted during initial auto-connect.
    let oauth_completion_rx = pool_service.oauth_manager().subscribe();
    info!(
        "[Gateway] Services resolved — port={}, server_manager={:p}",
        final_port, &*server_manager
    );

    // Meta-tool approval broker — attach a Tauri-event publisher so
    // incoming approval requests reach the React dialog.
    let approval_broker = server.approval_broker();
    attach_approval_publisher(&approval_broker, app_handle.clone()).await;

    // Start domain event bridge (clean architecture)
    start_domain_event_bridge(&app_handle, gw_state.clone());

    // Wire ServerManager into state + spawn OAuth handler + periodic
    // refresh. MUST happen here, otherwise the Servers page sees every
    // server stuck on "Connecting..." because `get_server_statuses` can't
    // reach the ServerManager.
    let sm_state_inner: Arc<RwLock<ServerManagerState>> = sm_state.inner().clone();
    init_gateway_runtime(
        pool_service.clone(),
        server_manager.clone(),
        oauth_completion_rx,
        sm_state_inner,
    )
    .await;

    // Spawn gateway (runs in background, auto-connects servers)
    let handle = server.spawn();

    info!(
        "[Gateway] Setting GatewayAppState fields — port={}, url={}",
        final_port, url
    );
    state.running = true;
    state.url = Some(url.clone());
    state.bound_port = Some(final_port);
    state.handle = Some(handle);
    state.gateway_state = Some(gw_state);
    state.pool_service = Some(pool_service);
    state.feature_service = Some(feature_service);
    state.event_emitter = Some(event_emitter);
    state.grant_service = Some(grant_service);
    state.approval_broker = Some(approval_broker);
    state.session_roots = Some(session_roots);
    info!(
        "[Gateway] Started — url={}, event_emitter={}, grant_service={}",
        url,
        state.event_emitter.is_some(),
        state.grant_service.is_some()
    );

    // Notify every frontend subscriber (status-bar footer, Dashboard,
    // Servers page, Settings). Without this, only the caller sees the new
    // URL; the footer would stay on "Gateway: Stopped" until the user
    // changes Space and retriggers a manual reload.
    if let Err(e) = app_handle.emit(
        "gateway-changed",
        serde_json::json!({
            "action": "started",
            "url": url,
            "port": final_port,
        }),
    ) {
        warn!("[Gateway] Failed to emit gateway-changed(started): {}", e);
    }

    // Sync admin server health endpoint and register SSE stream.
    {
        use crate::services::admin_server::{register_gateway_sse, set_gateway_running};
        let admin_state: tauri::State<
            '_,
            Arc<tokio::sync::RwLock<crate::services::AdminServerState>>,
        > = app_handle.state();
        let guard = admin_state.read().await;
        set_gateway_running(&guard, true);
        if let Some(gw_state) = state.gateway_state.clone() {
            let admin_guard_clone = admin_state.clone();
            let gw_state_clone = gw_state;
            drop(guard);
            let guard2 = admin_guard_clone.read().await;
            register_gateway_sse(&guard2, &gw_state_clone).await;
            wire_consent_ui_notifications(
                &app_handle,
                &gw_state_clone,
                Some(guard2.ui_event_bus.clone()),
            )
            .await;
        }
    }

    Ok(url)
}

/// Stop the gateway server
#[tauri::command]
pub async fn stop_gateway(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Take the handle out under the lock, then drop the guard BEFORE
    // awaiting the shutdown — otherwise the lock is held for up to 2s
    // and every concurrent status query blocks.
    let handle = {
        let mut state = gateway_state.write().await;
        if !state.running {
            return Err("Gateway is not running".to_string());
        }
        let handle = state.handle.take();
        state.running = false;
        state.url = None;
        state.bound_port = None;
        handle
    };

    if let Some(h) = handle {
        info!("[Gateway] Stop requested — shutting down gracefully");
        shutdown_gateway_handle(h).await;
    }

    if let Err(e) = app_handle.emit("gateway-changed", serde_json::json!({"action": "stopped"})) {
        warn!("[Gateway] Failed to emit gateway-changed(stopped): {}", e);
    }

    // Sync admin server health endpoint and clear SSE stream.
    {
        use crate::services::admin_server::{clear_gateway_sse, set_gateway_running};
        let admin_state: tauri::State<
            '_,
            Arc<tokio::sync::RwLock<crate::services::AdminServerState>>,
        > = app_handle.state();
        let guard = admin_state.read().await;
        set_gateway_running(&guard, false);
        clear_gateway_sse(&guard).await;
    }

    Ok(())
}

/// Gateway port configuration response.
///
/// - `configured_port` is the user's persisted override (None = "follow default").
/// - `default_port` is the built-in default the app ships with.
/// - `active_port` is the port the currently-running gateway is bound to
///   (None when stopped). When it differs from `configured_port`, the UI
///   should nudge the user to restart the gateway.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayPortSettings {
    pub configured_port: Option<u16>,
    pub default_port: u16,
    pub active_port: Option<u16>,
}

fn parse_port_from_url(url: &str) -> Option<u16> {
    // URL shape is always "http://localhost:PORT" — parse defensively.
    let after_scheme = url.split("://").nth(1)?;
    let host_port = after_scheme.split('/').next()?;
    host_port.rsplit(':').next()?.parse().ok()
}

/// Get the persisted gateway port setting, plus the currently-active port.
#[tauri::command]
pub async fn get_gateway_port_settings(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    app_state: State<'_, AppState>,
) -> Result<GatewayPortSettings, String> {
    let configured_port = app_state.gateway_port_service.load_persisted_port().await;

    let active_port = {
        let state = gateway_state.read().await;
        state
            .bound_port
            .or_else(|| state.url.as_deref().and_then(parse_port_from_url))
    };

    Ok(GatewayPortSettings {
        configured_port,
        default_port: mcpmux_core::DEFAULT_GATEWAY_PORT,
        active_port,
    })
}

/// Persist a custom gateway port. Takes effect on the next gateway start.
///
/// Does NOT touch a running gateway — the UI is expected to offer a
/// "Restart gateway" action. The port must be in the user-space range
/// (1024–65535). Ports ≤ 1023 are rejected to avoid privileged-port
/// surprises on Unix.
#[tauri::command]
pub async fn set_gateway_port(port: u16, app_state: State<'_, AppState>) -> Result<(), String> {
    if port < 1024 {
        return Err(format!(
            "Port {} is in the privileged range (≤ 1023). Choose a port between 1024 and 65535.",
            port
        ));
    }

    app_state
        .gateway_port_service
        .save_port(port)
        .await
        .map_err(|e| e.to_string())?;

    info!("[Gateway] Persisted custom gateway port: {}", port);
    Ok(())
}

/// Clear the persisted gateway port override. The next gateway start will
/// use the built-in default (or a dynamically-allocated port if the default
/// is in use).
#[tauri::command]
pub async fn reset_gateway_port(app_state: State<'_, AppState>) -> Result<(), String> {
    app_state
        .gateway_port_service
        .clear_persisted_port()
        .await
        .map_err(|e| e.to_string())?;

    info!("[Gateway] Cleared persisted gateway port — reverting to default on next start");
    Ok(())
}

/// App-settings key for the system-wide inbound-auth toggle. Stored as
/// `"true"`/`"false"`; missing means auth is required (the secure default).
pub const GATEWAY_AUTH_DISABLED_KEY: &str = "gateway.auth_disabled";

/// Whether inbound MCP authentication is disabled — connections are accepted
/// without an access key (localhost-only convenience). Default **false** (auth
/// required).
#[tauri::command]
pub async fn get_gateway_auth_disabled(app_state: State<'_, AppState>) -> Result<bool, String> {
    let stored = app_state
        .settings_repository
        .get(GATEWAY_AUTH_DISABLED_KEY)
        .await
        .map_err(|e| e.to_string())?;
    Ok(stored.map(|v| v == "true").unwrap_or(false))
}

/// Enable/disable system-wide inbound auth. Persists the setting AND mirrors it
/// into the running gateway so the change takes effect immediately (no
/// restart). When the gateway isn't running it's a no-op beyond persistence —
/// `start_gateway` seeds the value on launch.
#[tauri::command]
pub async fn set_gateway_auth_disabled(
    disabled: bool,
    app_state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<bool, String> {
    app_state
        .settings_repository
        .set(GATEWAY_AUTH_DISABLED_KEY, &disabled.to_string())
        .await
        .map_err(|e| e.to_string())?;

    let state = gateway_state.read().await;
    if let Some(ref gw) = state.gateway_state {
        gw.write().await.set_auth_disabled(disabled);
    }
    info!("[Gateway] Inbound auth disabled set to {}", disabled);
    Ok(disabled)
}

/// Public URL configuration response.
///
/// `configured_public_base_url` is the persisted external origin advertised in
/// OAuth metadata. `active_public_base_url` is the URL currently advertised by
/// the running gateway, and `local_base_url` is the localhost listener.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayPublicUrlSettings {
    pub configured_public_base_url: Option<String>,
    pub active_public_base_url: Option<String>,
    pub local_base_url: Option<String>,
}

#[tauri::command]
pub async fn get_gateway_public_url_settings(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    app_state: State<'_, AppState>,
) -> Result<GatewayPublicUrlSettings, String> {
    let configured_public_base_url = load_public_base_url(&app_state).await;
    let (active_public_base_url, local_base_url) = {
        let state = gateway_state.read().await;
        (
            if state.running {
                state.url.clone()
            } else {
                None
            },
            state
                .bound_port
                .map(|port| format!("http://localhost:{}", port)),
        )
    };

    Ok(GatewayPublicUrlSettings {
        configured_public_base_url,
        active_public_base_url,
        local_base_url,
    })
}

/// Persist the public base URL advertised in OAuth metadata.
///
/// Pass `None` or an empty string to return to local-only localhost metadata.
/// Non-empty values must be HTTPS origins, e.g. `https://mcp.example.com`.
#[tauri::command]
pub async fn set_gateway_public_base_url(
    public_base_url: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let normalized = match public_base_url.as_deref() {
        Some(raw) => normalize_public_base_url(raw)?,
        None => None,
    };

    match normalized {
        Some(url) => {
            app_state
                .settings_repository
                .set(GATEWAY_PUBLIC_BASE_URL_KEY, &url)
                .await
                .map_err(|e| e.to_string())?;
            info!("[Gateway] Persisted public base URL: {}", url);
        }
        None => {
            app_state
                .settings_repository
                .delete(GATEWAY_PUBLIC_BASE_URL_KEY)
                .await
                .map_err(|e| e.to_string())?;
            info!("[Gateway] Cleared public base URL — reverting to local-only metadata");
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn reset_gateway_public_base_url(app_state: State<'_, AppState>) -> Result<(), String> {
    app_state
        .settings_repository
        .delete(GATEWAY_PUBLIC_BASE_URL_KEY)
        .await
        .map_err(|e| e.to_string())?;

    info!("[Gateway] Cleared public base URL — reverting to local-only metadata");
    Ok(())
}

/// Whether the gateway is configured to bind all network interfaces (`0.0.0.0`).
#[tauri::command]
pub async fn get_gateway_network_access(app_state: State<'_, AppState>) -> Result<bool, String> {
    Ok(load_network_access(&app_state).await)
}

/// Enable or disable binding the gateway to all interfaces (`0.0.0.0`) so other
/// devices on the network can reach it. Off (default) keeps it on `127.0.0.1`
/// (this machine only). Restart the gateway for the change to take effect.
#[tauri::command]
pub async fn set_gateway_network_access(
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    app_state
        .settings_repository
        .set(
            GATEWAY_NETWORK_ACCESS_KEY,
            if enabled { "true" } else { "false" },
        )
        .await
        .map_err(|e| e.to_string())?;

    if enabled {
        info!("[Gateway] Network access enabled — will bind 0.0.0.0 on next start/restart");
    } else {
        info!("[Gateway] Network access disabled — will bind 127.0.0.1 on next start/restart");
    }
    Ok(())
}

/// Which port source a startup attempt would use.
///
/// Kept as a string-valued enum for clean JSON serialization to the UI.
#[derive(Debug, Clone, Copy)]
enum PortSource {
    Override,
    Configured,
    Default,
}

impl PortSource {
    fn as_str(self) -> &'static str {
        match self {
            PortSource::Override => "override",
            PortSource::Configured => "configured",
            PortSource::Default => "default",
        }
    }
}

/// Result of probing whether the gateway can start on its preferred port.
///
/// - `preferred_port` is the port that _would_ be used — explicit override
///   wins over configured persisted port, which wins over the shipped default.
/// - `preferred_available` is false when something else is bound to it.
/// - `source` tells the UI which tier was chosen, so messages can reference
///   "your configured port" vs. "the default port".
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStartProbe {
    pub preferred_port: u16,
    pub preferred_available: bool,
    pub source: &'static str,
}

async fn resolve_preferred_port(
    app_state: &AppState,
    explicit_port: Option<u16>,
) -> (u16, PortSource) {
    if let Some(p) = explicit_port {
        return (p, PortSource::Override);
    }
    if let Some(p) = app_state.gateway_port_service.load_persisted_port().await {
        return (p, PortSource::Configured);
    }
    (mcpmux_core::DEFAULT_GATEWAY_PORT, PortSource::Default)
}

/// Probe whether the gateway's preferred port is free, without starting it.
///
/// Frontends should call this before invoking `start_gateway` so they can
/// prompt the user when a fallback would be required.
#[tauri::command]
pub async fn probe_gateway_start(
    port: Option<u16>,
    app_state: State<'_, AppState>,
) -> Result<GatewayStartProbe, String> {
    let (preferred_port, source) = resolve_preferred_port(&app_state, port).await;
    let preferred_available = is_port_available(preferred_port);
    Ok(GatewayStartProbe {
        preferred_port,
        preferred_available,
        source: source.as_str(),
    })
}

/// Atomically read **and clear** any deferred auto-start port conflict.
///
/// The "take" semantic matters: React StrictMode double-mounts components
/// in dev, and without atomic consumption both mounts would read the same
/// conflict and double-prompt the user. Only the first caller wins.
#[tauri::command]
pub async fn take_pending_port_conflict(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<Option<PendingPortConflict>, String> {
    let mut state = gateway_state.write().await;
    Ok(state.pending_port_conflict.take())
}

/// Restart the gateway server.
///
/// Both `port` and `allow_dynamic_fallback` are forwarded to `start_gateway`
/// — see its docs for semantics.
#[tauri::command]
pub async fn restart_gateway(
    port: Option<u16>,
    allow_dynamic_fallback: Option<bool>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    sm_state: State<'_, Arc<RwLock<ServerManagerState>>>,
    app_state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    info!("[Gateway] Restart requested — tearing down current state");
    // Take handle out under lock; drop lock before awaiting shutdown so
    // start_gateway below can re-acquire it.
    let handle = {
        let mut state = gateway_state.write().await;
        let handle = state.handle.take();
        state.running = false;
        state.url = None;
        state.bound_port = None;
        handle
    };
    if let Some(h) = handle {
        shutdown_gateway_handle(h).await;
    }

    // Start with new config
    start_gateway(
        port,
        allow_dynamic_fallback,
        gateway_state,
        sm_state,
        app_state,
        app_handle,
    )
    .await
}

/// Generate gateway config for a client
#[tauri::command]
pub async fn generate_gateway_config(
    client_type: String,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<String, String> {
    let state = gateway_state.read().await;

    let url = state.url.as_ref().ok_or("Gateway is not running")?;

    // Use branding constant for MCP config key
    let config_key = mcpmux_core::branding::MCP_CONFIG_KEY;

    let config = match client_type.as_str() {
        "cursor" => {
            serde_json::json!({
                "mcpServers": {
                    (config_key): {
                        "url": url,
                        "transport": "streamable-http"
                    }
                }
            })
        }
        "claude" => {
            serde_json::json!({
                "mcpServers": {
                    (config_key): {
                        "url": url,
                        "transport": "sse"
                    }
                }
            })
        }
        _ => {
            serde_json::json!({
                "mcpServers": {
                    (config_key): {
                        "url": url
                    }
                }
            })
        }
    };

    serde_json::to_string_pretty(&config).map_err(|e| e.to_string())
}

/// Disconnect a server from the gateway
#[tauri::command]
pub async fn disconnect_server(
    server_id: String,
    space_id: String,
    logout: Option<bool>,
    app_state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    server_manager_state: State<'_, Arc<RwLock<ServerManagerState>>>,
) -> Result<(), String> {
    info!(
        "[Gateway] Disconnecting server: {} from space: {} (logout: {:?})",
        server_id, space_id, logout
    );

    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    // Get pool service
    let state = gateway_state.read().await;
    let pool_service = state.pool_service.clone();

    // Note: Server state is managed by ServerManager, not GatewayState
    drop(state);

    // Disconnect from pool (clears tokens, marks features unavailable)
    if let Some(pool) = pool_service {
        pool.disconnect_server(space_uuid, &server_id)
            .await
            .map_err(|e| e.to_string())?;
    }

    // If logout requested, ensure OAuth tokens are cleared
    // (PoolService.disconnect_server already does this, but be explicit for logout)
    if logout.unwrap_or(false) {
        match app_state
            .credential_repository
            .clear_tokens(&space_uuid, &server_id)
            .await
        {
            Ok(true) => {
                info!(
                    "[Gateway] Cleared OAuth tokens for server: {} (client registration preserved)",
                    server_id
                );
            }
            Ok(false) => {
                info!(
                    "[Gateway] No credentials to clear for server: {}",
                    server_id
                );
            }
            Err(e) => {
                warn!("[Gateway] Failed to clear tokens for {}: {}", server_id, e);
            }
        }
    }

    // Update ServerManager state and emit event
    let sm_state = server_manager_state.read().await;
    if let Some(manager) = sm_state.manager.as_ref() {
        let key = ServerKey::new(space_uuid, &server_id);
        // If logout, set to auth_required so Connect button shows; otherwise just disconnected
        if logout.unwrap_or(false) {
            manager.set_auth_required(&key, None).await;
        } else {
            manager.set_disconnected(&key).await;
        }
    }
    drop(sm_state);

    info!("[Gateway] Server {} disconnected successfully", server_id);
    Ok(())
}

/// List connected backend servers
///
/// Note: Server state is now tracked by ServerManager, accessed via server_manager commands
#[tauri::command]
pub async fn list_connected_servers(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<Vec<BackendStatusResponse>, String> {
    let state = gateway_state.read().await;

    // Return empty list - ServerManager now handles server state
    // Use server_manager::get_all_server_statuses for actual status
    let _ = state; // Suppress warning
    Ok(vec![])
}

/// Result of bulk connection operation
#[derive(Debug, Serialize)]
pub struct BulkConnectResult {
    /// Successfully connected (new instances)
    pub connected: usize,
    /// Reused existing instances
    pub reused: usize,
    /// Failed to connect
    pub failed: usize,
    /// Require OAuth authentication
    pub oauth_required: usize,
    /// Error details for failed connections
    pub errors: Vec<String>,
}

/// Connect all enabled servers from all spaces.
/// This is used on gateway startup to auto-connect everything.
#[tauri::command]
pub async fn connect_all_enabled_servers(
    app_state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<BulkConnectResult, String> {
    info!("[Gateway] Connecting all enabled servers from all spaces");

    // Check if gateway is running
    let state = gateway_state.read().await;
    if !state.running {
        return Err("Gateway is not running".to_string());
    }
    let pool_service = state
        .pool_service
        .clone()
        .ok_or("Pool service not initialized")?;
    drop(state);

    // Get all spaces
    let spaces = app_state
        .space_service
        .list()
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;

    // Build list of servers to connect
    let mut servers_to_connect: Vec<(
        InstalledServerInfo,
        ResolvedTransport,
        mcpmux_core::ServerDefinition,
        mcpmux_core::InstalledServer,
    )> = vec![];

    for space in &spaces {
        let space_id_str = space.id.to_string();

        // Get enabled servers for this space
        let installed_servers = app_state
            .installed_server_repository
            .list_enabled(&space_id_str)
            .await
            .map_err(|e| e.to_string())?;

        for installed in installed_servers {
            // Use cached definition from InstalledServer (offline-first approach)
            // No need to hit registry API - everything is stored locally at install time
            let server_definition = match installed.get_definition() {
                Some(def) => def,
                None => {
                    warn!(
                        "[Gateway] Skipping {}: no cached definition (installed before offline support)",
                        installed.server_id
                    );
                    // Try to backfill from registry if available
                    if let Some(def) = app_state.server_discovery.get(&installed.server_id).await {
                        // Note: This won't persist - server needs to be reinstalled for full offline support
                        def
                    } else {
                        continue;
                    }
                }
            };

            // Check if has OAuth credentials (access token)
            let has_credentials = matches!(
                app_state
                    .credential_repository
                    .get(
                        &space.id,
                        &installed.server_id,
                        &mcpmux_core::CredentialType::AccessToken
                    )
                    .await,
                Ok(Some(_))
            );

            // Determine if server requires OAuth
            let requires_oauth = matches!(
                server_definition.auth,
                Some(mcpmux_core::domain::AuthConfig::Oauth)
            );

            let server_info = InstalledServerInfo {
                space_id: space.id,
                server_id: installed.server_id.clone(),
                requires_oauth,
                has_credentials,
            };

            let transport = mcpmux_gateway::pool::transport::resolution::build_transport_config(
                &server_definition.transport,
                &installed,
                Some(app_state.data_dir()),
                mcpmux_gateway::pool::transport::resolution::TransportResolutionOptions::default(),
            );

            servers_to_connect.push((server_info, transport, server_definition, installed));
        }
    }

    info!(
        "[Gateway] Prepared {} server connection requests across {} spaces",
        servers_to_connect.len(),
        spaces.len()
    );

    // Connect servers one by one and track results
    let mut result = BulkConnectResult {
        connected: 0,
        reused: 0,
        failed: 0,
        oauth_required: 0,
        errors: vec![],
    };

    for (server_info, transport, _server_definition, _installed) in servers_to_connect {
        let space_uuid = server_info.space_id;
        let server_id = server_info.server_id.clone();

        let ctx = ConnectionContext::new(space_uuid, server_id.clone(), transport);
        match pool_service.connect_server(&ctx).await {
            ConnectionResult::Connected { reused, features } => {
                if reused {
                    result.reused += 1;
                } else {
                    result.connected += 1;
                }

                info!(
                    "[Gateway] Connected {} (reused: {}, features: {})",
                    server_id,
                    reused,
                    features.total_count()
                );
            }
            ConnectionResult::OAuthRequired { auth_url: _ } => {
                result.oauth_required += 1;
            }
            ConnectionResult::Failed { error } => {
                result.failed += 1;
                result.errors.push(format!("{}: {}", server_id, error));
            }
        }
    }

    info!(
        "[Gateway] Bulk connect complete: {} connected, {} reused, {} failed, {} need OAuth",
        result.connected, result.reused, result.failed, result.oauth_required
    );

    Ok(result)
}

/// Get pool statistics
#[tauri::command]
pub async fn get_pool_stats(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<PoolStatsResponse, String> {
    let state = gateway_state.read().await;

    let stats = match &state.pool_service {
        Some(pool) => pool.stats(),
        None => mcpmux_gateway::PoolStats::default(),
    };

    Ok(PoolStatsResponse {
        total_instances: stats.total_instances,
        connected_instances: stats.connected_instances,
        total_space_server_mappings: stats.connecting_instances
            + stats.failed_instances
            + stats.oauth_pending_instances,
    })
}

/// Refresh OAuth tokens on startup for all installed HTTP servers.
///
/// NOTE: This is now a no-op. RMCP's AuthClient handles token refresh automatically
/// per-request via DatabaseCredentialStore. Keeping this command for API compatibility.
#[tauri::command]
pub async fn refresh_oauth_tokens_on_startup(
    _app_state: State<'_, AppState>,
) -> Result<RefreshResult, String> {
    info!("[OAuth] Token refresh handled automatically by RMCP per-request. No startup refresh needed.");

    Ok(RefreshResult {
        servers_checked: 0,
        tokens_refreshed: 0,
        refresh_failed: 0,
    })
}

/// Result of OAuth token refresh operation
#[derive(Debug, Serialize)]
pub struct RefreshResult {
    /// Number of servers checked
    pub servers_checked: usize,
    /// Number of tokens successfully refreshed
    pub tokens_refreshed: usize,
    /// Number of refresh attempts that failed
    pub refresh_failed: usize,
}

/// Pool statistics response
#[derive(Debug, Serialize)]
pub struct PoolStatsResponse {
    pub total_instances: usize,
    pub connected_instances: usize,
    pub total_space_server_mappings: usize,
}

/// Reload (or stop-then-start) the web admin server based on current settings.
///
/// Call this after the user toggles `gateway.admin_enabled`, changes the admin
/// port, or modifies Cloudflare Access settings so the admin server picks up the
/// new configuration without requiring a full app restart.
#[tauri::command]
pub async fn reload_admin_server(
    app_handle: tauri::AppHandle,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    server_manager_state: State<'_, Arc<RwLock<ServerManagerState>>>,
) -> Result<(), String> {
    let admin_state: tauri::State<'_, Arc<tokio::sync::RwLock<crate::services::AdminServerState>>> =
        app_handle.state();
    let event_bus = mcpmux_core::create_shared_event_bus();
    crate::services::admin_server::reload_admin_server(
        app_handle.clone(),
        admin_state.inner().clone(),
        gateway_state.inner().clone(),
        server_manager_state.inner().clone(),
        event_bus,
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod public_base_url_tests {
    use super::{advertised_base_url, normalize_public_base_url};

    #[test]
    fn normalize_accepts_https_origin_and_trims_trailing_slash() {
        assert_eq!(
            normalize_public_base_url("https://mcp.example.com/").unwrap(),
            Some("https://mcp.example.com".to_string())
        );
        assert_eq!(
            normalize_public_base_url("https://mcp.example.com:8443").unwrap(),
            Some("https://mcp.example.com:8443".to_string())
        );
    }

    #[test]
    fn normalize_treats_blank_as_none() {
        assert_eq!(normalize_public_base_url("").unwrap(), None);
        assert_eq!(normalize_public_base_url("   ").unwrap(), None);
    }

    #[test]
    fn normalize_rejects_unsafe_or_non_origin_urls() {
        // Non-https, credentials, query/fragment, non-root path, and garbage all rejected.
        assert!(normalize_public_base_url("http://mcp.example.com").is_err());
        assert!(normalize_public_base_url("https://user:pass@mcp.example.com").is_err());
        assert!(normalize_public_base_url("https://mcp.example.com/?x=1").is_err());
        assert!(normalize_public_base_url("https://mcp.example.com/#frag").is_err());
        assert!(normalize_public_base_url("https://mcp.example.com/mcp").is_err());
        assert!(normalize_public_base_url("not a url").is_err());
    }

    #[test]
    fn advertised_base_url_falls_back_to_localhost() {
        assert_eq!(advertised_base_url(None, 45818), "http://localhost:45818");
        assert_eq!(
            advertised_base_url(Some("   "), 45818),
            "http://localhost:45818"
        );
        assert_eq!(
            advertised_base_url(Some("https://mcp.example.com/"), 45818),
            "https://mcp.example.com"
        );
    }

    #[test]
    fn bind_host_for_maps_network_access_to_address() {
        assert_eq!(super::bind_host_for(false), "127.0.0.1");
        assert_eq!(super::bind_host_for(true), "0.0.0.0");
    }
}
