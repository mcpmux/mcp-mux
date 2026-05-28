//! Gateway Server
//!
//! HTTP server exposing MCP protocol over Streamable HTTP transport.
//! Self-contained with dependency injection for clean architecture.
//!

mod dependencies;
mod handlers;
pub mod logging_middleware;
pub mod rate_limit;
mod service_container;
mod startup;
mod state;

// Exposed for integration tests that mount these routes against a real
// ServiceContainer — e.g. asserting the OAuth-discovery endpoints 404 when
// inbound auth is disabled, and driving the full inbound OAuth flow
// (register → authorize → consent → token → authenticated /mcp) end to end.
// AppState is also used throughout this module.
pub(crate) use handlers::effective_base_url;
pub use handlers::{
    oauth_authorize, oauth_consent_approve, oauth_metadata, oauth_register, oauth_token,
    resource_metadata, AppState,
};

pub use dependencies::{DependenciesBuilder, GatewayDependencies};
pub use handlers::PendingAuthorization;
pub use service_container::ServiceContainer;
pub use startup::{AutoConnectResult, StartupOrchestrator, TokenRefreshResult};
pub use state::{ClientSession, ConsentUiNotifier, GatewayState};

use axum::{
    extract::ConnectInfo,
    middleware,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};

use crate::consumers::MCPNotifier;
use crate::mcp::{mcp_oauth_middleware, McpMuxGatewayHandler};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;

/// Gateway server configuration
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Host to bind to
    pub host: String,
    /// Port to listen on
    pub port: u16,
    /// Public base URL advertised in OAuth metadata.
    ///
    /// When unset, the gateway behaves as a local-only server and advertises
    /// `http://localhost:<port>`. When set, this must be the externally
    /// reachable origin fronting the gateway, for example a Cloudflare Tunnel
    /// URL such as `https://mcp.example.com`.
    pub public_base_url: Option<String>,
    /// Enable CORS for browser access
    pub enable_cors: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: mcpmux_core::branding::DEFAULT_GATEWAY_PORT,
            public_base_url: None,
            enable_cors: true,
        }
    }
}

impl GatewayConfig {
    /// Get the socket address
    pub fn addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .expect("Invalid address")
    }

    /// Get the base URL this gateway advertises to MCP/OAuth clients.
    pub fn base_url(&self) -> String {
        self.public_base_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(|url| url.trim_end_matches('/').to_string())
            .unwrap_or_else(|| format!("http://localhost:{}", self.port))
    }

    /// True when the gateway binds to a non-loopback address (e.g. `0.0.0.0`
    /// or a specific LAN interface) — i.e. it is intentionally exposed on the
    /// network rather than being local-only.
    pub fn is_network_bind(&self) -> bool {
        let host = self.host.trim();
        !(host.is_empty() || host == "127.0.0.1" || host == "::1" || host == "localhost")
    }

    /// Host values accepted by rmcp's DNS rebinding protection.
    ///
    /// rmcp's Streamable HTTP service defaults to loopback-only Host headers.
    /// When the gateway is published through a reverse proxy or Cloudflare
    /// Tunnel, ChatGPT reaches it with the public host, so that hostname must
    /// be explicitly allowlisted.
    ///
    /// When bound to a non-loopback address the gateway is exposed on the LAN
    /// and reached by IP / hostname / mDNS name we can't enumerate ahead of
    /// time, so the allowlist is relaxed to empty — rmcp treats an empty list
    /// as allow-all. The OAuth + per-client consent layer remains the gate.
    pub fn allowed_hosts(&self) -> Vec<String> {
        if self.is_network_bind() {
            return Vec::new();
        }

        let mut hosts = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "::1".to_string(),
        ];

        let bind_host = self.host.trim();
        if !bind_host.is_empty() {
            hosts.push(bind_host.to_string());
            hosts.push(format!("{}:{}", bind_host, self.port));
        }

        if let Some(public_base_url) = self.public_base_url.as_deref() {
            let trimmed = public_base_url.trim();
            if !trimmed.is_empty() {
                match url::Url::parse(trimmed) {
                    Ok(parsed) => {
                        if let Some(host) = parsed.host_str() {
                            hosts.push(host.to_string());
                            if let Some(port) = parsed.port() {
                                hosts.push(format!("{}:{}", host, port));
                            }
                        }
                    }
                    Err(error) => {
                        warn!(
                            public_base_url = trimmed,
                            error = %error,
                            "[Gateway] Ignoring invalid public_base_url for allowed host list"
                        );
                    }
                }
            }
        }

        hosts.sort();
        hosts.dedup();
        hosts
    }
}

