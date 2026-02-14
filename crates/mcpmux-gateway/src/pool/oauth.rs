//! Backend OAuth Client - OAuth 2.1 PKCE flow using rmcp SDK
//!
//! Uses rmcp's OAuthState state machine for the complete OAuth flow:
//! - Metadata discovery (RFC 8414 + RFC 9728)
//! - Dynamic Client Registration (DCR)
//! - PKCE authorization flow
//! - Automatic token refresh
//!
//! OAuth callbacks are received via loopback HTTP server (per RFC 8252 Section 7.3)
//! using `http://127.0.0.1:{port}/oauth2redirect`. This method is universally
//! compatible with all OAuth providers, including enterprise security systems
//! that block custom URL schemes.
//!
//! Our DatabaseCredentialStore provides persistent encrypted storage.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use mcpmux_core::{
    branding, CredentialRepository, CredentialType, LogLevel, LogSource, OutboundOAuthRepository,
    ServerLog, ServerLogManager,
};
use rmcp::transport::auth::{AuthError, AuthorizationManager, AuthorizationSession, OAuthState};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::credential_store::DatabaseCredentialStore;
use super::oauth_utils;

/// Default OAuth timeout (5 minutes for user to complete browser auth)
const DEFAULT_OAUTH_TIMEOUT: Duration = Duration::from_secs(300);

/// OAuth callback parameters from browser redirect (via deep link)
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OAuthCallback {
    /// Authorization code (missing if error occurred)
    #[serde(default)]
    pub code: Option<String>,
    pub state: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

/// Result of initiating OAuth flow
#[derive(Debug)]
pub enum OAuthInitResult {
    /// OAuth flow initiated - browser should open auth_url
    Initiated { auth_url: String },
    /// Already have valid credentials
    AlreadyAuthorized,
    /// OAuth not supported by server
    NotSupported(String),
}

/// Token info extracted from credentials (for status checks)
#[derive(Debug, Clone)]
pub struct OAuthTokenInfo {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub token_type: String,
    pub scope: Option<String>,
}

impl OAuthTokenInfo {
    /// Check if token is expired or about to expire (5 min buffer)
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let buffer = chrono::Duration::seconds(300);
            expires_at - buffer < Utc::now()
        } else {
            false
        }
    }

    pub fn can_refresh(&self) -> bool {
        self.refresh_token.is_some()
    }
}

/// Pending OAuth flow - keyed by state parameter for callback routing
struct PendingOAuthFlow {
    space_id: Uuid,
    server_id: String,
    server_url: String,
    started_at: std::time::Instant,
    /// Channel to notify the waiting task when callback arrives
    callback_tx: tokio::sync::oneshot::Sender<OAuthCallback>,
}

/// OAuth completion event - emitted when OAuth flow completes
#[derive(Debug, Clone)]
pub struct OAuthCompleteEvent {
    pub space_id: Uuid,
    pub server_id: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Backend OAuth manager - handles OAuth flows for server connections
///
/// OAuth callbacks are received via a persistent loopback HTTP server (per RFC 8252 Section 7.3)
/// which provides universal compatibility with enterprise security systems.
///
/// The callback server is started once and shared across all concurrent OAuth flows,
/// ensuring consistent port usage (like VS Code's 33418) for DCR registration.
/// If the preferred port is unavailable, a dynamic port is allocated and persisted
/// for future use.
pub struct OutboundOAuthManager {
    /// Pending flows keyed by OAuth state parameter (for callback routing)
    pending_by_state: Arc<DashMap<String, PendingOAuthFlow>>,
    /// Active flow state by space+server (for duplicate detection)
    /// Maps (space_id, server_id) -> state
    active_by_server: Arc<DashMap<(Uuid, String), String>>,
    /// Active OAuth state machines (state -> OAuthState)
    oauth_states: Arc<DashMap<String, Arc<Mutex<OAuthState>>>>,
    /// Completed OAuth flows ready for reconnection
    completed_flows: Arc<DashMap<(Uuid, String), std::time::Instant>>,
    /// OAuth timeout
    timeout: Duration,
    /// Event channel for OAuth completion notifications
    completion_tx: tokio::sync::broadcast::Sender<OAuthCompleteEvent>,
    /// Server log manager for detailed OAuth logging
    log_manager: Option<Arc<ServerLogManager>>,
    /// Shared callback server state (port + running flag)
    /// Using Mutex to ensure only one server starts
    callback_server: Arc<Mutex<Option<CallbackServerState>>>,
    /// App settings repository for persisting callback port
    settings_repo: Option<Arc<dyn mcpmux_core::AppSettingsRepository>>,
    /// Space repository for looking up space names (for DCR client_name)
    space_repo: Option<Arc<dyn mcpmux_core::SpaceRepository>>,
}

/// Persistent callback server state
struct CallbackServerState {
    /// Port the server is listening on
    port: u16,
    /// Shutdown signal sender
    _shutdown_tx: tokio::sync::watch::Sender<bool>,
}

impl OutboundOAuthManager {
    pub fn new() -> Self {
        let (completion_tx, _) = tokio::sync::broadcast::channel(32);
        Self {
            pending_by_state: Arc::new(DashMap::new()),
            active_by_server: Arc::new(DashMap::new()),
            oauth_states: Arc::new(DashMap::new()),
            completed_flows: Arc::new(DashMap::new()),
            timeout: DEFAULT_OAUTH_TIMEOUT,
            completion_tx,
            log_manager: None,
            callback_server: Arc::new(Mutex::new(None)),
            settings_repo: None,
            space_repo: None,
        }
    }

    pub fn with_log_manager(mut self, log_manager: Arc<ServerLogManager>) -> Self {
        self.log_manager = Some(log_manager);
        self
    }

    pub fn with_settings_repo(
        mut self,
        settings_repo: Arc<dyn mcpmux_core::AppSettingsRepository>,
    ) -> Self {
        self.settings_repo = Some(settings_repo);
        self
    }

    pub fn with_space_repo(mut self, space_repo: Arc<dyn mcpmux_core::SpaceRepository>) -> Self {
        self.space_repo = Some(space_repo);
        self
    }

    /// Get the DCR client name for a space (e.g., "McpMux (Work)")
    async fn get_client_name_for_space(&self, space_id: Uuid) -> String {
        let space_name = if let Some(repo) = &self.space_repo {
            repo.get(&space_id).await.ok().flatten().map(|s| s.name)
        } else {
            None
        };
        branding::outbound_oauth_client_name_for_space(space_name.as_deref())
    }

