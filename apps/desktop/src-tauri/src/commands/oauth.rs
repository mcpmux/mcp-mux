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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use mcpmux_core::branding;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use url::Url;

use super::gateway::GatewayAppState;
use crate::services::ui_events::{
    emit_ui_channel_from_app, OAUTH_CLIENT_CHANGED_CHANNEL, OAUTH_CONSENT_REQUEST_CHANNEL,
};

// ============================================================================
// Deep Link Handling
// ============================================================================

/// Holds a deep-link URL the app was cold-started with (Windows/Linux) until
/// the webview has mounted its listeners. Emitting `oauth-consent-request`
/// before React has subscribed drops the event — Tauri events are fire-and-
/// forget with no replay. The frontend calls `flush_pending_deep_link` once
/// its listener is live to process any buffered URL.
#[derive(Default)]
pub struct PendingInitialDeepLink {
    pub url: Mutex<Option<String>>,
    pub webview_ready: AtomicBool,
}

/// Called from `on_open_url`: route immediately if the webview has signalled
/// ready, otherwise buffer for later flush. Falls back to direct routing
/// if the state isn't managed yet (shouldn't happen after setup).
pub fn route_or_buffer_deep_link<R: tauri::Runtime>(app: &tauri::AppHandle<R>, url: &str) {
    match app.try_state::<PendingInitialDeepLink>() {
        Some(pending) if !pending.webview_ready.load(Ordering::Acquire) => {
            info!("[DeepLink] Webview not ready — buffering URL: {}", url);
            if let Ok(mut guard) = pending.url.lock() {
                *guard = Some(url.to_string());
            }
        }
        Some(pending) => {
            info!(
                "[DeepLink] Webview ready — routing immediately: {} (ready={})",
                url,
                pending.webview_ready.load(Ordering::Acquire)
            );
            handle_deep_link(app, url);
        }
        None => {
            warn!(
                "[DeepLink] PendingInitialDeepLink state missing — routing anyway: {}",
                url
            );
            handle_deep_link(app, url);
        }
    }
}

/// Invoked by the frontend once the `oauth-consent-request` listener is live.
/// Marks the webview ready so subsequent URLs route immediately, and drains
/// any URL that arrived before mount.
#[tauri::command]
pub fn flush_pending_deep_link(app: tauri::AppHandle, pending: State<'_, PendingInitialDeepLink>) {
    info!("[DeepLink] flush_pending_deep_link called — marking webview ready");
    pending.webview_ready.store(true, Ordering::Release);
    let buffered = pending.url.lock().ok().and_then(|mut g| g.take());
    if let Some(url) = buffered {
        info!("[DeepLink] Flushing buffered cold-start URL: {}", url);
        handle_deep_link(&app, &url);
    } else {
        info!("[DeepLink] flush_pending_deep_link: no buffered URL to drain");
    }
}

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
pub use mcpmux_gateway::oauth::ConsentRequestDetails;

/// Window within which an identical deep-link URL is treated as a duplicate
/// and dropped. On a warm launch BOTH the deep-link plugin's `on_open_url` and
/// the single-instance callback fire for the same `mcpmux://` URL, so without
/// this guard the whole consent flow (deep-link emit → `get_pending_consent` →
/// consent modal) runs twice per approval.
const DEEP_LINK_DEDUP_WINDOW: Duration = Duration::from_secs(3);

/// Pure predicate: is `url` a repeat of `last_url` seen `elapsed` ago, within
/// `window`? Extracted so the dedup rule is unit-testable without the clock.
fn is_recent_duplicate_link(
    last_url: Option<&str>,
    elapsed: Duration,
    url: &str,
    window: Duration,
) -> bool {
    last_url == Some(url) && elapsed < window
}

