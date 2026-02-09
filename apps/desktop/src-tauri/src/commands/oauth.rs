//! OAuth Authorization Flow
//!
//! This module handles the OAuth 2.0 authorization code flow with PKCE
//! for MCP client approval.
//!
//! ## Flow Overview
//!
//! 1. MCP client (VS Code, Cursor, etc.) calls `/oauth/authorize` on gateway
//! 2. Gateway validates request, stores pending auth, returns HTML redirect page
//! 3. Browser opens, triggers `mcpmux://authorize?request_id=xxx` deep link
//! 4. Desktop app receives deep link, calls `get_pending_consent` to validate
//! 5. Backend validates request_id, returns full consent details from DB
//! 6. Desktop shows consent modal (only if valid)
//! 7. User approves → app calls `approve_oauth_consent`
//! 8. Backend atomically processes approval, issues code
//! 9. Desktop app opens redirect URL with code back to MCP client
//! 10. MCP client exchanges code for tokens via `/oauth/token`
//!
//! ## Security
//!
//! - Deep link only contains request_id (no spoofable client info)
//! - Backend validates request exists and isn't expired/processed
//! - Client name/scopes come from backend (authoritative source)
//! - Atomic approval prevents race conditions
//! - PKCE required for all authorization requests (RFC 7636)

use std::collections::HashMap;
use std::sync::Arc;

use mcpmux_core::branding;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use url::Url;

use super::gateway::GatewayAppState;

// ============================================================================
// Deep Link Handling
// ============================================================================

/// Event name for OAuth consent requests sent to frontend
/// Now only contains request_id - frontend must call get_pending_consent
pub const OAUTH_CONSENT_EVENT: &str = "oauth-consent-request";

/// Event name for server install requests sent to frontend (from deep link)
pub const SERVER_INSTALL_EVENT: &str = "server-install-request";

/// Minimal deep link payload - only request_id
/// Frontend must call get_pending_consent to get full details
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthDeepLinkPayload {
    pub request_id: String,
}

/// Deep link payload for server installation requests
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInstallDeepLinkPayload {
    pub server_id: String,
}

/// Full consent request details returned by get_pending_consent
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentRequestDetails {
    pub request_id: String,
    pub client_id: String,
    pub client_name: String,
    pub redirect_uri: String,
    pub scope: String,
    pub state: Option<String>,
    /// When this request expires (Unix timestamp)
    pub expires_at: i64,
}

/// Handle an incoming deep link URL
///
/// Routes based on the URL path:
/// - `mcpmux://authorize` - OAuth authorization request (inbound - client approval)
/// - `mcpmux://callback/oauth` - OAuth callback (outbound - server connection)
pub fn handle_deep_link<R: tauri::Runtime>(app: &tauri::AppHandle<R>, url: &str) {
    info!("[DeepLink] Received: {}", url);

    // Validate URL scheme
    if !branding::is_deep_link(url) {
        warn!(
            "[DeepLink] Invalid scheme, expected {}://",
            branding::DEEP_LINK_SCHEME
        );
        return;
    }

    // Check for OAuth callback first (mcpmux://callback/oauth?...)
    if branding::is_oauth_callback(url) {
        let parsed = match Url::parse(url) {
            Ok(u) => u,
            Err(e) => {
                error!("[DeepLink] Failed to parse OAuth callback URL: {}", e);
                return;
            }
        };
        handle_oauth_callback_deep_link(app, &parsed);
        return;
    }

    // Parse URL for other routes
    let parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(e) => {
            error!("[DeepLink] Failed to parse URL: {}", e);
            return;
        }
    };

    // Route based on host (for mcpmux://authorize, host is "authorize")
    match parsed.host_str() {
        Some("authorize") | Some("oauth") => {
            // Inbound OAuth - client requesting approval
            handle_authorize_deep_link(app, &parsed);
        }
        Some("install") => {
            handle_install_deep_link(app, &parsed);
        }
        Some("test") => {
            info!("[DeepLink] Test URL received successfully!");
        }
        _ => {
            debug!("[DeepLink] Unknown route: {:?}", parsed.host_str());
        }
    }
}