    /// Log an OAuth event
    async fn log(
        &self,
        space_id: &str,
        server_id: &str,
        level: LogLevel,
        message: String,
        metadata: Option<serde_json::Value>,
    ) {
        if let Some(log_manager) = &self.log_manager {
            let log = ServerLog::new(level, LogSource::OAuth, message)
                .with_metadata(metadata.unwrap_or(serde_json::json!({})));
            if let Err(e) = log_manager.append(space_id, server_id, log).await {
                warn!("[OAuth] Failed to write log: {}", e);
            }
        }
    }

    async fn ensure_metadata_with_origin_fallback(
        &self,
        manager: &mut AuthorizationManager,
        server_url: &str,
        _space_id: &str,
        _server_id: &str,
    ) -> Result<mcpmux_core::StoredOAuthMetadata, AuthError> {
        // Delegate to shared utility - returns both formats for setting on manager and storing
        let (rmcp_metadata, stored_metadata) =
            oauth_utils::discover_and_convert_metadata(manager, server_url).await?;
        manager.set_metadata(rmcp_metadata);
        Ok(stored_metadata)
    }

    /// Extract appropriate scopes from discovered OAuth metadata.
    ///
    /// Uses server-advertised scopes if available, otherwise falls back to empty scopes
    /// (letting the server determine defaults). This ensures compatibility with servers
    /// like Miro that require specific scopes and don't support generic ones like `offline_access`.
    ///
    /// Note: Some servers (like Atlassian) return `scopes_supported: null` in their metadata,
    /// meaning they don't advertise supported scopes. In this case, we pass empty scopes
    /// and the server will apply its default scope policy.
    fn get_scopes_from_metadata(metadata: &mcpmux_core::StoredOAuthMetadata) -> Vec<String> {
        match &metadata.scopes_supported {
            Some(scopes) if !scopes.is_empty() => {
                info!("[OAuth] Using server-advertised scopes: {:?}", scopes);
                scopes.clone()
            }
            Some(_) => {
                // Empty array - server advertised no specific scopes
                info!("[OAuth] Server advertised empty scopes_supported, using empty scopes (server will apply defaults)");
                Vec::new()
            }
            None => {
                // null/missing - server doesn't advertise scopes (e.g., Atlassian)
                info!("[OAuth] Server metadata has no scopes_supported (null), using empty scopes (server will apply defaults)");
                Vec::new()
            }
        }
    }

    /// Convert scope Vec to slice references for RMCP API
    fn scopes_as_refs(scopes: &[String]) -> Vec<&str> {
        scopes.iter().map(|s| s.as_str()).collect()
    }

    /// Add RFC 8707 'resource' parameter to authorization URL.
    ///
    /// The resource parameter tells the Authorization Server which protected resource
    /// (MCP server) the client is requesting access to. This enables the AS to:
    /// - Issue tokens scoped to the specific resource
    /// - Apply resource-specific policies
    /// - Prevent token replay at other resources
    ///
    /// Some servers (like Miro) require this parameter.
    fn add_resource_parameter(auth_url: &str, server_url: &str) -> String {
        use url::Url;

        match Url::parse(auth_url) {
            Ok(mut url) => {
                // Add the resource parameter with the MCP server URL
                url.query_pairs_mut().append_pair("resource", server_url);
                info!("[OAuth] Added RFC 8707 resource parameter: {}", server_url);
                url.to_string()
            }
            Err(e) => {
                warn!(
                    "[OAuth] Failed to parse auth URL to add resource parameter: {}",
                    e
                );
                // Return original URL if parsing fails
                auth_url.to_string()
            }
        }
    }

    /// Subscribe to OAuth completion events
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<OAuthCompleteEvent> {
        self.completion_tx.subscribe()
    }

    /// Check if OAuth flow is pending for a server
    pub fn is_pending(&self, space_id: Uuid, server_id: &str) -> bool {
        let key = (space_id, server_id.to_string());
        if let Some(state) = self.active_by_server.get(&key) {
            if let Some(flow) = self.pending_by_state.get(state.value()) {
                if flow.started_at.elapsed() < Duration::from_secs(600) {
                    return true;
                }
            }
            // Expired, clean up
            drop(state);
            if let Some((_, old_state)) = self.active_by_server.remove(&key) {
                self.pending_by_state.remove(&old_state);
                self.oauth_states.remove(&old_state);
            }
        }
        false
    }

    /// Get pending flow context
    pub fn get_pending_context(&self, space_id: Uuid, server_id: &str) -> Option<(Uuid, String)> {
        let key = (space_id, server_id.to_string());
        if let Some(state) = self.active_by_server.get(&key) {
            self.pending_by_state
                .get(state.value())
                .map(|flow| (flow.space_id, flow.server_url.clone()))
        } else {
            None
        }
    }

    /// Check if OAuth just completed (and consume the flag)
    pub fn take_completed(&self, space_id: Uuid, server_id: &str) -> bool {
        let key = (space_id, server_id.to_string());
        if let Some((_, completed_at)) = self.completed_flows.remove(&key) {
            completed_at.elapsed() < Duration::from_secs(300)
        } else {
            false
        }
    }

    /// Check if OAuth just completed (without consuming)
    pub fn has_completed(&self, space_id: Uuid, server_id: &str) -> bool {
        let key = (space_id, server_id.to_string());
        self.completed_flows
            .get(&key)
            .map(|e| e.elapsed() < Duration::from_secs(300))
            .unwrap_or(false)
    }

    /// Cancel any pending OAuth flow for a server
    ///
    /// Called when disconnecting to ensure a fresh flow can start on reconnect.
    /// Note: The persistent callback server keeps running - only the pending flow is removed.
    pub fn cancel_flow(&self, space_id: Uuid, server_id: &str) {
        let key = (space_id, server_id.to_string());
        if let Some((_, state)) = self.active_by_server.remove(&key) {
            info!(
                "[OAuth] Cancelled pending flow for {} (space={}) (state={})",
                server_id,
                space_id,
                &state[..8]
            );
            // Remove from pending_by_state - this drops the oneshot sender,
            // causing the waiting task to get an error
            if let Some((_, flow)) = self.pending_by_state.remove(&state) {
                // Dropping flow.callback_tx signals cancellation to the waiting task
                drop(flow);
            }
            if self.oauth_states.remove(&state).is_some() {
                debug!("[OAuth] Removed OAuth state for {}", server_id);
            }
        }
    }