/// True if this exact URL was just handled within [`DEEP_LINK_DEDUP_WINDOW`].
/// Records `url` as the most-recent on a miss. Distinct authorizations carry a
/// fresh `request_id` (different URL), so legitimate back-to-back flows are
/// never collapsed.
fn deep_link_is_duplicate(url: &str) -> bool {
    static LAST: OnceLock<Mutex<Option<(String, Instant)>>> = OnceLock::new();
    let cell = LAST.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    let dup = guard.as_ref().is_some_and(|(last_url, seen_at)| {
        is_recent_duplicate_link(
            Some(last_url.as_str()),
            now.duration_since(*seen_at),
            url,
            DEEP_LINK_DEDUP_WINDOW,
        )
    });
    if !dup {
        *guard = Some((url.to_string(), now));
    }
    dup
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

    // Drop the duplicate that the on_open_url + single-instance paths both
    // deliver for the same warm-launch URL — otherwise the consent modal and
    // `get_pending_consent` fire twice per approval.
    if deep_link_is_duplicate(url) {
        info!("[DeepLink] Ignoring duplicate within {DEEP_LINK_DEDUP_WINDOW:?}: {url}");
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

    emit_ui_channel_from_app(
        app,
        OAUTH_CONSENT_REQUEST_CHANNEL,
        serde_json::json!({ "requestId": payload.request_id }),
    );

    info!(
        "[DeepLink] Emitted Tauri event '{}' for request_id='{}'",
        OAUTH_CONSENT_REQUEST_CHANNEL, payload.request_id
    );

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
pub use mcpmux_gateway::oauth::ConsentError;

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
        "[OAuth] Tauri get_pending_consent invoked: request_id='{}'",
        request_id
    );
    let app_state = gateway_state.read().await;
    let gw_state = app_state.gateway_state.as_ref().ok_or_else(|| {
        warn!("[OAuth] get_pending_consent: gateway not running");
        ConsentError::gateway_unavailable()
    })?;
    let result = mcpmux_gateway::oauth::get_pending_consent(gw_state, request_id.clone()).await;
    match &result {
        Ok(details) => info!(
            "[OAuth] get_pending_consent OK: request_id='{}', client='{}'",
            request_id, details.client_name
        ),
        Err(err) => warn!(
            "[OAuth] get_pending_consent failed: request_id='{}', code='{}', message='{}'",
            request_id, err.code, err.message
        ),
    }
    result
}

/// Request to approve or deny OAuth consent
pub use mcpmux_gateway::oauth::ConsentApprovalRequest;

/// Response from consent approval
pub use mcpmux_gateway::oauth::ConsentApprovalResponse;

/// Approve or deny an OAuth consent request (direct state access)
///
/// This command is called by the frontend after the user has reviewed
/// and approved (or denied) an OAuth authorization request.
#[tauri::command]
pub async fn approve_oauth_consent(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    request: ConsentApprovalRequest,
) -> Result<ConsentApprovalResponse, String> {
    let app_state = gateway_state.read().await;
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };
    mcpmux_gateway::oauth::approve_oauth_consent(gw_state, request).await
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
        .map(|client| OAuthClientInfo {
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
            last_seen: client.last_seen,
            created_at: client.created_at,
            reports_roots: client.reports_roots,
            roots_capability_known: client.roots_capability_known,
        })
        .collect();

    Ok(client_infos)
}

/// Approve a registered OAuth client by ID (for E2E testing only).
///
/// Guarded by the `MCPMUX_E2E_TEST` environment variable. In production
/// builds this command is a no-op that returns an error.
#[tauri::command]
pub async fn approve_oauth_client(
    client_id: String,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    if std::env::var("MCPMUX_E2E_TEST").is_err() {
        return Err("approve_oauth_client is only available in E2E test mode".to_string());
    }

    let app_state = gateway_state.read().await;
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };
    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };
    repo.approve_client(&client_id)
        .await
        .map_err(|e| format!("Failed to approve client: {}", e))?;
    info!(
        "[OAuth] Approved client via E2E test command: {}",
        client_id
    );
    Ok(())
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

    pub last_seen: Option<String>,
    pub created_at: String,

    /// Sticky-positive bit: `true` once any session of this client
    /// declared the MCP `roots` capability. Meaningful only when
    /// `roots_capability_known` is `true` — for a brand-new client we
    /// haven't seen `initialize` for yet, this defaults to `false` but
    /// the UI must hide the "Rootless" badge instead of trusting it.
    pub reports_roots: bool,

    /// `true` once we've processed at least one `notifications/initialized`
    /// for this client. Until then, the UI treats the capability as
    /// unknown (no badge). Once known, the badge resolves to either
    /// "Reports workspace" (`reports_roots = true`) or "Rootless"
    /// (`reports_roots = false`).
    pub roots_capability_known: bool,
}