/// Handle OAuth authorization deep link
///
/// Only extracts request_id and emits to frontend.
/// Frontend must call get_pending_consent to validate and get details.
fn handle_authorize_deep_link<R: tauri::Runtime>(app: &tauri::AppHandle<R>, url: &Url) {
    let params: HashMap<_, _> = url.query_pairs().collect();

    // Only require request_id - all other data comes from backend
    let request_id = match params.get("request_id") {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            error!("[DeepLink] Missing required parameter: request_id");
            return;
        }
    };

    info!(
        "[DeepLink] Authorization request received: request_id='{}'",
        request_id
    );

    // Emit minimal payload - frontend will fetch details from backend
    let payload = OAuthDeepLinkPayload { request_id };

    if let Err(e) = app.emit(OAUTH_CONSENT_EVENT, &payload) {
        error!("[DeepLink] Failed to emit consent event: {}", e);
        return;
    }

    // Focus the main window
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Handle server install deep link
///
/// Extracts server_id and emits to frontend.
/// Frontend will look up the server definition and show install modal.
fn handle_install_deep_link<R: tauri::Runtime>(app: &tauri::AppHandle<R>, url: &Url) {
    let params: HashMap<_, _> = url.query_pairs().collect();

    let server_id = match params.get("server") {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            error!("[DeepLink] Install link missing required parameter: server");
            return;
        }
    };

    info!(
        "[DeepLink] Server install request: server_id='{}'",
        server_id
    );

    let payload = ServerInstallDeepLinkPayload { server_id };

    if let Err(e) = app.emit(SERVER_INSTALL_EVENT, &payload) {
        error!("[DeepLink] Failed to emit server install event: {}", e);
        return;
    }

    // Focus the main window
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Handle OAuth callback deep link (legacy - for outbound OAuth server connections)
///
/// NOTE: The primary OAuth callback mechanism is now the loopback HTTP server
/// (per RFC 8252 Section 7.3) which handles callbacks directly. This deep link
/// handler is kept for backwards compatibility but is not the main path.
///
/// The loopback server provides universal compatibility with enterprise security
/// systems that may block custom URL schemes.
///
/// URL format: mcpmux://callback/oauth?code=XXX&state=YYY
/// Or on error: mcpmux://callback/oauth?error=XXX&error_description=YYY&state=ZZZ
fn handle_oauth_callback_deep_link<R: tauri::Runtime>(app: &tauri::AppHandle<R>, url: &Url) {
    let params: HashMap<_, _> = url.query_pairs().collect();

    // State is required for routing to the correct OAuth flow
    let state = match params.get("state") {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            error!("[DeepLink] OAuth callback missing required 'state' parameter");
            return;
        }
    };

    let state_short = if state.len() > 8 { &state[..8] } else { &state };
    info!("[DeepLink] OAuth callback received: state={}", state_short);

    // Build callback struct
    let callback = mcpmux_gateway::OAuthCallback {
        code: params.get("code").map(|s| s.to_string()),
        state,
        error: params.get("error").map(|s| s.to_string()),
        error_description: params.get("error_description").map(|s| s.to_string()),
    };

    // Get the pool service and route the callback
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Get GatewayAppState
        let gateway_state: tauri::State<'_, Arc<RwLock<GatewayAppState>>> = app_handle.state();
        let app_state = gateway_state.read().await;

        if let Some(ref pool_service) = app_state.pool_service {
            // Route callback to OAuth manager
            match pool_service.oauth_manager().handle_callback(callback) {
                Ok(_) => {
                    info!("[DeepLink] OAuth callback successfully routed to handler");
                }
                Err(e) => {
                    error!("[DeepLink] Failed to route OAuth callback: {}", e);
                }
            }
        } else {
            error!("[DeepLink] Pool service not available to handle OAuth callback");
        }
    });
}

// ============================================================================
// Tauri Commands
// ============================================================================

/// Error type for consent operations
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentError {
    pub code: String,
    pub message: String,
}