    /// Cancel any pending OAuth flow for a space/server pair
    pub fn cancel_flow_for_space(&self, space_id: Uuid, server_id: &str) {
        self.cancel_flow(space_id, server_id);
        // Also clear completed flows
        self.completed_flows
            .remove(&(space_id, server_id.to_string()));
    }

    /// Get the redirect URI for OAuth callbacks (loopback interface per RFC 8252)
    ///
    /// Per RFC 8252 Section 7.3, loopback interface redirection uses:
    /// `http://127.0.0.1:{port}/oauth2redirect`
    ///
    /// This is the most compatible method for native app OAuth as enterprise
    /// security systems don't block loopback addresses.
    ///
    /// Note: Port is dynamically assigned when the callback server starts.
    pub fn get_redirect_uri_with_port(port: u16) -> String {
        branding::oauth_callback_uri_with_port(port)
    }

    /// Try to bind to preferred port, fall back to dynamic port
    ///
    /// Returns (listener, source_description)
    async fn try_bind_with_fallback(
        preferred_port: u16,
    ) -> Result<(tokio::net::TcpListener, &'static str)> {
        use tokio::net::TcpListener;

        match TcpListener::bind(format!("127.0.0.1:{}", preferred_port)).await {
            Ok(l) => {
                info!("[OAuth] Bound to preferred port {}", preferred_port);
                Ok((l, "preferred"))
            }
            Err(_) => {
                info!(
                    "[OAuth] Preferred port {} unavailable, allocating dynamic port",
                    preferred_port
                );
                let l = TcpListener::bind("127.0.0.1:0")
                    .await
                    .context("Failed to bind loopback callback server")?;
                Ok((l, "dynamic"))
            }
        }
    }

    /// Get or start the shared callback server
    ///
    /// This ensures only ONE callback server runs at a time, shared across all
    /// concurrent OAuth flows. Like VS Code's consistent use of port 33418,
    /// this minimizes DCR re-registration by keeping the redirect_uri stable.
    ///
    /// Port resolution order:
    /// 1. Persisted port (from previous run) - if available
    /// 2. Default preferred port (45819) - if available
    /// 3. Dynamic port allocation - always persist for next run
    ///
    /// The server routes callbacks to the correct flow using the `state` parameter.
    ///
    /// Returns the port the shared server is listening on.
    async fn ensure_callback_server(&self) -> Result<u16> {
        use axum::{
            extract::{Query, State as AxumState},
            response::Html,
            routing::get,
            Router,
        };
        use tokio::net::TcpListener;

        let mut server_guard = self.callback_server.lock().await;

        // Check if server is already running
        if let Some(ref state) = *server_guard {
            debug!(
                "[OAuth] Reusing existing callback server on port {}",
                state.port
            );
            return Ok(state.port);
        }

        // Try to get persisted port from settings
        let persisted_port = if let Some(ref settings) = self.settings_repo {
            settings
                .get("oauth.callback_port")
                .await
                .ok()
                .flatten()
                .and_then(|s| s.parse::<u16>().ok())
        } else {
            None
        };

        // Start new shared callback server
        // Port resolution: persisted > default > dynamic
        let (listener, port_source) = if let Some(port) = persisted_port {
            match TcpListener::bind(format!("127.0.0.1:{}", port)).await {
                Ok(l) => {
                    info!("[OAuth] Using persisted callback port {}", port);
                    (l, "persisted")
                }
                Err(_) => {
                    info!(
                        "[OAuth] Persisted port {} unavailable, trying default",
                        port
                    );
                    Self::try_bind_with_fallback(branding::DEFAULT_OAUTH_CALLBACK_PORT).await?
                }
            }
        } else {
            Self::try_bind_with_fallback(branding::DEFAULT_OAUTH_CALLBACK_PORT).await?
        };

        let port = listener.local_addr()?.port();
        info!(
            "[OAuth] Shared callback server listening on 127.0.0.1:{} ({})",
            port, port_source
        );

        // Persist the port for future runs (if it's not already persisted at this value)
        if persisted_port != Some(port) {
            if let Some(ref settings) = self.settings_repo {
                if let Err(e) = settings.set("oauth.callback_port", &port.to_string()).await {
                    warn!("[OAuth] Failed to persist callback port {}: {}", port, e);
                } else {
                    info!("[OAuth] Persisted callback port {} for future use", port);
                }
            }
        }

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

        // Clone pending_by_state for the handler
        let pending_map = self.pending_by_state.clone();

        // Build the callback handler that routes by state parameter
        let app = Router::new()
            .route(
                branding::oauth_callback_path(),
                get(
                    |AxumState(pending): AxumState<Arc<DashMap<String, PendingOAuthFlow>>>,
                     Query(callback): Query<OAuthCallback>| async move {
                        let state = &callback.state;
                        let state_short = if state.len() > 8 { &state[..8] } else { state };

                        info!(
                            "[OAuth] Callback received on shared server, state={}",
                            state_short
                        );

                        // Route to the correct pending flow
                        match pending.remove(state) {
                            Some((_, flow)) => {
                                info!(
                                    "[OAuth] Routing callback to server={} (space={})",
                                    flow.server_id, flow.space_id
                                );
                                let _ = flow.callback_tx.send(callback);
                            }
                            None => {
                                warn!(
                                    "[OAuth] No pending flow for state={}, may have timed out",
                                    state_short
                                );
                            }
                        }

                        // Return a branded HTML page that auto-closes
                        let app_name = branding::DISPLAY_NAME;
                        Html(format!(
                            r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{app_name} - Authorization Complete</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
            background: linear-gradient(135deg, #1a1210 0%, #2a1c17 50%, #1e1412 100%);
            color: #e6e6e6;
            padding: 1rem;
        }}
        .container {{
            text-align: center;
            max-width: 400px;
        }}
        .logo {{
            width: 64px;
            height: 64px;
            margin: 0 auto 1.5rem;
        }}
        .check {{
            width: 48px;
            height: 48px;
            margin: 0 auto 1rem;
            background: rgba(74, 222, 128, 0.12);
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
        }}
        .check svg {{
            width: 24px;
            height: 24px;
        }}
        h1 {{
            font-size: 1.5rem;
            font-weight: 600;
            margin-bottom: 0.75rem;
            color: #fff;
        }}
        .subtitle {{
            color: #a0917e;
            line-height: 1.5;
            margin-bottom: 1.5rem;
        }}
        .note {{
            font-size: 0.875rem;
            color: #7a6e62;
        }}
    </style>
</head>
<body>
    <div class="container">
        <svg class="logo" viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
            <defs><linearGradient id="bg" x1="0" y1="0" x2="32" y2="32" gradientUnits="userSpaceOnUse"><stop offset="0%" stop-color="#DA7756"/><stop offset="100%" stop-color="#B8553A"/></linearGradient></defs>
            <rect width="32" height="32" rx="7" fill="url(#bg)"/>
            <circle cx="6" cy="9" r="2.8" fill="white" opacity="0.85"/><circle cx="6" cy="16" r="3" fill="white"/><circle cx="6" cy="23" r="2.8" fill="white" opacity="0.85"/>
            <path d="M 10 21 V 11 L 13 18 L 16 10 L 19 18 L 22 11 V 21" stroke="white" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
            <circle cx="26" cy="9" r="2.8" fill="white" opacity="0.85"/><circle cx="26" cy="16" r="3" fill="white"/><circle cx="26" cy="23" r="2.8" fill="white" opacity="0.85"/>
        </svg>
        <div class="check">
            <svg viewBox="0 0 24 24" fill="none" stroke="#4ade80" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
        </div>
        <h1>Authorization Complete</h1>
        <p class="subtitle">
            You can close this window and return to {app_name}.
        </p>
        <p class="note">This window will close automatically.</p>
    </div>
    <script>setTimeout(function(){{ window.close(); }}, 2000);</script>
</body>
</html>"##
                        ))
                    },
                ),
            )
            .with_state(pending_map);