/// Request to update client settings.
///
/// Only the alias is user-editable now — connection mode / space pin no
/// longer exist.
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateClientSettingsRequest {
    pub client_alias: Option<String>,
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

    repo.update_client_alias(&client_id, settings.client_alias)
        .await
        .map_err(|e| format!("Failed to update client: {}", e))?;

    info!("[OAuth] Updated client: {}", client_id);

    state.emit_domain_event(mcpmux_core::DomainEvent::ClientUpdated {
        client_id: client_id.clone(),
    });

    let updated_client = repo
        .get_client(&client_id)
        .await
        .map_err(|e| format!("Failed to get updated client: {}", e))?
        .ok_or("Client not found after update")?;

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
        last_seen: updated_client.last_seen,
        created_at: updated_client.created_at,
        reports_roots: updated_client.reports_roots,
        roots_capability_known: updated_client.roots_capability_known,
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

// =============================================================================
// API-key clients (manually registered, host-issued credentials)
//
// A "preregistered", pre-approved inbound client authenticated by a long-lived
// API key. Unlike DCR clients it skips the browser-consent deep link, so
// headless/remote clients can connect with just the key.
// =============================================================================

/// A newly-registered API-key client. `api_key` is returned ONCE at creation —
/// McpMux stores only its SHA-256 hash and can never show it again.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredApiKeyClient {
    pub client_id: String,
    pub client_name: String,
    pub locked_space_id: Option<String>,
    pub api_key: String,
    pub key_prefix: String,
}

/// API-key metadata for display (never includes the secret).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyInfo {
    pub key_id: String,
    pub key_prefix: String,
    pub label: Option<String>,
    pub revoked: bool,
    pub last_used_at: Option<String>,
    pub created_at: String,
}

/// Generate a strong API key: `mcpk_` + 256 bits of v4-UUID randomness.
/// Returns `(key_id, plaintext, key_prefix)`. Only the hash is ever stored.
fn generate_api_key() -> (String, String, String) {
    let key_id = uuid::Uuid::new_v4().to_string();
    let secret = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let plaintext = format!("mcpk_{secret}");
    let key_prefix: String = plaintext.chars().take(13).collect();
    (key_id, plaintext, key_prefix)
}

/// Register a new pre-approved client authenticated by an API key.
/// The returned `api_key` is shown once and never stored.
#[tauri::command]
pub async fn register_api_key_client(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    name: String,
    locked_space_id: Option<String>,
) -> Result<RegisteredApiKeyClient, String> {
    let app_state = gateway_state.read().await;
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };
    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };

    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Client name is required".to_string());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let client_id = format!("mcp_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let client = mcpmux_storage::InboundClient {
        client_id: client_id.clone(),
        registration_type: mcpmux_storage::RegistrationType::Preregistered,
        client_name: trimmed.to_string(),
        client_alias: None,
        redirect_uris: vec![],
        grant_types: vec![],
        response_types: vec![],
        token_endpoint_auth_method: "none".to_string(),
        scope: None,
        approved: true,
        logo_uri: None,
        client_uri: None,
        software_id: None,
        software_version: None,
        metadata_url: None,
        metadata_cached_at: None,
        metadata_cache_ttl: None,
        last_seen: None,
        created_at: now.clone(),
        updated_at: now,
        reports_roots: false,
        roots_capability_known: false,
        machine_id: None,
    };
    repo.save_client(&client)
        .await
        .map_err(|e| format!("Failed to create client: {}", e))?;

    let locked_space = locked_space_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            uuid::Uuid::parse_str(s).map_err(|_| format!("Invalid locked_space_id: {s}"))
        })
        .transpose()?;
    if let Some(space_id) = locked_space {
        repo.set_locked_space(&client_id, Some(space_id))
            .await
            .map_err(|e| format!("Failed to set Space lock: {}", e))?;
    }

    let (key_id, plaintext, key_prefix) = generate_api_key();
    repo.create_api_key(&key_id, &client_id, &plaintext, &key_prefix, None, None)
        .await
        .map_err(|e| format!("Failed to create API key: {}", e))?;

    info!(
        "[OAuth] Registered API-key client {} ({})",
        trimmed, client_id
    );

    state.emit_domain_event(mcpmux_core::DomainEvent::ClientRegistered {
        client_id: client_id.clone(),
        client_name: trimmed.to_string(),
        registration_type: Some("preregistered".to_string()),
    });

    Ok(RegisteredApiKeyClient {
        client_id,
        client_name: trimmed.to_string(),
        locked_space_id: locked_space.map(|id| id.to_string()),
        api_key: plaintext,
        key_prefix,
    })
}