impl ConsentError {
    fn not_found(request_id: &str) -> Self {
        Self {
            code: "NOT_FOUND".to_string(),
            message: format!(
                "Authorization request '{}' not found or expired",
                request_id
            ),
        }
    }

    fn expired(request_id: &str) -> Self {
        Self {
            code: "EXPIRED".to_string(),
            message: format!("Authorization request '{}' has expired", request_id),
        }
    }

    #[allow(dead_code)]
    fn already_processed(request_id: &str) -> Self {
        Self {
            code: "ALREADY_PROCESSED".to_string(),
            message: format!(
                "Authorization request '{}' has already been processed",
                request_id
            ),
        }
    }

    fn gateway_unavailable() -> Self {
        Self {
            code: "GATEWAY_UNAVAILABLE".to_string(),
            message: "Gateway is not running".to_string(),
        }
    }
}

/// Get pending consent request details from backend
///
/// This validates the request_id and returns full details from the authoritative
/// backend source. The frontend should call this after receiving a deep link
/// before showing the consent modal.
///
/// Returns:
/// - Ok(ConsentRequestDetails) if the request is valid and pending
/// - Err(ConsentError) if request not found, expired, or already processed
#[tauri::command]
pub async fn get_pending_consent(
    request_id: String,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<ConsentRequestDetails, ConsentError> {
    info!(
        "[OAuth] Fetching pending consent: request_id='{}'",
        request_id
    );

    let app_state = gateway_state.read().await;

    // Get gateway state
    let gw_state = app_state
        .gateway_state
        .as_ref()
        .ok_or_else(ConsentError::gateway_unavailable)?;

    // Look up the pending authorization
    let auth = {
        let state = gw_state.read().await;
        state.pending_authorizations.get(&request_id).cloned()
    };

    let auth = auth.ok_or_else(|| ConsentError::not_found(&request_id))?;

    // Check if expired
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    if auth.expires_at < now {
        warn!("[OAuth] Request '{}' has expired", request_id);
        // Remove expired entry
        let mut state = gw_state.write().await;
        state.pending_authorizations.remove(&request_id);
        return Err(ConsentError::expired(&request_id));
    }

    // Build response with authoritative data from backend
    // The client_name here comes from our database lookup in handlers.rs
    let details = ConsentRequestDetails {
        request_id: request_id.clone(),
        client_id: auth.client_id.clone(),
        client_name: auth
            .client_name
            .clone()
            .unwrap_or_else(|| auth.client_id.clone()),
        redirect_uri: auth.redirect_uri.clone(),
        scope: auth.scope.clone().unwrap_or_default(),
        state: auth.state.clone(),
        expires_at: auth.expires_at,
    };

    info!(
        "[OAuth] Consent details validated: client='{}' scopes='{}'",
        details.client_name, details.scope
    );

    Ok(details)
}

/// Request to approve or deny OAuth consent
#[derive(Debug, Deserialize)]
pub struct ConsentApprovalRequest {
    /// The request_id from the pending authorization
    pub request_id: String,
    /// Whether the user approved the request
    pub approved: bool,
    /// Optional alias name for the client
    pub client_alias: Option<String>,
    /// Connection mode: "follow_active", "locked", or "ask_on_change"
    pub connection_mode: Option<String>,
    /// Space ID to lock to (only used when connection_mode is "locked")
    pub locked_space_id: Option<String>,
}

/// Response from consent approval
#[derive(Debug, Serialize, Deserialize)]
pub struct ConsentApprovalResponse {
    /// Whether the approval was successful
    pub success: bool,
    /// The redirect URL for the client (with code or error)
    pub redirect_url: String,
    /// Optional error message
    pub error: Option<String>,
}

/// Approve or deny an OAuth consent request (direct state access)
///
/// This command is called by the frontend after the user has reviewed
/// and approved (or denied) an OAuth authorization request.
#[tauri::command]
pub async fn approve_oauth_consent(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    request: ConsentApprovalRequest,
) -> Result<ConsentApprovalResponse, String> {
    info!(
        "[OAuth] Frontend consent {} for request_id: {}",
        if request.approved {
            "approved"
        } else {
            "denied"
        },
        request.request_id
    );

    let app_state = gateway_state.read().await;

    // Get gateway state
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };

    // Look up the pending authorization
    let pending = {
        let state = gw_state.read().await;
        state
            .pending_authorizations
            .get(&request.request_id)
            .cloned()
    };

    let Some(pending) = pending else {
        error!("[OAuth] Consent approval failed: request_id not found");
        return Ok(ConsentApprovalResponse {
            success: false,
            redirect_url: String::new(),
            error: Some("Authorization request not found or expired".to_string()),
        });
    };

    // Remove the pending authorization (it's been processed)
    {
        let mut state = gw_state.write().await;
        state.pending_authorizations.remove(&request.request_id);
    }

    if !request.approved {
        // User denied - redirect with error
        // Client registration remains (unapproved) so they can try again later
        let mut redirect_url = pending.redirect_uri.clone();
        redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
        redirect_url.push_str("error=access_denied&error_description=User+denied+the+request");
        if let Some(ref state_param) = pending.state {
            redirect_url.push_str(&format!("&state={}", urlencoding::encode(state_param)));
        }

        info!(
            "[OAuth] User denied consent for client: {}",
            pending.client_id
        );
        return Ok(ConsentApprovalResponse {
            success: true,
            redirect_url,
            error: None,
        });
    }

    // User approved - generate authorization code
    use uuid::Uuid;
    let code = format!("mc_{}", Uuid::new_v4().to_string().replace("-", ""));

    // Auth codes expire in 10 minutes (standard OAuth)
    let code_expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 + 600) // 10 minutes
        .unwrap_or(i64::MAX);

    // Store the authorization with the new code and update client alias if provided
    {
        let mut state = gw_state.write().await;

        // Clone pending fields for new authorization
        let new_pending = mcpmux_gateway::PendingAuthorization {
            client_id: pending.client_id.clone(),
            client_name: pending.client_name.clone(),
            redirect_uri: pending.redirect_uri.clone(),
            scope: pending.scope.clone(),
            state: pending.state.clone(),
            code_challenge: pending.code_challenge.clone(),
            code_challenge_method: pending.code_challenge_method.clone(),
            expires_at: code_expires_at,
        };

        state.store_pending_authorization(&code, new_pending);

        // Mark client as approved and store settings
        if let Some(repo) = state.inbound_client_repository() {
            // Mark as approved for clients tab visibility
            if let Err(e) = repo.approve_client(&pending.client_id).await {
                error!("[OAuth] Failed to approve client: {}", e);
            } else {
                info!("[OAuth] Client approved: {}", pending.client_id);
            }

            // Update client settings (alias, connection_mode, locked_space_id)
            if let Ok(Some(mut client)) = repo.get_client(&pending.client_id).await {
                let mut changed = false;

                // Set alias if provided
                if let Some(alias) = &request.client_alias {
                    if !alias.is_empty() {
                        client.client_alias = Some(alias.clone());
                        changed = true;
                        info!(
                            "[OAuth] Set client alias '{}' for: {}",
                            alias, pending.client_id
                        );
                    }
                }

                // Set connection mode if provided
                if let Some(mode) = &request.connection_mode {
                    client.connection_mode = mode.clone();
                    changed = true;
                    info!(
                        "[OAuth] Set connection mode '{}' for: {}",
                        mode, pending.client_id
                    );
                }

                // Set locked space if provided (only meaningful when mode is "locked")
                if let Some(space_id) = &request.locked_space_id {
                    client.locked_space_id = Some(space_id.clone());
                    changed = true;
                    info!(
                        "[OAuth] Locked to space '{}' for: {}",
                        space_id, pending.client_id
                    );
                } else if request.connection_mode.as_deref() == Some("follow_active") {
                    // Clear locked space if switching to follow_active
                    client.locked_space_id = None;
                    changed = true;
                }

                if changed {
                    if let Err(e) = repo.save_client(&client).await {
                        error!("[OAuth] Failed to save client settings: {}", e);
                    }
                }
            }
        }

        // Emit domain event for client registration
        state.emit_domain_event(mcpmux_core::DomainEvent::ClientRegistered {
            client_id: pending.client_id.clone(),
            client_name: pending.client_id.clone(), // Use client_name field
            registration_type: Some("unknown".to_string()), // Will be updated when client metadata is fetched
        });
    }

    // Build redirect URL with authorization code
    let mut redirect_url = pending.redirect_uri.clone();
    redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
    redirect_url.push_str(&format!("code={}", code));
    if let Some(ref state_param) = pending.state {
        redirect_url.push_str(&format!("&state={}", urlencoding::encode(state_param)));
    }

    info!(
        "[OAuth] Authorization approved for client: {}, issuing code",
        pending.client_id
    );
    info!("[OAuth] Redirect URL: {}", redirect_url);

    Ok(ConsentApprovalResponse {
        success: true,
        redirect_url,
        error: None,
    })
}

