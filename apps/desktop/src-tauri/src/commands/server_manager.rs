//! Server Manager Commands - Event-driven server connection management
//!
//! These commands use the new ServerManager for:
//! - Event-driven status updates (no polling needed)
//! - Proper state machine with flow_id for race prevention
//! - Browser debounce for OAuth flows
//! - Connect/Reconnect button based on connection history

use crate::AppState;
use mcpmux_gateway::pool::transport::resolution::build_transport_config; // Import from gateway
use mcpmux_gateway::{
    ConnectionContext, ConnectionResult, ConnectionStatus, ServerKey, ServerManager,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

/// Server status for UI display
#[derive(Debug, Clone, Serialize)]
pub struct ServerStatusResponse {
    pub server_id: String,
    pub status: ConnectionStatus,
    pub flow_id: u64,
    pub has_connected_before: bool,
    pub message: Option<String>,
}

/// App state wrapper for ServerManager
#[derive(Default)]
pub struct ServerManagerState {
    pub manager: Option<Arc<ServerManager>>,
    pub pool_service: Option<Arc<mcpmux_gateway::PoolService>>,
}

// Event bridge moved to unified gateway event system in commands/gateway.rs
// See: start_gateway_event_bridge() which handles all backend→frontend events

/// Get all server statuses for a space
#[tauri::command]
pub async fn get_server_statuses(
    space_id: String,
    state: State<'_, Arc<RwLock<ServerManagerState>>>,
) -> Result<HashMap<String, ServerStatusResponse>, String> {
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| format!("Invalid space_id: {}", e))?;

    let manager_state = state.read().await;
    let manager = manager_state
        .manager
        .as_ref()
        .ok_or("ServerManager not initialized")?;

    let statuses = manager.get_all_statuses(space_uuid).await;

    Ok(statuses
        .into_iter()
        .map(|(server_id, (status, flow_id, has_connected, msg))| {
            (
                server_id.clone(),
                ServerStatusResponse {
                    server_id,
                    status,
                    flow_id,
                    has_connected_before: has_connected,
                    message: msg,
                },
            )
        })
        .collect())
}

/// Enable a server and attempt connection
///
/// This replaces the old `set_server_enabled(true)` + `connect_server()` pattern.
/// Flow:
/// 1. Update database (enabled = true)
/// 2. Set status = Connecting, emit event
/// 3. Build TransportConfig from registry + installed server
/// 4. Call pool_service.connect_server()
/// 5. Set Connected/AuthRequired/Error based on result
/// 6. Mark features available (connected) or unavailable (auth required/error)
#[tauri::command]
pub async fn enable_server_v2(
    space_id: String,
    server_id: String,
    state: State<'_, Arc<RwLock<ServerManagerState>>>,
    gateway_state: State<'_, Arc<RwLock<crate::commands::gateway::GatewayAppState>>>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| format!("Invalid space_id: {}", e))?;

    // Get installed server record
    let installed = app_state
        .installed_server_repository
        .get_by_server_id(&space_id, &server_id)
        .await
        .map_err(|e| format!("Failed to get server: {}", e))?
        .ok_or_else(|| format!("Server {} not installed", server_id))?;

    // Use cached definition from InstalledServer (offline-first)
    let server_definition = installed
        .get_definition()
        .ok_or_else(|| format!("Server {} has no cached definition", server_id))?;

    // Update database first
    app_state
        .installed_server_repository
        .set_enabled(&installed.id, true)
        .await
        .map_err(|e| format!("Failed to update database: {}", e))?;

    let manager_state = state.read().await;
    let manager = manager_state
        .manager
        .as_ref()
        .ok_or("ServerManager not initialized")?
        .clone();
    let pool_service = manager_state
        .pool_service
        .as_ref()
        .ok_or("PoolService not initialized")?
        .clone();
    drop(manager_state);

    let key = ServerKey::new(space_uuid, &server_id);

    // Set status = Connecting
    manager.set_connecting(&key).await;

    // Build transport config
    let transport = build_transport_config(
        &server_definition.transport,
        &installed,
        Some(app_state.data_dir()),
    );

    // Attempt connection (manual connect from user clicking Connect button)
    let ctx = ConnectionContext::new(space_uuid, server_id.clone(), transport);
    let result = pool_service.connect_server(&ctx).await;

    match result {
        ConnectionResult::Connected { features, .. } => {
            manager.set_connected(&key, features).await;
            // Features are marked available during discover_and_cache (called by pool_service)
            Ok(())
        }
        ConnectionResult::OAuthRequired { .. } => {
            // OAuth is needed - set state to AuthRequired (NOT Authenticating)
            // Don't open browser yet - wait for user to click Connect
            // Cancel the OAuth flow that was started during connection probe
            pool_service
                .oauth_manager()
                .cancel_flow_for_space(space_uuid, &server_id);

            manager.set_auth_required(&key, None).await;

            // Mark features unavailable - not connected
            if let Some(ref feature_service) = gateway_state.read().await.feature_service {
                if let Err(e) = feature_service
                    .mark_unavailable(&space_id, &server_id)
                    .await
                {
                    warn!("[ServerManager] Failed to mark features unavailable: {}", e);
                }
            }

            Ok(())
        }
        ConnectionResult::Failed { error } => {
            manager.set_error(&key, error.clone()).await;

            // Mark features unavailable - connection failed
            if let Some(ref feature_service) = gateway_state.read().await.feature_service {
                if let Err(e) = feature_service
                    .mark_unavailable(&space_id, &server_id)
                    .await
                {
                    warn!("[ServerManager] Failed to mark features unavailable: {}", e);
                }
            }

            Err(error)
        }
    }
}