/// Issue an additional API key for an existing client (rotation). Returns the
/// new key plaintext once.
#[tauri::command]
pub async fn create_client_api_key(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    client_id: String,
    label: Option<String>,
) -> Result<RegisteredApiKeyClient, String> {
    let app_state = gateway_state.read().await;
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };
    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };

    let Some(client) = repo
        .get_client(&client_id)
        .await
        .map_err(|e| format!("Failed to load client: {}", e))?
    else {
        return Err("Client not found".to_string());
    };

    let (key_id, plaintext, key_prefix) = generate_api_key();
    repo.create_api_key(
        &key_id,
        &client_id,
        &plaintext,
        &key_prefix,
        label.as_deref(),
        None,
    )
    .await
    .map_err(|e| format!("Failed to create API key: {}", e))?;

    let locked_space_id = repo
        .get_locked_space(&client_id)
        .await
        .map_err(|e| format!("Failed to read Space lock: {}", e))?
        .map(|id| id.to_string());

    Ok(RegisteredApiKeyClient {
        client_id,
        client_name: client.client_name,
        locked_space_id,
        api_key: plaintext,
        key_prefix,
    })
}

/// List a client's API keys (metadata only — never the secret).
#[tauri::command]
pub async fn list_client_api_keys(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    client_id: String,
) -> Result<Vec<ApiKeyInfo>, String> {
    let app_state = gateway_state.read().await;
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };
    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };

    let keys = repo
        .list_api_keys(&client_id)
        .await
        .map_err(|e| format!("Failed to list API keys: {}", e))?;

    Ok(keys
        .into_iter()
        .map(|k| ApiKeyInfo {
            key_id: k.key_id,
            key_prefix: k.key_prefix,
            label: k.label,
            revoked: k.revoked,
            last_used_at: k.last_used_at,
            created_at: k.created_at,
        })
        .collect())
}