/// The desktop-only client-management routes (list / update / delete clients).
/// `/oauth/clients/{id}/features` is intentionally excluded — it is the public
/// client-facing endpoint.
fn is_management_path(path: &str) -> bool {
    path == "/oauth/clients"
        || (path.starts_with("/oauth/clients/") && !path.ends_with("/features"))
}

/// Reject the desktop-only client-management endpoints when the request comes
/// from a non-loopback peer.
///
/// On a loopback bind every peer is local, so this is a no-op. On a `0.0.0.0`
/// (network) bind the whole router is exposed, but client enumeration / CRUD
/// must stay off the LAN — the OAuth flow and `/oauth/clients/{id}/features`
/// remain reachable. The peer socket address (not the spoofable `Host` header)
/// is the trust signal. Falls open only when no peer address is available
/// (an embedded/test server without `ConnectInfo`), which never happens on the
/// real network listener.
async fn restrict_management_to_loopback(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    if is_management_path(request.uri().path()) {
        let peer_is_local = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip().is_loopback())
            .unwrap_or(true);
        if !peer_is_local {
            warn!("[Gateway] Rejected non-loopback access to a client-management endpoint");
            return (
                axum::http::StatusCode::FORBIDDEN,
                "Client management is only available from this machine",
            )
                .into_response();
        }
    }
    next.run(request).await
}

/// MCP Gateway Server
///
/// Self-contained server that manages its own services and lifecycle.
/// Follows Dependency Injection pattern - all external dependencies
/// are injected through the constructor.
pub struct GatewayServer {
    config: GatewayConfig,
    state: Arc<RwLock<GatewayState>>,
    services: ServiceContainer,
}