/// Disable a server (marks as disabled, marks features unavailable, keeps tokens & DCR for fast re-enable)
#[tauri::command]
pub async fn disable_server_v2(
    space_id: String,
    server_id: String,
    state: State<'_, Arc<RwLock<ServerManagerState>>>,
    gateway_state: State<'_, Arc<RwLock<crate::commands::gateway::GatewayAppState>>>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| format!("Invalid space_id: {}", e))?;

    // Get installed server record
    let installed = app_state
        .installed_server_repository
        .get_by_server_id(&space_id, &server_id)
        .await
        .map_err(|e| format!("Failed to get server: {}", e))?
        .ok_or_else(|| format!("Server {} not installed", server_id))?;

    let manager_state = state.read().await;
    let manager = manager_state
        .manager
        .as_ref()
        .ok_or("ServerManager not initialized")?
        .clone();
    let pool_service = manager_state
        .pool_service
        .as_ref()
        .ok_or("PoolService not initialized")?
        .clone();
    drop(manager_state);

    let key = ServerKey::new(space_uuid, &server_id);

    // Just remove from pool (close connection) but don't clear tokens
    pool_service.remove_instance(space_uuid, &server_id);

    // Cancel any pending OAuth flows
    pool_service
        .oauth_manager()
        .cancel_flow_for_space(space_uuid, &server_id);

    // Update state to disconnected (not connected, but not cleared either)
    manager.set_disconnected(&key).await;

    // Update database - just mark as disabled
    app_state
        .installed_server_repository
        .set_enabled(&installed.id, false)
        .await
        .map_err(|e| format!("Failed to update database: {}", e))?;

    // Mark features as unavailable (they'll be re-discovered on re-enable)
    // This ensures features don't show in effective features while server is disabled
    if let Some(ref feature_service) = gateway_state.read().await.feature_service {
        if let Err(e) = feature_service
            .mark_unavailable(&space_id, &server_id)
            .await
        {
            warn!("[ServerManager] Failed to mark features unavailable: {}", e);
        }
    }

    info!(
        "[ServerManager] Server {} disabled (features unavailable, tokens preserved)",
        server_id
    );
    Ok(())
}