/// Revoke a single API key (it can never authenticate again).
#[tauri::command]
pub async fn revoke_client_api_key(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    key_id: String,
) -> Result<(), String> {
    let app_state = gateway_state.read().await;
    let Some(ref gw_state) = app_state.gateway_state else {
        return Err("Gateway not running".to_string());
    };
    let state = gw_state.read().await;
    let Some(repo) = state.inbound_client_repository() else {
        return Err("Database not available".to_string());
    };

    repo.revoke_api_key(&key_id)
        .await
        .map_err(|e| format!("Failed to revoke API key: {}", e))?;
    info!("[OAuth] Revoked API key {}", key_id);
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
                if e.is_connect() || e.is_timeout() {
                    warn!(
                        "[OAuth] Loopback callback listener unavailable ({}); skipping browser redirect",
                        e
                    );
                    Ok(())
                } else {
                    error!("[OAuth] Failed to deliver callback: {}", e);
                    Err(format!(
                        "Failed to deliver OAuth callback. Please try again. Error: {e}"
                    ))
                }
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

// ============================================================================
// Client grants — rootless OAuth-client fallback path.
//
// Roots-capable sessions ignore these grants; the resolver routes them via
// `WorkspaceBinding`. These commands target the older `client_grants` table
// (restored in migration 009) and back the per-client FS toggles in the
// Clients UI. Each write is funnelled through `GrantService` so a
// `ClientGrantChanged` domain event fires + MCPNotifier pushes
// `list_changed` to that client's open peers.
// ============================================================================

/// Read the FeatureSet ids granted to a (client, space) pair.
///
/// Returns an empty Vec when nothing is granted — the UI renders the
/// "no defaults configured" state in that case. The default-FS layering
/// from older revisions is *not* applied here: the resolver itself decides
/// what an unconfigured grant means (deny when rootless), and the UI shows
/// the literal grant set so the user can see exactly what they configured.
#[tauri::command]
pub async fn get_oauth_client_grants(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    client_id: String,
    space_id: String,
) -> Result<Vec<String>, String> {
    let gw_state = gateway_state.read().await;
    let Some(ref grant_service) = gw_state.grant_service else {
        return Err("Gateway not running".to_string());
    };
    grant_service
        .get_grants_for_space(&client_id, &space_id)
        .await
        .map_err(|e| format!("Failed to get grants: {}", e))
}

/// Grant a feature set to an OAuth client in a specific space.
/// Idempotent at the DB layer; always emits `ClientGrantChanged`.
#[tauri::command]
pub async fn grant_oauth_client_feature_set(
    app_handle: tauri::AppHandle,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    client_id: String,
    space_id: String,
    feature_set_id: String,
) -> Result<(), String> {
    info!(
        "[OAuth] grant_oauth_client_feature_set: client_id={}, space_id={}, feature_set_id={}",
        client_id, space_id, feature_set_id
    );

    let gw_state = gateway_state.read().await;
    let Some(ref grant_service) = gw_state.grant_service else {
        error!("[OAuth] Grant service unavailable (gateway not running)");
        return Err("Gateway not running".to_string());
    };

    grant_service
        .grant_feature_set(&client_id, &space_id, &feature_set_id)
        .await
        .map_err(|e| format!("Failed to grant feature set: {}", e))?;

    emit_ui_channel_from_app(
        &app_handle,
        OAUTH_CLIENT_CHANGED_CHANNEL,
        serde_json::json!({
            "action": "grants_updated",
            "client_id": client_id,
        }),
    );

    Ok(())
}

/// Revoke a feature set from an OAuth client in a specific space.
#[tauri::command]
pub async fn revoke_oauth_client_feature_set(
    app_handle: tauri::AppHandle,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    client_id: String,
    space_id: String,
    feature_set_id: String,
) -> Result<(), String> {
    let gw_state = gateway_state.read().await;
    let Some(ref grant_service) = gw_state.grant_service else {
        return Err("Gateway not running".to_string());
    };

    grant_service
        .revoke_feature_set(&client_id, &space_id, &feature_set_id)
        .await
        .map_err(|e| format!("Failed to revoke feature set: {}", e))?;

    emit_ui_channel_from_app(
        &app_handle,
        OAUTH_CLIENT_CHANGED_CHANNEL,
        serde_json::json!({
            "action": "grants_updated",
            "client_id": client_id,
        }),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Duration = Duration::from_secs(3);
    const URL: &str = "mcpmux://authorize?request_id=abc-123";

    #[test]
    fn first_sighting_is_not_a_duplicate() {
        // No prior URL → never a duplicate.
        assert!(!is_recent_duplicate_link(None, Duration::ZERO, URL, W));
    }

    #[test]
    fn same_url_within_window_is_a_duplicate() {
        // The on_open_url + single-instance double-fire: same URL, ~ms apart.
        assert!(is_recent_duplicate_link(
            Some(URL),
            Duration::from_millis(40),
            URL,
            W
        ));
    }

    #[test]
    fn same_url_after_window_is_not_a_duplicate() {
        // A genuine re-auth of the same URL long after is allowed through.
        assert!(!is_recent_duplicate_link(
            Some(URL),
            Duration::from_secs(5),
            URL,
            W
        ));
    }

    #[test]
    fn different_request_id_is_never_a_duplicate() {
        // Distinct authorizations carry a fresh request_id even back-to-back.
        let other = "mcpmux://authorize?request_id=def-456";
        assert!(!is_recent_duplicate_link(
            Some(URL),
            Duration::from_millis(10),
            other,
            W
        ));
    }
}