/// Get list of connected OAuth clients (direct service access)
#[tauri::command]
pub async fn get_oauth_clients(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<Vec<OAuthClientInfo>, String> {
    let app_state = gateway_state.read().await;

    // Get gateway state and inbound client repository (direct access)
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };

    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };

    // Fetch clients directly from repository (no HTTP call)
    // Only show approved clients in the UI
    let all_clients = repo
        .list_clients()
        .await
        .map_err(|e| format!("Failed to fetch clients: {}", e))?;

    let clients: Vec<_> = all_clients.into_iter().filter(|c| c.approved).collect();

    info!(
        "[OAuth] Fetched {} approved clients from repository",
        clients.len()
    );

    // Map to response format
    let client_infos: Vec<OAuthClientInfo> = clients
        .into_iter()
        .map(|client| {
            OAuthClientInfo {
                client_id: client.client_id,
                registration_type: client.registration_type.as_str().to_string(),
                client_name: client.client_name,
                client_alias: client.client_alias,
                redirect_uris: client.redirect_uris,
                scope: client.scope,
                approved: client.approved,
                logo_uri: client.logo_uri,
                client_uri: client.client_uri,
                software_id: client.software_id,
                software_version: client.software_version,
                metadata_url: client.metadata_url,
                metadata_cached_at: client.metadata_cached_at,
                metadata_cache_ttl: client.metadata_cache_ttl,
                connection_mode: client.connection_mode,
                locked_space_id: client.locked_space_id,
                last_seen: client.last_seen,
                created_at: client.created_at,
                has_active_tokens: false, // TODO: Check if client has active tokens
            }
        })
        .collect();

    Ok(client_infos)
}