        // Spawn persistent server task
        tokio::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
                info!("[OAuth] Shared callback server shutting down");
            });

            if let Err(e) = server.await {
                error!("[OAuth] Shared callback server error: {}", e);
            }
        });

        // Store server state
        *server_guard = Some(CallbackServerState {
            port,
            _shutdown_tx: shutdown_tx,
        });

        Ok(port)
    }

    /// Register a pending OAuth flow and get a receiver for the callback
    ///
    /// This creates a oneshot channel for this specific flow and registers it
    /// in `pending_by_state` so the shared callback server can route to it.
    fn register_pending_flow(
        &self,
        state: String,
        space_id: Uuid,
        server_id: String,
        server_url: String,
    ) -> tokio::sync::oneshot::Receiver<OAuthCallback> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        let flow = PendingOAuthFlow {
            space_id,
            server_id: server_id.clone(),
            server_url,
            started_at: std::time::Instant::now(),
            callback_tx: tx,
        };

        // Register by state for callback routing
        self.pending_by_state.insert(state.clone(), flow);

        // Track active flow by server
        self.active_by_server.insert((space_id, server_id), state);

        rx
    }

    /// Handle an OAuth callback received via deep link (legacy/fallback)
    ///
    /// This is called by the desktop app when it receives a deep link.
    /// With loopback redirect, this is typically not used, but kept for
    /// potential future custom scheme support.
    ///
    /// Routes the callback to the appropriate pending flow based on the state parameter.
    pub fn handle_callback(&self, callback: OAuthCallback) -> Result<(), String> {
        let state = &callback.state;
        let state_short = if state.len() > 8 { &state[..8] } else { state };

        info!("[OAuth] Deep link callback received, state={}", state_short);

        // Look up the pending flow by state
        match self.pending_by_state.remove(state) {
            Some((_, flow)) => {
                info!(
                    "[OAuth] Found pending flow for server={}, forwarding callback",
                    flow.server_id
                );

                // Send callback to the waiting task
                match flow.callback_tx.send(callback) {
                    Ok(_) => {
                        info!(
                            "[OAuth] Callback sent to waiting task for {}",
                            flow.server_id
                        );
                        Ok(())
                    }
                    Err(_) => {
                        warn!("[OAuth] Waiting task already gone for {}", flow.server_id);
                        Err("OAuth flow already completed or timed out".to_string())
                    }
                }
            }
            None => {
                // Check if state exists in oauth_states (callback for completed/cancelled flow)
                if self.oauth_states.contains_key(state) {
                    warn!(
                        "[OAuth] Duplicate callback for state={} - already processed",
                        state_short
                    );
                    Err("OAuth callback already processed".to_string())
                } else {
                    warn!("[OAuth] Unknown state={} - expired or invalid", state_short);
                    Err("Unknown or expired OAuth state".to_string())
                }
            }
        }
    }

    /// Get stored token info (for status checks)
    pub async fn get_stored_token(
        &self,
        credential_repo: &dyn CredentialRepository,
        space_id: Uuid,
        server_id: &str,
    ) -> Option<OAuthTokenInfo> {
        // Load access token row
        let access_cred = match credential_repo
            .get(&space_id, server_id, &CredentialType::AccessToken)
            .await
        {
            Ok(Some(cred)) => cred,
            Ok(None) => return None,
            Err(e) => {
                warn!(
                    "[OAuth] Failed to load access token for {}/{}: {}",
                    space_id, server_id, e
                );
                return None;
            }
        };

        // Load refresh token row (optional)
        let refresh_token = match credential_repo
            .get(&space_id, server_id, &CredentialType::RefreshToken)
            .await
        {
            Ok(Some(cred)) => Some(cred.value),
            _ => None,
        };

        Some(OAuthTokenInfo {
            access_token: access_cred.value,
            refresh_token,
            expires_at: access_cred.expires_at,
            token_type: access_cred
                .token_type
                .unwrap_or_else(|| "Bearer".to_string()),
            scope: access_cred.scope,
        })
    }

    /// Create an AuthorizationManager with our database-backed credential store.
    pub async fn create_auth_manager(
        &self,
        credential_repo: Arc<dyn CredentialRepository>,
        backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
        space_id: Uuid,
        server_id: &str,
        server_url: &str,
    ) -> Result<AuthorizationManager> {
        let mut manager = AuthorizationManager::new(server_url)
            .await
            .context("Failed to create authorization manager")?;

        // Attach our database-backed credential store
        let store = DatabaseCredentialStore::new(
            space_id,
            server_id,
            server_url,
            credential_repo,
            backend_oauth_repo,
        );
        manager.set_credential_store(store);

        // Try to initialize from stored credentials
        if manager.initialize_from_store().await.unwrap_or(false) {
            debug!(
                "[OAuth] Initialized from stored credentials for {}/{}",
                space_id, server_id
            );
        }

        Ok(manager)
    }

    /// Get access token for a server, with automatic refresh if needed.
    pub async fn get_access_token(
        &self,
        credential_repo: Arc<dyn CredentialRepository>,
        backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
        space_id: Uuid,
        server_id: &str,
        server_url: &str,
    ) -> Result<String> {
        let manager = self
            .create_auth_manager(
                credential_repo,
                backend_oauth_repo,
                space_id,
                server_id,
                server_url,
            )
            .await?;

        manager
            .get_access_token()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get access token: {}", e))
    }

    /// Start OAuth flow for a server using SDK's OAuthState
    pub async fn start_oauth_flow(
        &self,
        credential_repo: Arc<dyn CredentialRepository>,
        backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
        space_id: Uuid,
        server_id: &str,
        server_url: &str,
    ) -> Result<OAuthInitResult> {
        let space_id_str = space_id.to_string();
        info!(
            "[OAuth] start_oauth_flow called for {}/{}",
            space_id, server_id
        );

        self.log(
            &space_id_str,
            server_id,
            LogLevel::Info,
            format!("Starting OAuth flow for server: {}", server_url),
            Some(serde_json::json!({"server_url": server_url})),
        )
        .await;

        if self.is_pending(space_id, server_id) {
            warn!(
                "[OAuth] Flow already pending for {}/{}; rejecting new request",
                space_id, server_id
            );
            return Err(anyhow::anyhow!(
                "OAuth flow already in progress for {}/{}",
                space_id,
                server_id
            ));
        }

        info!(
            "[OAuth] No pending flow for {}/{}; proceeding",
            space_id, server_id
        );

        // Check for existing valid/refreshable credentials
        if let Some(token_info) = self
            .get_stored_token(credential_repo.as_ref(), space_id, server_id)
            .await
        {
            if !token_info.is_expired() || token_info.can_refresh() {
                // Try to get access token (triggers refresh if needed)
                match self
                    .get_access_token(
                        credential_repo.clone(),
                        backend_oauth_repo.clone(),
                        space_id,
                        server_id,
                        server_url,
                    )
                    .await
                {
                    Ok(_) => {
                        info!("[OAuth] Valid token exists for {}/{}", space_id, server_id);
                        return Ok(OAuthInitResult::AlreadyAuthorized);
                    }
                    Err(e) => {
                        debug!("[OAuth] Token check failed ({}), starting fresh flow", e);
                    }
                }
            }
        }

        info!("[OAuth] Starting OAuth flow for {}/{}", space_id, server_id);

        // Ensure shared callback server is running (RFC 8252 Section 7.3)
        // This server is shared across all concurrent OAuth flows
        let callback_port = match self.ensure_callback_server().await {
            Ok(port) => port,
            Err(e) => {
                self.log(
                    &space_id_str,
                    server_id,
                    LogLevel::Error,
                    format!("Failed to start callback server: {}", e),
                    Some(serde_json::json!({"error": e.to_string()})),
                )
                .await;
                return Err(e);
            }
        };

        // Use loopback redirect URI with the shared port
        let redirect_uri = Self::get_redirect_uri_with_port(callback_port);

        info!(
            "[OAuth] Using shared callback server on port {}, redirect_uri={}",
            callback_port, redirect_uri
        );

        // Check for existing client_id (from previous DCR)
        let existing_registration = backend_oauth_repo
            .get(&space_id, server_id)
            .await
            .ok()
            .flatten();

        // Create OAuthState using the SDK (this performs metadata discovery/handshake)
        self.log(
            &space_id_str,
            server_id,
            LogLevel::Info,
            "Discovering OAuth metadata (RFC 8414 handshake)".to_string(),
            Some(serde_json::json!({"server_url": server_url})),
        )
        .await;

        let oauth_state_result = OAuthState::new(server_url, None).await;
        let mut oauth_state = match oauth_state_result {
            Ok(state) => state,
            Err(e) => {
                self.log(
                    &space_id_str,
                    server_id,
                    LogLevel::Error,
                    format!("OAuth metadata discovery failed: {}", e),
                    Some(serde_json::json!({"error": e.to_string()})),
                )
                .await;
                return Err(anyhow::anyhow!("Failed to create OAuth state: {}", e));
            }
        };

        self.log(
            &space_id_str,
            server_id,
            LogLevel::Info,
            "OAuth metadata discovery completed successfully".to_string(),
            None,
        )
        .await;

        // Set our credential store on the manager
        if let OAuthState::Unauthorized(ref mut manager) = oauth_state {
            let store = DatabaseCredentialStore::new(
                space_id,
                server_id,
                server_url,
                credential_repo.clone(),
                backend_oauth_repo.clone(),
            );
            manager.set_credential_store(store);
        }

        // Use app name + space name for DCR to help users identify registrations
        // e.g., "McpMux (Work)" vs "McpMux (Personal)"
        let client_name = self.get_client_name_for_space(space_id).await;

        // Check if we can reuse existing DCR (redirect_uri must match)
        let can_reuse_dcr = existing_registration
            .as_ref()
            .map(|reg| reg.matches_redirect_uri(&redirect_uri))
            .unwrap_or(false);

        // Track whether this is a new registration and capture discovered metadata
        let (is_new_registration, discovered_metadata): (
            bool,
            Option<mcpmux_core::StoredOAuthMetadata>,
        ) = if can_reuse_dcr {
            // REUSE EXISTING CLIENT_ID - redirect_uri matches!
            let reg = existing_registration.as_ref().unwrap();
            info!(
                "[OAuth] Reusing existing client_id={} for {}/{} (redirect_uri matches)",
                reg.client_id, space_id, server_id
            );

            self.log(
                &space_id_str,
                server_id,
                LogLevel::Info,
                format!("Reusing existing DCR client_id: {}", reg.client_id),
                Some(serde_json::json!({
                    "client_id": reg.client_id,
                    "redirect_uri": redirect_uri,
                    "action": "reuse_dcr"
                })),
            )
            .await;

            if let OAuthState::Unauthorized(ref mut manager) = oauth_state {
                // Discover metadata and configure the existing client
                self.log(
                    &space_id_str,
                    server_id,
                    LogLevel::Info,
                    "Configuring existing client with OAuth server".to_string(),
                    Some(serde_json::json!({"client_id": reg.client_id})),
                )
                .await;

                // First discover OAuth metadata to get supported scopes
                let discovered_metadata = match self
                    .ensure_metadata_with_origin_fallback(
                        manager,
                        server_url,
                        &space_id_str,
                        server_id,
                    )
                    .await
                {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        self.log(
                            &space_id_str,
                            server_id,
                            LogLevel::Error,
                            format!("Failed to discover OAuth metadata: {}", e),
                            Some(serde_json::json!({"error": e.to_string()})),
                        )
                        .await;
                        return Err(anyhow::anyhow!("Failed to discover metadata: {}", e));
                    }
                };

                // Get scopes from discovered metadata
                let scopes = Self::get_scopes_from_metadata(&discovered_metadata);

                // Then configure client with the existing registration
                let config = rmcp::transport::auth::OAuthClientConfig {
                    client_id: reg.client_id.clone(),
                    client_secret: None,
                    scopes: scopes.clone(),
                    redirect_uri: redirect_uri.clone(),
                };

                if let Err(e) = manager.configure_client(config) {
                    self.log(
                        &space_id_str,
                        server_id,
                        LogLevel::Error,
                        format!("Failed to configure existing client: {}", e),
                        Some(serde_json::json!({"error": e.to_string()})),
                    )
                    .await;
                    return Err(anyhow::anyhow!("Failed to configure client: {}", e));
                }

                self.log(
                    &space_id_str,
                    server_id,
                    LogLevel::Info,
                    "Client configuration completed".to_string(),
                    None,
                )
                .await;

                // Generate authorization URL with server-supported scopes
                let scope_refs = Self::scopes_as_refs(&scopes);
                let auth_url = manager
                    .get_authorization_url(&scope_refs)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to get auth URL: {}", e))?;

                // Create session manually
                oauth_state = OAuthState::Session(rmcp::transport::auth::AuthorizationSession {
                    auth_manager: std::mem::replace(
                        manager,
                        rmcp::transport::auth::AuthorizationManager::new(server_url)
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed: {}", e))?,
                    ),
                    auth_url: auth_url.clone(),
                    redirect_uri: redirect_uri.clone(),
                });
            }
            (false, None) // Not a new registration, no metadata to save
        } else {
            // Need fresh DCR - either no existing registration OR port changed
            if let Some(ref reg) = existing_registration {
                warn!(
                    "[OAuth] Port changed! Old redirect_uri={:?}, new={} - deleting old DCR and re-registering",
                    reg.redirect_uri, redirect_uri
                );
                // Delete old registration since port changed
                if let Err(e) = backend_oauth_repo.delete(&space_id, server_id).await {
                    warn!("[OAuth] Failed to delete old DCR: {}", e);
                }
            } else {
                info!(
                    "[OAuth] No existing client_id for {}/{} - doing fresh DCR",
                    space_id, server_id
                );
            }

            self.log(
                &space_id_str,
                server_id,
                LogLevel::Info,
                "Starting Dynamic Client Registration (DCR)".to_string(),
                Some(serde_json::json!({
                    "redirect_uri": redirect_uri,
                    "client_name": client_name,
                    "action": "new_dcr"
                })),
            )
            .await;

            let manager = match std::mem::replace(
                &mut oauth_state,
                OAuthState::Unauthorized(AuthorizationManager::new(server_url).await?),
            ) {
                OAuthState::Unauthorized(manager) => manager,
                _ => {
                    return Err(anyhow::anyhow!(
                        "OAuth state is not Unauthorized; cannot start authorization"
                    ));
                }
            };

            let mut manager = manager;
            let metadata_for_storage = match self
                .ensure_metadata_with_origin_fallback(
                    &mut manager,
                    server_url,
                    &space_id_str,
                    server_id,
                )
                .await
            {
                Ok(metadata) => Some(metadata),
                Err(e) => {
                    self.log(
                        &space_id_str,
                        server_id,
                        LogLevel::Error,
                        format!("Failed to discover OAuth metadata: {}", e),
                        Some(serde_json::json!({"error": e.to_string()})),
                    )
                    .await;
                    return Err(anyhow::anyhow!("Failed to discover metadata: {}", e));
                }
            };

            // Get scopes from discovered metadata (or use empty if not available)
            let scopes = metadata_for_storage
                .as_ref()
                .map(Self::get_scopes_from_metadata)
                .unwrap_or_default();
            let scope_refs = Self::scopes_as_refs(&scopes);

            match AuthorizationSession::new(
                manager,
                &scope_refs,
                &redirect_uri,
                Some(&client_name),
                None,
            )
            .await
            {
                Ok(session) => {
                    oauth_state = OAuthState::Session(session);
                    self.log(
                        &space_id_str,
                        server_id,
                        LogLevel::Info,
                        "Dynamic Client Registration (DCR) completed successfully".to_string(),
                        None,
                    )
                    .await;
                }
                Err(AuthError::NoAuthorizationSupport) => {
                    self.log(
                        &space_id_str,
                        server_id,
                        LogLevel::Warn,
                        "OAuth not supported: Server doesn't support OAuth discovery".to_string(),
                        None,
                    )
                    .await;
                    return Ok(OAuthInitResult::NotSupported(
                        "Server doesn't support OAuth discovery".to_string(),
                    ));
                }
                Err(AuthError::RegistrationFailed(msg)) => {
                    self.log(
                        &space_id_str,
                        server_id,
                        LogLevel::Error,
                        format!("DCR registration failed: {}", msg),
                        Some(serde_json::json!({"error": msg})),
                    )
                    .await;
                    return Err(anyhow::anyhow!("Client registration failed: {}", msg));
                }
                Err(e) => {
                    self.log(
                        &space_id_str,
                        server_id,
                        LogLevel::Error,
                        format!("OAuth flow failed: {}", e),
                        Some(serde_json::json!({"error": e.to_string()})),
                    )
                    .await;
                    return Err(anyhow::anyhow!("OAuth flow failed: {}", e));
                }
            }
            (true, metadata_for_storage) // New registration with discovered metadata
        };

        // Get authorization URL
        let auth_url_result = oauth_state.get_authorization_url().await;
        let auth_url = match auth_url_result {
            Ok(url) => url,
            Err(e) => {
                self.log(
                    &space_id_str,
                    server_id,
                    LogLevel::Error,
                    format!("Failed to get authorization URL: {}", e),
                    Some(serde_json::json!({"error": e.to_string()})),
                )
                .await;
                return Err(anyhow::anyhow!("Failed to get auth URL: {}", e));
            }
        };

        // Add RFC 8707 'resource' parameter to the authorization URL.
        // This tells the Authorization Server which protected resource (MCP server)
        // the token is being requested for. Some servers (like Miro) require this.
        let auth_url = Self::add_resource_parameter(&auth_url, server_url);

        // Extract state parameter from auth_url
        let state = match Self::extract_state_from_url(&auth_url) {
            Some(s) => s,
            None => {
                self.log(
                    &space_id_str,
                    server_id,
                    LogLevel::Error,
                    "Failed to extract state parameter from auth URL".to_string(),
                    None,
                )
                .await;
                return Err(anyhow::anyhow!("Failed to extract state from auth URL"));
            }
        };

        info!(
            "[OAuth] Auth URL ready, state={}: {}",
            &state[..8.min(state.len())],
            auth_url
        );

        self.log(
            &space_id_str,
            server_id,
            LogLevel::Info,
            format!(
                "Authorization URL ready - browser should open (loopback callback: {})",
                redirect_uri
            ),
            Some(serde_json::json!({
                "auth_url": auth_url,
                "redirect_uri": redirect_uri,
                "callback_port": callback_port,
                "state": &state[..8.min(state.len())]
            })),
        )
        .await;

        // Register this flow for callback routing (shared server routes by state parameter)
        let callback_rx = self.register_pending_flow(
            state.clone(),
            space_id,
            server_id.to_string(),
            server_url.to_string(),
        );

        // Store OAuth state for token exchange (keyed by state)
        self.oauth_states
            .insert(state.clone(), Arc::new(Mutex::new(oauth_state)));

        // Spawn callback handler task (callback_rx will receive from shared server)
        let server_id_clone = server_id.to_string();
        let state_clone = state.clone();
        let completed = self.completed_flows.clone();
        let oauth_states = self.oauth_states.clone();
        let active_by_server = self.active_by_server.clone();
        let space_id_for_cleanup = space_id;
        let server_id_for_cleanup = server_id.to_string();
        let timeout = self.timeout;
        let completion_tx = self.completion_tx.clone();
        let backend_oauth_repo_clone = backend_oauth_repo.clone();
        let server_url_clone = server_url.to_string();
        let redirect_uri_clone = redirect_uri.clone();

        // Clone log manager for the spawned task
        let log_manager_clone = self.log_manager.clone();
        let space_id_str_clone = space_id_str.clone();
        let discovered_metadata_clone = discovered_metadata.clone();

        tokio::spawn(async move {
            info!(
                "[OAuth] Waiting for callback for {} (timeout={}s)",
                server_id_clone,
                timeout.as_secs()
            );

            // Log waiting state
            if let Some(log_manager) = &log_manager_clone {
                let log = ServerLog::new(
                    LogLevel::Info,
                    LogSource::OAuth,
                    format!(
                        "Waiting for OAuth callback (timeout: {}s)",
                        timeout.as_secs()
                    ),
                );
                let _ = log_manager
                    .append(&space_id_str_clone, &server_id_clone, log)
                    .await;
            }

            let result = tokio::time::timeout(timeout, callback_rx).await;

            match result {
                Ok(Ok(callback)) => {
                    // Check for OAuth error first
                    if let Some(ref error) = callback.error {
                        let error_msg = callback
                            .error_description
                            .as_ref()
                            .map(|d| format!("{}: {}", error, d))
                            .unwrap_or_else(|| error.clone());

                        error!(
                            "[OAuth] Authorization failed for {}: {}",
                            server_id_clone, error_msg
                        );

                        if let Some(log_manager) = &log_manager_clone {
                            let log = ServerLog::new(
                                LogLevel::Error,
                                LogSource::OAuth,
                                format!("Authorization denied: {}", error_msg),
                            )
                            .with_metadata(serde_json::json!({
                                "error": error,
                                "error_description": callback.error_description,
                            }));
                            let _ = log_manager
                                .append(&space_id_str_clone, &server_id_clone, log)
                                .await;
                        }

                        let _ = completion_tx.send(OAuthCompleteEvent {
                            space_id,
                            server_id: server_id_clone.clone(),
                            success: false,
                            error: Some(error_msg),
                        });
                        return;
                    }

                    // Check for authorization code
                    let code = match &callback.code {
                        Some(c) => c.clone(),
                        None => {
                            error!("[OAuth] Missing authorization code for {}", server_id_clone);

                            if let Some(log_manager) = &log_manager_clone {
                                let log = ServerLog::new(
                                    LogLevel::Error,
                                    LogSource::OAuth,
                                    "Missing authorization code in callback".to_string(),
                                );
                                let _ = log_manager
                                    .append(&space_id_str_clone, &server_id_clone, log)
                                    .await;
                            }

                            let _ = completion_tx.send(OAuthCompleteEvent {
                                space_id,
                                server_id: server_id_clone.clone(),
                                success: false,
                                error: Some("Missing authorization code".to_string()),
                            });
                            return;
                        }
                    };

                    info!(
                        "[OAuth] Callback received for {} with code length {}",
                        server_id_clone,
                        code.len()
                    );

                    // Log callback received
                    if let Some(log_manager) = &log_manager_clone {
                        let log = ServerLog::new(
                            LogLevel::Info,
                            LogSource::OAuth,
                            "OAuth callback received from browser".to_string(),
                        )
                        .with_metadata(serde_json::json!({
                            "code_length": code.len(),
                            "state": &callback.state[..8.min(callback.state.len())],
                            "has_error": callback.error.is_some(),
                        }));
                        let _ = log_manager
                            .append(&space_id_str_clone, &server_id_clone, log)
                            .await;
                    }

                    // Get the OAuth state (keyed by state)
                    let oauth_state_arc = match oauth_states.get(&state_clone) {
                        Some(state) => state.clone(),
                        None => {
                            error!("[OAuth] No OAuth state found for {}", server_id_clone);
                            let _ = completion_tx.send(OAuthCompleteEvent {
                                space_id,
                                server_id: server_id_clone.clone(),
                                success: false,
                                error: Some("OAuth state not found".to_string()),
                            });
                            return;
                        }
                    };

                    // Handle callback using SDK (token exchange)
                    let mut oauth_state = oauth_state_arc.lock().await;

                    // Log token exchange start
                    if let Some(log_manager) = &log_manager_clone {
                        let log = ServerLog::new(
                            LogLevel::Info,
                            LogSource::OAuth,
                            "Exchanging authorization code for tokens".to_string(),
                        );
                        let _ = log_manager
                            .append(&space_id_str_clone, &server_id_clone, log)
                            .await;
                    }

                    if let Err(e) = oauth_state.handle_callback(&code, &callback.state).await {
                        error!(
                            "[OAuth] Callback handling failed for {}: {}",
                            server_id_clone, e
                        );

                        if let Some(log_manager) = &log_manager_clone {
                            let log = ServerLog::new(
                                LogLevel::Error,
                                LogSource::OAuth,
                                format!("Token exchange failed: {}", e),
                            )
                            .with_metadata(serde_json::json!({"error": e.to_string()}));
                            let _ = log_manager
                                .append(&space_id_str_clone, &server_id_clone, log)
                                .await;
                        }

                        let _ = completion_tx.send(OAuthCompleteEvent {
                            space_id,
                            server_id: server_id_clone.clone(),
                            success: false,
                            error: Some(format!("Token exchange failed: {}", e)),
                        });
                    } else {
                        info!("[OAuth] Token exchange completed for {}", server_id_clone);

                        if let Some(log_manager) = &log_manager_clone {
                            let log = ServerLog::new(
                                LogLevel::Info,
                                LogSource::OAuth,
                                "Token exchange completed successfully".to_string(),
                            );
                            let _ = log_manager
                                .append(&space_id_str_clone, &server_id_clone, log)
                                .await;
                        }

                        // Save registration if new (includes redirect_uri, port change detection, AND metadata)
                        if is_new_registration {
                            if let Ok((client_id, _)) = oauth_state.get_credentials().await {
                                // Use with_metadata if we have discovered metadata, otherwise use new
                                let registration =
                                    if let Some(ref metadata) = discovered_metadata_clone {
                                        mcpmux_core::OutboundOAuthRegistration::with_metadata(
                                            space_id,
                                            &server_id_clone,
                                            &server_url_clone,
                                            &client_id,
                                            &redirect_uri_clone,
                                            metadata.clone(),
                                        )
                                    } else {
                                        mcpmux_core::OutboundOAuthRegistration::new(
                                            space_id,
                                            &server_id_clone,
                                            &server_url_clone,
                                            &client_id,
                                            &redirect_uri_clone,
                                        )
                                    };
                                if let Err(e) = backend_oauth_repo_clone.save(&registration).await {
                                    error!("[OAuth] Failed to save registration: {}", e);
                                    if let Some(log_manager) = &log_manager_clone {
                                        let log = ServerLog::new(
                                            LogLevel::Error,
                                            LogSource::OAuth,
                                            format!("Failed to save DCR registration: {}", e),
                                        )
                                        .with_metadata(serde_json::json!({"error": e.to_string()}));
                                        let _ = log_manager
                                            .append(&space_id_str_clone, &server_id_clone, log)
                                            .await;
                                    }
                                } else {
                                    info!("[OAuth] Saved new registration for {}/{} with redirect_uri={}", 
                                        space_id, server_id_clone, redirect_uri_clone);
                                    if let Some(log_manager) = &log_manager_clone {
                                        let log = ServerLog::new(
                                            LogLevel::Info,
                                            LogSource::OAuth,
                                            format!(
                                                "Saved DCR registration (client_id: {})",
                                                client_id
                                            ),
                                        )
                                        .with_metadata(serde_json::json!({
                                            "client_id": client_id,
                                            "redirect_uri": redirect_uri_clone
                                        }));
                                        let _ = log_manager
                                            .append(&space_id_str_clone, &server_id_clone, log)
                                            .await;
                                    }
                                }
                            }
                        }

                        // Mark as completed
                        completed.insert(
                            (space_id, server_id_clone.clone()),
                            std::time::Instant::now(),
                        );

                        // Emit success event
                        info!("[OAuth] Emitting success event for {}", server_id_clone);

                        if let Some(log_manager) = &log_manager_clone {
                            let log = ServerLog::new(
                                LogLevel::Info,
                                LogSource::OAuth,
                                "OAuth flow completed successfully - ready to connect".to_string(),
                            );
                            let _ = log_manager
                                .append(&space_id_str_clone, &server_id_clone, log)
                                .await;
                        }

                        let _ = completion_tx.send(OAuthCompleteEvent {
                            space_id,
                            server_id: server_id_clone.clone(),
                            success: true,
                            error: None,
                        });
                    }
                }
                Ok(Err(_)) => {
                    // Sender dropped = flow was cancelled
                    info!("[OAuth] Flow cancelled for {}", server_id_clone);

                    if let Some(log_manager) = &log_manager_clone {
                        let log = ServerLog::new(
                            LogLevel::Warn,
                            LogSource::OAuth,
                            "OAuth flow cancelled by user".to_string(),
                        );
                        let _ = log_manager
                            .append(&space_id_str_clone, &server_id_clone, log)
                            .await;
                    }

                    let _ = completion_tx.send(OAuthCompleteEvent {
                        space_id,
                        server_id: server_id_clone.clone(),
                        success: false,
                        error: Some("Flow cancelled".to_string()),
                    });
                }
                Err(_) => {
                    // Timeout
                    error!(
                        "[OAuth] Timeout waiting for callback for {}",
                        server_id_clone
                    );

                    if let Some(log_manager) = &log_manager_clone {
                        let log = ServerLog::new(
                            LogLevel::Error,
                            LogSource::OAuth,
                            format!("OAuth callback timeout after {}s", timeout.as_secs()),
                        )
                        .with_metadata(serde_json::json!({"timeout_seconds": timeout.as_secs()}));
                        let _ = log_manager
                            .append(&space_id_str_clone, &server_id_clone, log)
                            .await;
                    }

                    let _ = completion_tx.send(OAuthCompleteEvent {
                        space_id,
                        server_id: server_id_clone.clone(),
                        success: false,
                        error: Some(format!("Timeout after {}s", timeout.as_secs())),
                    });
                }
            }

            // Cleanup
            oauth_states.remove(&state_clone);
            active_by_server.remove(&(space_id_for_cleanup, server_id_for_cleanup));
            // Note: pending_by_state not used for loopback callback flows
            info!("[OAuth] Callback handler completed for {}", server_id_clone);
        });

        Ok(OAuthInitResult::Initiated { auth_url })
    }

    /// Extract state parameter from auth URL
    fn extract_state_from_url(url: &str) -> Option<String> {
        url::Url::parse(url)
            .ok()?
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.to_string())
    }

    /// Get the AuthorizationManager from a completed OAuth flow
    pub async fn get_authorized_manager(
        &self,
        space_id: Uuid,
        server_id: &str,
    ) -> Option<AuthorizationManager> {
        // Lookup state by space+server
        let key = (space_id, server_id.to_string());
        let state_key = self.active_by_server.get(&key).map(|r| r.value().clone())?;

        if let Some((_, state_arc)) = self.oauth_states.remove(&state_key) {
            let state = Arc::try_unwrap(state_arc).ok()?.into_inner();
            state.into_authorization_manager()
        } else {
            None
        }
    }
}

impl Default for OutboundOAuthManager {
    fn default() -> Self {
        Self::new()
    }
}