impl GatewayServer {
    /// Create a new gateway server with dependency injection
    ///
    /// This constructor accepts all external dependencies, making the
    /// Gateway testable and environment-agnostic (Desktop, CLI, tests).
    pub fn new(config: GatewayConfig, dependencies: GatewayDependencies) -> Self {
        info!("[Gateway] Initializing with dependency injection...");

        // Create broadcast channel for unified event system
        let (domain_event_tx, _) = tokio::sync::broadcast::channel(256);

        // Configure gateway state
        let mut state = GatewayState::new(domain_event_tx.clone());
        state.set_base_url(config.base_url());
        state.set_public_base_url(config.public_base_url.clone());
        state.set_network_bind(config.is_network_bind());
        if let Some(jwt_secret) = dependencies.jwt_secret.clone() {
            state.set_jwt_secret(jwt_secret);
        }
        let state = Arc::new(RwLock::new(state));

        // Set database and services in state (needs async, so we block here)
        let local_machine_id = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut state_guard = state.write().await;
                state_guard.set_database(dependencies.database.clone());
                state_guard
                    .set_client_metadata_service(dependencies.client_metadata_service.clone());

                if let Some(ref settings_repo) = dependencies.settings_repo {
                    mcpmux_core::AppSettingsService::new(settings_repo.clone())
                        .get_local_machine_id()
                        .await
                } else {
                    None
                }
            })
        });

        // Initialize all services using DI container (pass domain event sender for non-blocking emission)
        let services = ServiceContainer::initialize(
            &dependencies,
            domain_event_tx,
            state.clone(),
            local_machine_id,
        );

        info!("[Gateway] Services initialized successfully");

        Self {
            config,
            state,
            services,
        }
    }

    /// Get a reference to the gateway state
    pub fn state(&self) -> Arc<RwLock<GatewayState>> {
        self.state.clone()
    }

    /// Get the pool service
    pub fn pool_service(&self) -> Arc<crate::pool::PoolService> {
        self.services.pool_services.pool_service.clone()
    }

    /// Get the server manager
    pub fn server_manager(&self) -> Arc<crate::pool::ServerManager> {
        self.services.server_manager.clone()
    }

    /// Get the feature service
    pub fn feature_service(&self) -> Arc<crate::pool::FeatureService> {
        self.services.pool_services.feature_service.clone()
    }

    /// Get the connection service
    pub fn connection_service(&self) -> Arc<crate::pool::ConnectionService> {
        self.services.pool_services.connection_service.clone()
    }

    /// Get the token service
    pub fn token_service(&self) -> Arc<crate::pool::TokenService> {
        self.services.pool_services.token_service.clone()
    }

    /// Get the event emitter (for external components to trigger notifications)
    pub fn event_emitter(&self) -> Arc<crate::services::EventEmitter> {
        let state = tokio::task::block_in_place(|| self.state.blocking_read());
        let event_tx = state.domain_event_sender();
        Arc::new(crate::services::EventEmitter::new(event_tx))
    }

    /// Get the grant service (centralized grant management with auto-notifications)
    pub fn grant_service(&self) -> Arc<crate::services::GrantService> {
        self.services.grant_service.clone()
    }

    /// Approval broker for meta-tool writes. Exposed so the desktop layer
    /// can attach a Tauri-event publisher + resolve pending prompts.
    pub fn approval_broker(&self) -> Arc<crate::services::ApprovalBroker> {
        self.services.approval_broker.clone()
    }

    /// Session-roots registry (MCP roots reported by connected peers).
    ///
    /// The desktop Workspaces tab reads this to surface every folder
    /// clients are currently operating in — both bound and unbound — so
    /// users can configure mappings even for roots they missed the
    /// one-shot prompt for.
    pub fn session_roots(&self) -> Arc<crate::services::SessionRootsRegistry> {
        self.services.session_roots.clone()
    }

    /// Get the OAuth manager
    pub fn oauth_manager(&self) -> Arc<crate::pool::OutboundOAuthManager> {
        self.services.pool_services.oauth_manager.clone()
    }

    /// Auto-connect all enabled servers
    ///
    /// This is called automatically during startup in a background task.
    /// Follows Single Responsibility - delegated to StartupOrchestrator.
    async fn auto_connect_servers(&self) {
        match self
            .services
            .startup_orchestrator
            .auto_connect_enabled_servers()
            .await
        {
            Ok(result) => {
                info!(
                    "[Gateway] Auto-connect complete: {} connected, {} needs OAuth, {} failed",
                    result.connected.len() + result.already_connected.len(),
                    result.needs_oauth.len(),
                    result.failed.len()
                );
            }
            Err(e) => {
                warn!("[Gateway] Auto-connect failed: {}", e);
            }
        }
    }

    /// Build the Axum router
    fn build_router(&self) -> Router {
        let state = self.state.clone();

        // Create app state with services
        let app_state = AppState {
            gateway_state: state.clone(),
            services: Arc::new(self.services.clone()),
            base_url: self.config.base_url(),
        };

        // Create MCP notifier (session-keyed fanout, consults the same
        // FeatureSet resolver the request handlers use).
        let notification_bridge = Arc::new(MCPNotifier::new(
            self.services.feature_set_resolver.clone(),
            self.services.pool_services.feature_service.clone(),
        ));

        // Start listening to DomainEvents
        {
            let gw_state = tokio::task::block_in_place(|| state.blocking_read());
            let event_rx = gw_state.subscribe_domain_events();
            notification_bridge.clone().start(event_rx);
        }

        // Create OAuth event handler (updates oauth_connected flag on OAuth success)
        {
            let oauth_handler = Arc::new(crate::consumers::OAuthEventHandler::new(
                self.services.dependencies.installed_server_repo.clone(),
            ));
            let oauth_rx = self
                .services
                .pool_services
                .pool_service
                .oauth_manager()
                .subscribe();
            oauth_handler.start(oauth_rx);
        }

        // Create MCP handler
        let handler =
            McpMuxGatewayHandler::new(Arc::new(self.services.clone()), notification_bridge.clone());

        // Create STATEFUL MCP service (full Streamable HTTP per spec 2025-11-25)
        // stateful_mode: true means:
        // - Mcp-Session-Id header for session management
        // - GET endpoint for SSE streams (server-initiated notifications)
        // - DELETE endpoint for session termination
        // - list_changed notifications delivered via SSE
        // Build via default() + setters so new non-exhaustive fields (e.g. allowed_hosts,
        // which defaults to localhost/127.0.0.1/::1) don't require us to enumerate them.
        let mut http_cfg = StreamableHttpServerConfig::default();
        http_cfg.stateful_mode = true;
        http_cfg.json_response = false;
        http_cfg.allowed_hosts = self.config.allowed_hosts();
        info!(
            "[Gateway] MCP allowed Host headers: {:?}",
            http_cfg.allowed_hosts
        );
        http_cfg.sse_keep_alive = Some(std::time::Duration::from_secs(30));
        http_cfg.sse_retry = Some(std::time::Duration::from_secs(3));
        http_cfg.cancellation_token = CancellationToken::new();
        let mcp_service = StreamableHttpService::new(
            move || {
                debug!("[Gateway] Creating handler instance for MCP session");
                Ok(handler.clone())
            },
            LocalSessionManager::default().into(),
            http_cfg,
        );

        // Wrap MCP service with OAuth middleware
        let mcp_routes =
            Router::new()
                .nest_service("/mcp", mcp_service)
                .layer(middleware::from_fn_with_state(
                    Arc::new(self.services.clone()),
                    mcp_oauth_middleware,
                ));

        // Client features endpoint (needs services, public)
        // Supports both DCR (simple IDs) and CIMD (URL-encoded IDs)
        // Clients should URL-encode client_ids that contain special characters
        let client_features_routes = Router::new()
            .route(
                "/oauth/clients/{client_id}/features",
                get(handlers::oauth_get_client_features),
            )
            .with_state(app_state.clone());

        let mut router = Router::new()
            // Health check (public)
            .route("/health", get(handlers::health))
            // OAuth endpoints (public) - use app_state for base_url access
            .route(
                "/.well-known/oauth-authorization-server",
                get(handlers::oauth_metadata),
            )
            // Some remote MCP clients, including ChatGPT connector flows, probe
            // the resource-scoped authorization-server metadata path.
            .route(
                "/.well-known/oauth-authorization-server/mcp",
                get(handlers::oauth_metadata),
            )
            .route(
                "/.well-known/oauth-protected-resource",
                get(handlers::resource_metadata),
            )
            // RFC 9728: Resource-specific metadata endpoint
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                get(handlers::resource_metadata),
            )
            // Other OAuth endpoints still need GatewayState
            .route("/oauth/authorize", get(handlers::oauth_authorize))
            // Fallback for clients that don't fetch metadata (VS Code default behavior)
            .route("/authorize", get(handlers::oauth_authorize))
            .route("/oauth/token", post(handlers::oauth_token))
            // NOTE: /oauth/consent/approve was removed for security.
            // Consent approval now happens exclusively via Tauri IPC command
            // (approve_oauth_consent), which can only be invoked by the desktop
            // app's own WebView—not by external HTTP clients, scripts, or bots.
            // Client registration (DCR - public)
            .route("/oauth/register", post(handlers::oauth_register))
            // Client management (for desktop app)
            .route("/oauth/clients", get(handlers::oauth_list_clients))
            // Client CRUD - expects URL-encoded client_id for CIMD clients
            .route(
                "/oauth/clients/{client_id}",
                put(handlers::oauth_update_client),
            )
            .route(
                "/oauth/clients/{client_id}",
                delete(handlers::oauth_delete_client),
            );

        // E2E test mode: re-enable HTTP consent endpoint (guarded by env var).
        // In production this endpoint does NOT exist—consent is Tauri-IPC-only.
        if std::env::var("MCPMUX_E2E_TEST").is_ok() {
            warn!("[Gateway] E2E test mode: /oauth/consent/approve HTTP endpoint enabled");
            router = router.route(
                "/oauth/consent/approve",
                post(handlers::oauth_consent_approve),
            );
        }

        // Rate limiter for OAuth endpoints (prevents abuse / consent flooding)
        let rate_limiter = rate_limit::default_oauth_rate_limiter();

        let mut router = router
            // Protected MCP routes (using rmcp's StreamableHttpService)
            .merge(mcp_routes)
            // Client features (needs services)
            .merge(client_features_routes)
            // Global state for all routes
            .with_state(app_state.clone())
            .layer(TraceLayer::new_for_http())
            // Request/Response logging with body (DEBUG level)
            .layer(middleware::from_fn(
                logging_middleware::http_logging_middleware,
            ))
            // Rate limiting on OAuth endpoints
            .layer(axum::Extension(rate_limiter))
            .layer(middleware::from_fn(rate_limit::rate_limit_middleware))
            // Keep desktop-only client management off the LAN on a 0.0.0.0 bind.
            .layer(middleware::from_fn(restrict_management_to_loopback));

        // Add CORS if enabled
        if self.config.enable_cors {
            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any);
            router = router.layer(cors);
        }

        router
    }

    /// Run the gateway server
    ///
    /// This is the main entry point. It:
    /// 1. Starts auto-connect in background
    /// 2. Starts the HTTP server
    pub async fn run(self) -> anyhow::Result<()> {
        // No external shutdown signal — axum will run until the process
        // exits or its future is dropped. Prefer `spawn()` for anything
        // that wants a clean stop without orphaning the listener socket.
        self.run_with_shutdown(std::future::pending::<()>()).await
    }

    /// Same as `run`, but accepts a shutdown future. When the future
    /// resolves, axum stops accepting new connections, drains in-flight
    /// requests, and closes the TCP listener. Rust `Drop` on the
    /// `TcpListener` then releases the port on the OS — preventing the
    /// orphaned-socket condition that force-killed processes leave behind.
    pub async fn run_with_shutdown(
        self,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        let addr = self.config.addr();

        info!("[Gateway] Starting on {}", addr);
        info!(
            "[Gateway] CORS: {}",
            if self.config.enable_cors {
                "enabled"
            } else {
                "disabled"
            }
        );

        // Log JWT signing status
        {
            let state = self.state.read().await;
            if state.has_jwt_secret() {
                info!("[Gateway] JWT signing: enabled");
            } else {
                warn!("[Gateway] JWT signing: disabled (no secret configured)");
            }
        }

        // MCPNotifier is started in build_router()
        info!("[Gateway] MCPNotifier started (listening to DomainEvents)");

        // Auto-connect enabled servers in background (non-blocking for fast startup)
        // MCP clients will receive list_changed notifications when backends connect
        let self_arc = Arc::new(self);
        let self_for_autoconnect = self_arc.clone();
        tokio::spawn(async move {
            // Step 0: Mark all features unavailable (will be restored when servers connect)
            // This ensures features don't appear available until servers actually reconnect
            if let Err(e) = self_for_autoconnect
                .services
                .startup_orchestrator
                .mark_all_features_unavailable()
                .await
            {
                warn!("[Gateway] Failed to mark features unavailable: {}", e);
            }

            // Step 1: Resolve server prefixes BEFORE connecting (priority-based)
            if let Err(e) = self_for_autoconnect
                .services
                .startup_orchestrator
                .resolve_server_prefixes()
                .await
            {
                warn!("[Gateway] Failed to resolve server prefixes: {}", e);
            }

            // Step 2: Refresh OAuth tokens BEFORE connecting
            // This uses TokenService with proper origin URL fallback (e.g., Atlassian)
            match self_for_autoconnect
                .services
                .startup_orchestrator
                .refresh_oauth_tokens()
                .await
            {
                Ok(result) => {
                    info!(
                        "[Gateway] Token refresh: {} checked, {} ready, {} failed",
                        result.servers_checked, result.tokens_refreshed, result.refresh_failed
                    );
                }
                Err(e) => {
                    warn!("[Gateway] Token refresh failed: {}", e);
                }
            }

            // Step 3: Auto-connect enabled servers (non-blocking)
            // As each server connects, it will emit list_changed notifications
            self_for_autoconnect.auto_connect_servers().await;
        });

        // Build router and start server immediately
        let router = self_arc.build_router();
        let listener = tokio::net::TcpListener::bind(addr).await?;

        info!("[Gateway] Ready to accept connections (servers connecting in background)");

        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            shutdown.await;
            info!("[Gateway] Graceful shutdown signal received — closing listener");
        })
        .await?;

        info!("[Gateway] Listener closed, run_with_shutdown returning");
        Ok(())
    }

    /// Start the server in the background.
    ///
    /// Returns a [`GatewayServerHandle`] with both the `JoinHandle` and a
    /// one-shot shutdown sender. Call `handle.shutdown()` (and then
    /// `.await` the join handle with a timeout) to close the listener
    /// cleanly. Dropping the sender without using it leaves axum running
    /// until its task is aborted — the old behavior.
    pub fn spawn(self) -> GatewayServerHandle {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            self.run_with_shutdown(async move {
                // If the sender is dropped without being used, `rx.await`
                // resolves with `Err` and we treat that as "shut down now"
                // — this makes accidental Drop of the handle release the
                // port instead of orphaning it.
                let _ = rx.await;
            })
            .await
        });
        GatewayServerHandle {
            task,
            shutdown: Some(tx),
        }
    }
}