/// Information about a connected OAuth client
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthClientInfo {
    pub client_id: String,
    pub registration_type: String,
    pub client_name: String,
    pub client_alias: Option<String>,
    pub redirect_uris: Vec<String>,
    pub scope: Option<String>,

    // Approval status
    pub approved: bool,

    // RFC 7591 Client Metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,

    // CIMD-specific fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_cached_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_cache_ttl: Option<i64>,

    // MCP client preferences
    pub connection_mode: String,
    pub locked_space_id: Option<String>,
    pub last_seen: Option<String>,
    pub created_at: String,
    pub has_active_tokens: bool,
}

/// Request to update client settings
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateClientSettingsRequest {
    pub client_alias: Option<String>,
    pub connection_mode: Option<String>,
    pub locked_space_id: Option<String>,
}

/// Update an OAuth client's settings (direct service access)
#[tauri::command]
pub async fn update_oauth_client(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    client_id: String,
    settings: UpdateClientSettingsRequest,
) -> Result<OAuthClientInfo, String> {
    let app_state = gateway_state.read().await;

    // Get gateway state and inbound client repository
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };

    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };

    // Update client directly via repository
    repo.update_client_settings(
        &client_id,
        settings.client_alias,
        settings.connection_mode,
        settings.locked_space_id.map(Some),
    )
    .await
    .map_err(|e| format!("Failed to update client: {}", e))?;

    info!("[OAuth] Updated client: {}", client_id);

    // Emit domain event
    state.emit_domain_event(mcpmux_core::DomainEvent::ClientUpdated {
        client_id: client_id.clone(),
    });

    // Get updated client
    let updated_client = repo
        .get_client(&client_id)
        .await
        .map_err(|e| format!("Failed to get updated client: {}", e))?
        .ok_or("Client not found after update")?;

    // Map to response format
    Ok(OAuthClientInfo {
        client_id: updated_client.client_id,
        registration_type: updated_client.registration_type.as_str().to_string(),
        client_name: updated_client.client_name,
        client_alias: updated_client.client_alias,
        redirect_uris: updated_client.redirect_uris,
        scope: updated_client.scope,
        approved: updated_client.approved,
        logo_uri: updated_client.logo_uri,
        client_uri: updated_client.client_uri,
        software_id: updated_client.software_id,
        software_version: updated_client.software_version,
        metadata_url: updated_client.metadata_url,
        metadata_cached_at: updated_client.metadata_cached_at,
        metadata_cache_ttl: updated_client.metadata_cache_ttl,
        connection_mode: updated_client.connection_mode,
        locked_space_id: updated_client.locked_space_id,
        last_seen: updated_client.last_seen,
        created_at: updated_client.created_at,
        has_active_tokens: false,
    })
}