/// Start OAuth flow (from AuthRequired state)
///
/// Handles debounce: if called within 2s of last browser open, ignores silently.
/// If >= 2s and already authenticating, reopens the browser with the existing auth URL.
/// If in AuthRequired state, initiates new OAuth flow.
#[tauri::command]
pub async fn start_auth_v2(
    space_id: String,
    server_id: String,
    state: State<'_, Arc<RwLock<ServerManagerState>>>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| format!("Invalid space_id: {}", e))?;

    let manager_state = state.read().await;
    let manager = manager_state
        .manager
        .as_ref()
        .ok_or("ServerManager not initialized")?
        .clone();
    let pool_service = manager_state
        .pool_service
        .as_ref()
        .ok_or("PoolService not initialized")?
        .clone();
    drop(manager_state);

    let key = ServerKey::new(space_uuid, &server_id);

    // Check if already authenticating
    if manager
        .is_status(&key, ConnectionStatus::Authenticating)
        .await
    {
        // Check debounce
        if manager.should_debounce_browser(&key).await {
            info!("[ServerManager] Browser debounce - ignoring quick re-click");
            return Ok(());
        }

        // Reopen browser with existing URL
        if let Some(auth_url) = manager.get_auth_url(&key).await {
            manager.update_browser_opened(&key).await;
            manager.open_browser(&auth_url);
            info!("[ServerManager] Reopened browser for existing OAuth flow");
        }
        return Ok(());
    }

    // Need to start new OAuth flow - get installed server (with cached definition)
    let installed = app_state
        .installed_server_repository
        .get_by_server_id(&space_id, &server_id)
        .await
        .map_err(|e| format!("Failed to get server: {}", e))?
        .ok_or_else(|| format!("Server {} not installed", server_id))?;

    // Use cached definition (offline-first)
    let server_definition = installed
        .get_definition()
        .ok_or_else(|| format!("Server {} has no cached definition", server_id))?;

    // Set status = Connecting
    manager.set_connecting(&key).await;

    // Build transport config and attempt connection (manual connect from user clicking Connect button)
    let transport = build_transport_config(
        &server_definition.transport,
        &installed,
        Some(app_state.data_dir()),
    );
    let ctx = ConnectionContext::new(space_uuid, server_id.clone(), transport);
    let result = pool_service.connect_server(&ctx).await;

    match result {
        ConnectionResult::Connected { features, .. } => {
            manager.set_connected(&key, features).await;
            Ok(())
        }
        ConnectionResult::OAuthRequired { auth_url } => {
            manager.set_authenticating(&key, auth_url.clone()).await;
            manager.open_browser(&auth_url);
            Ok(())
        }
        ConnectionResult::Failed { error } => {
            manager.set_error(&key, error.clone()).await;
            Err(error)
        }
    }
}

/// Cancel OAuth flow - resets to AuthRequired state
#[tauri::command]
pub async fn cancel_auth_v2(
    space_id: String,
    server_id: String,
    server_manager_state: State<'_, Arc<RwLock<ServerManagerState>>>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| format!("Invalid space_id: {}", e))?;

    let manager_state = server_manager_state.read().await;
    let manager = manager_state
        .manager
        .as_ref()
        .ok_or("ServerManager not initialized")?
        .clone();
    let pool_service = manager_state
        .pool_service
        .as_ref()
        .ok_or("PoolService not initialized")?
        .clone();
    drop(manager_state);

    let key = ServerKey::new(space_uuid, &server_id);

    // Cancel the pending OAuth flow in BackendOAuthManager
    pool_service
        .oauth_manager()
        .cancel_flow_for_space(space_uuid, &server_id);

    // Clear oauth_connected flag - requires fresh approval
    if let Ok(Some(installed)) = app_state
        .installed_server_repository
        .get_by_server_id(&space_id, &server_id)
        .await
    {
        if let Err(e) = app_state
            .installed_server_repository
            .set_oauth_connected(&installed.id, false)
            .await
        {
            warn!(
                "[ServerManager] Failed to clear oauth_connected for {}: {}",
                server_id, e
            );
        } else {
            info!(
                "[ServerManager] Cleared oauth_connected for cancelled OAuth flow: {}",
                server_id
            );
        }
    }

    // Reset to AuthRequired state
    manager
        .set_auth_required(&key, Some("Authentication cancelled".to_string()))
        .await;

    Ok(())
}

/// Retry connection (from Error state or after config change)
///
/// IMPORTANT: This MUST remove the existing instance first to ensure
/// new credentials/tokens/inputs are used. Otherwise pool_service.connect_server()
/// will reuse the existing healthy connection with stale config.
#[tauri::command]
pub async fn retry_connection(
    space_id: String,
    server_id: String,
    state: State<'_, Arc<RwLock<ServerManagerState>>>,
    gateway_state: State<'_, Arc<RwLock<crate::commands::gateway::GatewayAppState>>>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| format!("Invalid space_id: {}", e))?;

    // First, remove the existing instance to force fresh connection with new config
    {
        let manager_state = state.read().await;
        if let Some(pool_service) = manager_state.pool_service.as_ref() {
            pool_service.remove_instance(space_uuid, &server_id);
            info!(
                "[ServerManager] Removed existing instance for retry: {}",
                server_id
            );
        }
    }

    // Now enable_server_v2 will create a fresh connection with current config
    enable_server_v2(space_id, server_id, state, gateway_state, app_state).await
}

