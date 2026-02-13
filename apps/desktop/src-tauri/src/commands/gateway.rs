//! Gateway management commands
//!
//! IPC commands for controlling the local MCP gateway server.

use crate::commands::server_manager::ServerManagerState;
use crate::AppState;
use mcpmux_core::DomainEvent;
use mcpmux_gateway::{
    ConnectionContext, ConnectionResult, FeatureService, InstalledServerInfo, PoolService,
    ResolvedTransport, ServerKey,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use tracing::{error, info, trace, warn};
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

/// Gateway state managed by Tauri
#[derive(Default)]
pub struct GatewayAppState {
    /// Gateway running flag
    pub running: bool,
    /// Gateway URL
    pub url: Option<String>,
    /// Gateway task handle
    pub handle: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
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

            // Map domain events to UI channels
            let (channel, payload) = map_domain_event_to_ui(&event);

            trace!(
                event_type = event_type,
                channel = channel,
                "[Gateway] Forwarding domain event to UI"
            );

            if let Err(e) = app_handle_clone.emit(channel, payload) {
                error!("[Gateway] Failed to emit {} event: {}", channel, e);
            }
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
        DomainEvent::SpaceActivated {
            from_space_id,
            to_space_id,
            to_space_name,
        } => (
            "space-changed",
            serde_json::json!({
                "action": "activated",
                "from_space_id": from_space_id,
                "to_space_id": to_space_id,
                "to_space_name": to_space_name,
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

        // Grant events
        DomainEvent::GrantIssued {
            client_id,
            space_id,
            feature_set_id,
        } => (
            "grants-changed",
            serde_json::json!({
                "action": "granted",
                "client_id": client_id,
                "space_id": space_id,
                "feature_set_id": feature_set_id,
            }),
        ),
        DomainEvent::GrantRevoked {
            client_id,
            space_id,
            feature_set_id,
        } => (
            "grants-changed",
            serde_json::json!({
                "action": "revoked",
                "client_id": client_id,
                "space_id": space_id,
                "feature_set_id": feature_set_id,
            }),
        ),
        DomainEvent::ClientGrantsUpdated {
            client_id,
            space_id,
            feature_set_ids,
        } => (
            "grants-changed",
            serde_json::json!({
                "action": "batch_updated",
                "client_id": client_id,
                "space_id": space_id,
                "feature_set_ids": feature_set_ids,
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
    }
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

    info!(
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

/// Start the gateway server
#[tauri::command]
pub async fn start_gateway(
    port: Option<u16>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    app_state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let mut state = gateway_state.write().await;

    if state.running {
        return Err("Gateway is already running".to_string());
    }

    // Single Responsibility: Delegate port resolution to GatewayPortService
    let final_port = app_state
        .gateway_port_service
        .resolve_with_override(port)
        .await
        .map_err(|e| e.to_string())?;

    let url = format!("http://localhost:{}", final_port);

    info!("Starting gateway on {}", url);

    // Create dependencies using DI builder pattern
    let dependencies = create_gateway_dependencies(&app_state, app_handle.clone())?;

    // Create gateway config
    let config = mcpmux_gateway::GatewayConfig {
        host: "127.0.0.1".to_string(), // Bind address must be IP
        port: final_port,
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

    info!("[Gateway] Getting grant_service from server...");
    let grant_service = server.grant_service();
    info!("[Gateway] Got grant_service: {:p}", &*grant_service);

    // Start domain event bridge (clean architecture)
    start_domain_event_bridge(&app_handle, gw_state.clone());

    // Spawn gateway (runs in background, auto-connects servers)
    let handle = server.spawn();

    info!("[Gateway] Setting state fields...");
    state.running = true;
    state.url = Some(url.clone());
    state.handle = Some(handle);
    state.gateway_state = Some(gw_state);
    state.pool_service = Some(pool_service);
    state.feature_service = Some(feature_service);
    state.event_emitter = Some(event_emitter);
    info!(
        "[Gateway] About to set grant_service: {:p}",
        &*grant_service
    );
    state.grant_service = Some(grant_service);
    info!(
        "[Gateway] grant_service set! Checking: {}",
        state.grant_service.is_some()
    );

    info!(
        "[Gateway] Started successfully - EventEmitter initialized: {}, GrantService initialized: {}",
        state.event_emitter.is_some(),
        state.grant_service.is_some()
    );
    info!("[Gateway] Auto-connect will run in background");

    Ok(url)
}

/// Stop the gateway server
#[tauri::command]
pub async fn stop_gateway(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let mut state = gateway_state.write().await;

    if !state.running {
        return Err("Gateway is not running".to_string());
    }

    if let Some(handle) = state.handle.take() {
        handle.abort();
        info!("Gateway stopped");
    }

    state.running = false;
    state.url = None;

    Ok(())
}

/// Restart the gateway server
#[tauri::command]
pub async fn restart_gateway(
    port: Option<u16>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    app_state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    // Stop if running
    {
        let mut state = gateway_state.write().await;
        if let Some(handle) = state.handle.take() {
            handle.abort();
        }
        state.running = false;
        state.url = None;
    }

    // Start with new config
    start_gateway(port, gateway_state, app_state, app_handle).await
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

/// Get the active/default space ID
async fn get_default_space_id(app_state: &AppState) -> Result<String, String> {
    let space = app_state
        .space_service
        .get_active()
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or("No active space found")?;
    Ok(space.id.to_string())
}

/// Connect an installed server to the gateway
#[tauri::command]
pub async fn connect_server(
    server_id: String,
    space_id: Option<String>,
    app_state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    info!("[Gateway] Connecting server: {}", server_id);

    // Get space ID
    let space_id_str = match space_id {
        Some(sid) => sid,
        None => get_default_space_id(&app_state).await?,
    };

    let space_uuid = Uuid::parse_str(&space_id_str).map_err(|e| e.to_string())?;

    // Get the installed server from the database
    let installed = app_state
        .installed_server_repository
        .get_by_server_id(&space_id_str, &server_id)
        .await
        .map_err(|e| {
            error!(
                "[Gateway] Failed to get installed server {}: {}",
                server_id, e
            );
            e.to_string()
        })?
        .ok_or_else(|| {
            warn!("[Gateway] Server not installed: {}", server_id);
            format!("Server not installed: {}", server_id)
        })?;

    // Use cached definition (offline-first)
    let server_definition = installed.get_definition().ok_or_else(|| {
        warn!("[Gateway] Server has no cached definition: {}", server_id);
        format!("Server has no cached definition: {}", server_id)
    })?;

    // Get pool service
    let state = gateway_state.read().await;
    if !state.running {
        return Err("Gateway is not running".to_string());
    }
    let pool_service = state
        .pool_service
        .clone()
        .ok_or("Pool service not initialized")?;
    drop(state); // Release lock before async work

    // Build transport config from cached definition + input values
    let transport = mcpmux_gateway::pool::transport::resolution::build_transport_config(
        &server_definition.transport,
        &installed,
        Some(app_state.data_dir()),
    );

    // Connect using pool service (manual connect from API)
    let ctx = ConnectionContext::new(space_uuid, server_id.clone(), transport);
    let result = pool_service.connect_server(&ctx).await;

    match result {
        ConnectionResult::Connected { reused, features } => {
            info!(
                "[Gateway] Server {} connected (reused: {}, features: {})",
                server_id,
                reused,
                features.total_count()
            );

            // Ensure server-all featureset exists
            ensure_server_featureset(&app_state, &server_id, &server_definition, &installed).await;

            Ok(())
        }
        ConnectionResult::Failed { error } => {
            error!(
                "[Gateway] Failed to connect server {}: {}",
                server_id, error
            );

            Err(error)
        }
        ConnectionResult::OAuthRequired { auth_url } => {
            warn!(
                "[Gateway] Server {} requires OAuth authentication",
                server_id
            );

            Err(format!(
                "OAuth required. Please authenticate at: {}",
                auth_url
            ))
        }
    }
}

/// Ensure server-all featureset exists after connection
///
/// Note: Server state is now managed by ServerManager/PoolService, not GatewayState
async fn ensure_server_featureset(
    app_state: &AppState,
    server_id: &str,
    registry_entry: &mcpmux_core::ServerDefinition,
    installed: &mcpmux_core::InstalledServer,
) {
    let space_id_str = installed.space_id.clone();
    if let Err(e) = app_state
        .feature_set_repository
        .ensure_server_all(&space_id_str, server_id, &registry_entry.name)
        .await
    {
        warn!("[Gateway] Failed to create server-all featureset: {}", e);
    }
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

    for (server_info, transport, server_definition, installed) in servers_to_connect {
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

                // Ensure server-all featureset exists
                ensure_server_featureset(&app_state, &server_id, &server_definition, &installed)
                    .await;
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