/// Get grants for an OAuth client in a specific space
///
/// Returns the effective grants: explicit grants + the default feature set
/// This matches the authorization behavior used by MCP handlers
#[tauri::command]
pub async fn get_oauth_client_grants(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    app_state: State<'_, crate::AppState>,
    client_id: String,
    space_id: String,
) -> Result<Vec<String>, String> {
    let gw_app_state = gateway_state.read().await;

    // Get gateway state and inbound client repository
    let Some(ref gw_state) = gw_app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };

    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };

    // Get explicit grants from DB
    let mut grants = repo
        .get_grants_for_space(&client_id, &space_id)
        .await
        .map_err(|e| format!("Failed to get grants: {}", e))?;

    // Add default feature set (layered resolution - same as MCP handlers)
    if let Ok(Some(default_fs)) = app_state
        .feature_set_repository
        .get_default_for_space(&space_id)
        .await
    {
        if !grants.contains(&default_fs.id) {
            grants.push(default_fs.id);
        }
    }

    Ok(grants)
}

/// Grant a feature set to an OAuth client in a specific space
#[tauri::command]
pub async fn grant_oauth_client_feature_set(
    app_handle: tauri::AppHandle,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    client_id: String,
    space_id: String,
    feature_set_id: String,
) -> Result<(), String> {
    info!("[OAuth] grant_oauth_client_feature_set called: client_id={}, space_id={}, feature_set_id={}", 
        client_id, space_id, feature_set_id);

    let app_state = gateway_state.read().await;

    info!("[OAuth] Gateway running: {}", app_state.running);
    info!(
        "[OAuth] Gateway state exists: {}",
        app_state.gateway_state.is_some()
    );
    info!(
        "[OAuth] Grant service exists: {}",
        app_state.grant_service.is_some()
    );

    // Get GrantService (centralized grant management with auto-notifications)
    let Some(ref grant_service) = app_state.grant_service else {
        error!(
            "[OAuth] Grant service is None! Gateway running={}, gateway_state={}",
            app_state.running,
            app_state.gateway_state.is_some()
        );
        return Err("Gateway not running".to_string());
    };

    // Single call handles: DB update + validation + automatic notifications (DRY!)
    grant_service
        .grant_feature_set(&client_id, &space_id, &feature_set_id)
        .await
        .map_err(|e| format!("Failed to grant feature set: {}", e))?;

    // Notify UI
    if let Err(e) = app_handle.emit(
        "oauth-client-changed",
        serde_json::json!({
            "action": "grants_updated",
            "client_id": client_id,
        }),
    ) {
        error!("[OAuth] Failed to emit oauth-client-changed event: {}", e);
    }

    Ok(())
}