/// Logout server - Clear OAuth tokens but keep enabled
///
/// Preserves: DCR registration (client_id), input values, enabled flag
/// Clears: OAuth tokens, oauth_connected flag
/// Result: State = auth_required, user must re-authenticate
#[tauri::command]
pub async fn logout_server(
    space_id: String,
    server_id: String,
    server_manager_state: State<'_, Arc<RwLock<ServerManagerState>>>,
    gateway_state: State<'_, Arc<RwLock<crate::commands::gateway::GatewayAppState>>>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| format!("Invalid space_id: {}", e))?;

    let manager_state = server_manager_state.read().await;
    let manager = manager_state
        .manager
        .as_ref()
        .ok_or("ServerManager not initialized")?
        .clone();
    let pool_service = manager_state
        .pool_service
        .as_ref()
        .ok_or("PoolService not initialized")?
        .clone();
    drop(manager_state);

    let key = ServerKey::new(space_uuid, &server_id);

    // 1. Close active connection (if any)
    pool_service.remove_instance(space_uuid, &server_id);

    // 2. Cancel any pending OAuth flows
    pool_service
        .oauth_manager()
        .cancel_flow_for_space(space_uuid, &server_id);

    // 3. Clear all credentials for this server
    if let Err(e) = app_state
        .credential_repository
        .delete_all(&space_uuid, &server_id)
        .await
    {
        warn!(
            "[ServerManager] Failed to delete credentials for {}: {}",
            server_id, e
        );
    } else {
        info!("[ServerManager] Cleared credentials for {}", server_id);
    }

    // 4. Clear oauth_connected flag
    if let Ok(Some(installed)) = app_state
        .installed_server_repository
        .get_by_server_id(&space_id, &server_id)
        .await
    {
        if let Err(e) = app_state
            .installed_server_repository
            .set_oauth_connected(&installed.id, false)
            .await
        {
            warn!(
                "[ServerManager] Failed to clear oauth_connected for {}: {}",
                server_id, e
            );
        }
    }

    // 5. Set state = auth_required (keep enabled = true)
    manager
        .set_auth_required(&key, Some("Logged out - please reconnect".to_string()))
        .await;

    // 6. Mark features unavailable - not connected
    if let Some(ref feature_service) = gateway_state.read().await.feature_service {
        if let Err(e) = feature_service
            .mark_unavailable(&space_id, &server_id)
            .await
        {
            warn!("[ServerManager] Failed to mark features unavailable: {}", e);
        }
    }

    info!(
        "[ServerManager] Server {} logged out (tokens cleared, features unavailable)",
        server_id
    );

    Ok(())
}

/// Disconnect server v2 - Stop connection but keep enabled and preserve all credentials
///
/// Preserves: Everything (tokens, DCR, inputs, enabled flag)
/// Result: State = auth_required (for OAuth) or connecting attempt on next enable
/// Use case: Temporary pause, quick reconnect possible
#[tauri::command]
pub async fn disconnect_server_v2(
    space_id: String,
    server_id: String,
    server_manager_state: State<'_, Arc<RwLock<ServerManagerState>>>,
    gateway_state: State<'_, Arc<RwLock<crate::commands::gateway::GatewayAppState>>>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    use mcpmux_core::AuthConfig;

    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| format!("Invalid space_id: {}", e))?;

    let manager_state = server_manager_state.read().await;
    let manager = manager_state
        .manager
        .as_ref()
        .ok_or("ServerManager not initialized")?
        .clone();
    let pool_service = manager_state
        .pool_service
        .as_ref()
        .ok_or("PoolService not initialized")?
        .clone();
    drop(manager_state);

    let key = ServerKey::new(space_uuid, &server_id);

    // 1. Close active connection only
    pool_service.remove_instance(space_uuid, &server_id);

    // 2. Cancel any pending OAuth flows
    pool_service
        .oauth_manager()
        .cancel_flow_for_space(space_uuid, &server_id);

    // 3. Check if this is an OAuth server (use cached definition)
    let installed = app_state
        .installed_server_repository
        .get_by_server_id(&space_id, &server_id)
        .await
        .ok()
        .flatten();
    let is_oauth = installed
        .and_then(|i| i.get_definition())
        .map(|def| matches!(def.auth, Some(AuthConfig::Oauth)))
        .unwrap_or(false);

    // 4. Set state based on server type
    if is_oauth {
        // OAuth server: go to auth_required (can reconnect with stored tokens)
        manager.set_auth_required(&key, None).await;
    } else {
        // Non-OAuth server: go to disconnected
        manager.set_disconnected(&key).await;
    }

    // 5. Mark features unavailable - not connected
    if let Some(ref feature_service) = gateway_state.read().await.feature_service {
        if let Err(e) = feature_service
            .mark_unavailable(&space_id, &server_id)
            .await
        {
            warn!("[ServerManager] Failed to mark features unavailable: {}", e);
        }
    }

    info!(
        "[ServerManager] Server {} disconnected (features unavailable, credentials preserved)",
        server_id
    );

    Ok(())
}