/// Handle returned by [`GatewayServer::spawn`] — carries the task's
/// `JoinHandle` plus a one-shot shutdown sender for graceful stop.
///
/// Sending on `shutdown` tells axum to drain in-flight requests and close
/// the listener. After sending, await `task` (with a timeout) to let Rust
/// `Drop` release the socket on the OS — otherwise the port stays bound
/// in the kernel until the process exits.
pub struct GatewayServerHandle {
    pub task: tokio::task::JoinHandle<anyhow::Result<()>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl GatewayServerHandle {
    /// Send the graceful-shutdown signal. No-op if already sent (idempotent).
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }

    /// True when no shutdown signal has been sent yet.
    pub fn is_active(&self) -> bool {
        self.shutdown.is_some()
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    fn config_with(public: Option<&str>) -> GatewayConfig {
        GatewayConfig {
            public_base_url: public.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn base_url_defaults_to_localhost() {
        let cfg = config_with(None);
        assert_eq!(cfg.base_url(), format!("http://localhost:{}", cfg.port));
    }

    #[test]
    fn base_url_uses_public_origin_and_trims_trailing_slash() {
        assert_eq!(
            config_with(Some("https://mcp.example.com/")).base_url(),
            "https://mcp.example.com"
        );
    }

    #[test]
    fn allowed_hosts_always_include_loopback() {
        let hosts = config_with(None).allowed_hosts();
        for h in ["localhost", "127.0.0.1", "::1"] {
            assert!(hosts.contains(&h.to_string()), "missing {h} in {hosts:?}");
        }
    }

    #[test]
    fn allowed_hosts_include_public_host_and_optional_port() {
        let hosts = config_with(Some("https://mcp.example.com")).allowed_hosts();
        assert!(hosts.contains(&"mcp.example.com".to_string()), "{hosts:?}");

        let hosts_port = config_with(Some("https://mcp.example.com:8443")).allowed_hosts();
        assert!(
            hosts_port.contains(&"mcp.example.com".to_string()),
            "{hosts_port:?}"
        );
        assert!(
            hosts_port.contains(&"mcp.example.com:8443".to_string()),
            "{hosts_port:?}"
        );
    }

    #[test]
    fn allowed_hosts_ignores_invalid_public_url_without_panicking() {
        let hosts = config_with(Some("not a url")).allowed_hosts();
        assert!(hosts.contains(&"localhost".to_string()));
        assert!(!hosts.iter().any(|h| h.contains("not a url")));
    }

    fn config_on_host(host: &str) -> GatewayConfig {
        GatewayConfig {
            host: host.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn is_network_bind_distinguishes_loopback_from_exposed() {
        for h in ["127.0.0.1", "::1", "localhost", ""] {
            assert!(!config_on_host(h).is_network_bind(), "{h:?} is loopback");
        }
        for h in ["0.0.0.0", "::", "192.168.1.50"] {
            assert!(
                config_on_host(h).is_network_bind(),
                "{h:?} is a network bind"
            );
        }
    }

    #[test]
    fn allowed_hosts_relaxes_to_allow_all_on_network_bind() {
        // rmcp treats an empty allow-list as allow-all; on a network bind we
        // can't enumerate the LAN host clients will use, so we relax to that.
        assert!(config_on_host("0.0.0.0").allowed_hosts().is_empty());
        // Loopback bind keeps the strict allow-list.
        assert!(config_on_host("127.0.0.1")
            .allowed_hosts()
            .contains(&"localhost".to_string()));
    }

    #[test]
    fn management_path_matching_excludes_features_and_oauth_flow() {
        assert!(super::is_management_path("/oauth/clients")); // list
        assert!(super::is_management_path("/oauth/clients/abc123")); // update/delete
                                                                     // Client-facing + OAuth-flow + other routes are NOT loopback-gated.
        assert!(!super::is_management_path("/oauth/clients/abc123/features"));
        assert!(!super::is_management_path("/oauth/authorize"));
        assert!(!super::is_management_path("/oauth/token"));
        assert!(!super::is_management_path("/mcp"));
        assert!(!super::is_management_path("/health"));
    }
}