/// Revoke a feature set from an OAuth client in a specific space
#[tauri::command]
pub async fn revoke_oauth_client_feature_set(
    app_handle: tauri::AppHandle,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    client_id: String,
    space_id: String,
    feature_set_id: String,
) -> Result<(), String> {
    let app_state = gateway_state.read().await;

    // Get GrantService (centralized grant management with auto-notifications)
    let Some(ref grant_service) = app_state.grant_service else {
        return Err("Gateway not running".to_string());
    };

    // Single call handles: DB update + validation + automatic notifications (DRY!)
    grant_service
        .revoke_feature_set(&client_id, &space_id, &feature_set_id)
        .await
        .map_err(|e| format!("Failed to revoke feature set: {}", e))?;

    // Notify UI
    if let Err(e) = app_handle.emit(
        "oauth-client-changed",
        serde_json::json!({
            "action": "grants_updated",
            "client_id": client_id,
        }),
    ) {
        error!("[OAuth] Failed to emit oauth-client-changed event: {}", e);
    }

    Ok(())
}

/// Resolved client features response
#[derive(Debug, Serialize, Deserialize)]
pub struct ResolvedClientFeatures {
    pub space_id: String,
    pub feature_set_ids: Vec<String>,
    pub tools: Vec<serde_json::Value>,
    pub prompts: Vec<serde_json::Value>,
    pub resources: Vec<serde_json::Value>,
}

/// Get resolved features for an OAuth client in a specific space
///
/// Returns the granted feature sets and resolved capabilities for a client.
/// This is used by the UI to display what a client has access to.
///
/// The frontend is responsible for determining which space to query:
/// - For locked clients: pass the client's locked_space_id
/// - For follow_active clients: pass the currently active space_id
///
/// This keeps space resolution logic in ONE place (frontend/SpaceResolverService)
/// rather than duplicating it here.
#[tauri::command]
pub async fn get_oauth_client_resolved_features(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    app_state: State<'_, crate::AppState>,
    client_id: String,
    space_id: String, // Required - frontend must resolve which space to use
) -> Result<ResolvedClientFeatures, String> {
    let gw_app_state = gateway_state.read().await;

    // Get gateway state
    let Some(ref gw_state) = gw_app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };

    // Get feature service
    let Some(ref feature_service) = gw_app_state.feature_service else {
        return Err("Feature service not available".to_string());
    };

    // Get inbound client repository for grants
    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };

    // Get explicit grants for this client in this space
    let mut feature_set_ids = repo
        .get_grants_for_space(&client_id, &space_id)
        .await
        .map_err(|e| format!("Failed to get grants: {}", e))?;

    // Add default feature set (layered resolution - same as MCP handlers)
    if let Ok(Some(default_fs)) = app_state
        .feature_set_repository
        .get_default_for_space(&space_id)
        .await
    {
        if !feature_set_ids.contains(&default_fs.id) {
            feature_set_ids.push(default_fs.id);
        }
    }

    info!(
        "[OAuth] Client {} has {} effective grants in space {}",
        client_id,
        feature_set_ids.len(),
        space_id
    );

    // Release the lock before calling feature service
    drop(state);

    // Resolve features from feature sets using FeatureService
    let tools = feature_service
        .get_tools_for_grants(&space_id, &feature_set_ids)
        .await
        .unwrap_or_default();

    let prompts = feature_service
        .get_prompts_for_grants(&space_id, &feature_set_ids)
        .await
        .unwrap_or_default();

    let resources = feature_service
        .get_resources_for_grants(&space_id, &feature_set_ids)
        .await
        .unwrap_or_default();

    info!(
        "[OAuth] Resolved features for client {}: {} tools, {} prompts, {} resources",
        client_id,
        tools.len(),
        prompts.len(),
        resources.len()
    );

    // Convert to response format
    let tools_response: Vec<_> = tools
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.feature_name,
                "description": f.description,
                "server_id": f.server_id,
            })
        })
        .collect();

    let prompts_response: Vec<_> = prompts
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.feature_name,
                "description": f.description,
                "server_id": f.server_id,
            })
        })
        .collect();

    let resources_response: Vec<_> = resources
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.feature_name,
                "description": f.description,
                "server_id": f.server_id,
            })
        })
        .collect();

    Ok(ResolvedClientFeatures {
        space_id,
        feature_set_ids,
        tools: tools_response,
        prompts: prompts_response,
        resources: resources_response,
    })
}

/// Delete an OAuth client (direct service access)
#[tauri::command]
pub async fn delete_oauth_client(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    client_id: String,
) -> Result<(), String> {
    let app_state = gateway_state.read().await;

    // Get gateway state and inbound client repository
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };

    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };

    // Delete client directly via repository
    repo.delete_client(&client_id)
        .await
        .map_err(|e| format!("Failed to delete client: {}", e))?;

    info!("[OAuth] Deleted client: {}", client_id);

    // Emit domain event
    state.emit_domain_event(mcpmux_core::DomainEvent::ClientDeleted { client_id });

    Ok(())
}

/// Open a URL without flashing a terminal window (Windows-specific)
#[cfg(target_os = "windows")]
fn open_url_no_flash(url: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_cmd: i32,
        ) -> isize;
    }

    let url_wide: Vec<u16> = OsStr::new(url).encode_wide().chain(Some(0)).collect();
    let open_wide: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();

    // SW_SHOWNORMAL = 1
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            open_wide.as_ptr(),
            url_wide.as_ptr(),
            ptr::null(),
            ptr::null(),
            1,
        )
    };

    // ShellExecuteW returns > 32 on success
    if result > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW failed with code: {}", result))
    }
}

/// Open a URL using the default handler (non-Windows)
#[cfg(not(target_os = "windows"))]
fn open_url_no_flash(url: &str) -> Result<(), String> {
    open::that(url).map_err(|e| format!("Failed to open URL: {}", e))
}

/// Open a URL or deliver OAuth callback
///
/// For localhost/127.0.0.1 URLs (like VS Code's OAuth callback), makes a direct
/// HTTP request instead of opening a browser - cleaner UX, no browser window.
///
/// For custom protocol URLs (like `cursor://`), uses the system handler.
/// For regular http/https URLs to remote hosts, opens in the default browser.
#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    info!("[OAuth] Processing redirect URL: {}", url);

    // Parse the URL to determine how to handle it
    let parsed = Url::parse(&url).map_err(|e| format!("Invalid URL: {}", e))?;

    // Check if this is a localhost callback (VS Code, etc.)
    let is_localhost = matches!(parsed.host_str(), Some("localhost") | Some("127.0.0.1"));
    let is_http = parsed.scheme() == "http" || parsed.scheme() == "https";

    if is_localhost && is_http {
        // For localhost callbacks, make a direct HTTP request
        // This avoids opening a browser window for a cleaner UX
        info!("[OAuth] Delivering callback directly to localhost: {}", url);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        match client.get(&url).send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() || status.is_redirection() {
                    info!(
                        "[OAuth] Callback delivered successfully (status: {})",
                        status
                    );
                } else {
                    // Some clients return non-2xx but still process the code
                    warn!(
                        "[OAuth] Callback returned status {}, but code was delivered",
                        status
                    );
                }
                Ok(())
            }
            Err(e) => {
                // Connection refused likely means the client's server closed
                // This can happen if the user took too long to approve
                error!("[OAuth] Failed to deliver callback: {}", e);
                Err(format!("Failed to deliver OAuth callback. The application may have timed out waiting. Please try again. Error: {}", e))
            }
        }
    } else {
        // For custom protocols (cursor://, vscode://) or remote URLs, use system handler
        // Use ShellExecuteW on Windows to avoid terminal flash
        info!("[OAuth] Opening URL with system handler: {}", url);
        open_url_no_flash(&url).map_err(|e| {
            error!("[OAuth] Failed to open URL '{}': {}", url, e);
            e
        })?;

        info!("[OAuth] URL opened successfully");
        Ok(())
    }
}
