//! Gateway Server
//!
//! HTTP server exposing MCP protocol over Streamable HTTP transport.
//! Self-contained with dependency injection for clean architecture.
//!

mod dependencies;
mod handlers;
pub mod logging_middleware;
pub mod management;
pub mod pairing;
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
    pair_claim, pair_page, resource_metadata, AppState,
};

pub use dependencies::{DependenciesBuilder, GatewayDependencies};
pub use handlers::PendingAuthorization;
pub use service_container::ServiceContainer;
pub use startup::{AutoConnectResult, StartupOrchestrator, TokenRefreshResult};
pub use state::{AuthNetworkConflict, ClientSession, GatewayState};

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
    /// Extra Host-header values accepted on a network bind (user-configured:
    /// mDNS aliases, DNS names, reverse-proxy hosts). Bare host or host:port;
    /// bare entries also match with this gateway's port appended.
    pub additional_allowed_hosts: Vec<String>,
    /// Escape hatch: accept ANY Host header on a network bind (pre-hardening
    /// behavior). Off by default — with it off, network binds enforce an
    /// allowlist built from the machine's own addresses/hostname plus
    /// `public_base_url` and `additional_allowed_hosts`.
    pub allow_any_host: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: mcpmux_core::branding::DEFAULT_GATEWAY_PORT,
            public_base_url: None,
            enable_cors: true,
            additional_allowed_hosts: Vec::new(),
            allow_any_host: false,
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

    /// Host values accepted by the gateway's DNS-rebinding protection (fed to
    /// rmcp's Streamable HTTP config AND the router-wide Host middleware —
    /// one source of truth).
    ///
    /// Loopback bind: localhost variants + bind host + `public_base_url` host.
    ///
    /// Network bind: the allowlist is built from everything this machine is
    /// legitimately reachable as — its interface IPs, its hostname (and mDNS
    /// `.local` variant), the bind host when it names a specific interface,
    /// `public_base_url`, and the user-configured
    /// [`Self::additional_allowed_hosts`]. Every entry is accepted bare and
    /// with this gateway's port appended (IPv6 in bracketed form), matching
    /// how Host headers arrive. New LAN addresses acquired after startup need
    /// a gateway restart or an explicit additional-host entry.
    ///
    /// [`Self::allow_any_host`] (explicit, off by default) restores the old
    /// allow-all behavior by returning an empty list — rmcp treats empty as
    /// allow-all and the router middleware is skipped.
    pub fn allowed_hosts(&self) -> Vec<String> {
        if self.is_network_bind() && self.allow_any_host {
            return Vec::new();
        }

        let mut hosts: Vec<String> = Vec::new();
        for h in ["localhost", "127.0.0.1", "::1"] {
            push_host_forms(&mut hosts, h, self.port);
        }

        let bind_host = self.host.trim();
        // Wildcard binds aren't hostnames clients send — skip them; specific
        // interfaces (loopback or a chosen LAN IP) are valid Host values.
        if !bind_host.is_empty() && bind_host != "0.0.0.0" && bind_host != "::" {
            push_host_forms(&mut hosts, bind_host, self.port);
        }

        if self.is_network_bind() {
            for h in machine_hosts() {
                push_host_forms(&mut hosts, &h, self.port);
            }
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

        for extra in &self.additional_allowed_hosts {
            let extra = extra.trim();
            if extra.is_empty() {
                continue;
            }
            if extra.contains(':') && extra.parse::<std::net::Ipv6Addr>().is_err() {
                // Already host:port (or bracketed v6 with port) — take as-is.
                hosts.push(extra.to_string());
            } else {
                push_host_forms(&mut hosts, extra, self.port);
            }
        }

        hosts.sort();
        hosts.dedup();
        hosts
    }
}

/// Push the Host-header forms a client may send for `host` on `port`:
/// bare and `host:port`, with IPv6 addresses additionally in the bracketed
/// forms (`[addr]`, `[addr]:port`) that Host headers actually carry.
fn push_host_forms(hosts: &mut Vec<String>, host: &str, port: u16) {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() {
        return;
    }
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        hosts.push(host.to_string());
        hosts.push(format!("[{host}]"));
        hosts.push(format!("[{host}]:{port}"));
    } else {
        hosts.push(host.to_string());
        hosts.push(format!("{host}:{port}"));
    }
}

/// Everything this machine is legitimately addressed as on the local network:
/// interface IPs (v4 + v6) plus the OS hostname and its mDNS `.local` alias.
/// Best-effort — enumeration failures degrade to the static entries rather
/// than erroring the gateway.
fn machine_hosts() -> Vec<String> {
    let mut out = Vec::new();
    match local_ip_address::list_afinet_netifas() {
        Ok(ifas) => {
            for (_name, ip) in ifas {
                out.push(ip.to_string());
            }
        }
        Err(error) => {
            warn!(%error, "[Gateway] Could not enumerate local interfaces for the Host allowlist");
        }
    }
    if let Ok(name) = hostname::get() {
        if let Some(s) = name.to_str() {
            let s = s.trim().to_ascii_lowercase();
            if !s.is_empty() {
                out.push(s.clone());
                if !s.ends_with(".local") {
                    out.push(format!("{s}.local"));
                }
            }
        }
    }
    out
}

/// Router-wide Host-header enforcement for network binds. rmcp's built-in
/// check only covers the Streamable HTTP service; this covers every route
/// (OAuth authorize/token, client features, future pairing pages) and returns
/// a self-explanatory 421 instead of an opaque rejection. Same source of
/// truth as rmcp: [`GatewayConfig::allowed_hosts`].
async fn enforce_allowed_hosts(
    axum::extract::State(allowed): axum::extract::State<Arc<std::collections::HashSet<String>>>,
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let host = request
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if allowed.contains(&host) {
        return next.run(request).await;
    }
    warn!(
        host = %host,
        "[Gateway] Rejected request with unrecognized Host header (DNS-rebinding protection). \
         Add it under Settings → Gateway → Allowed hosts if it is legitimate."
    );
    (
        axum::http::StatusCode::MISDIRECTED_REQUEST,
        format!(
            "Unrecognized Host header {host:?}. If this name legitimately points at this \
             McpMux gateway, add it in Settings → Gateway → Network access → Allowed hosts."
        ),
    )
        .into_response()
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
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut state_guard = state.write().await;
                state_guard.set_database(dependencies.database.clone());
                state_guard
                    .set_client_metadata_service(dependencies.client_metadata_service.clone());
            });
        });

        // Initialize all services using DI container (pass domain event sender for non-blocking emission)
        let services = ServiceContainer::initialize(&dependencies, domain_event_tx, state.clone());

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

    /// Build the shared [`AppState`] used by the gateway's handlers, so callers
    /// (e.g. the serve binary mounting the management API) can construct extra
    /// routers against the same state the gateway uses.
    pub fn app_state(&self) -> AppState {
        AppState {
            gateway_state: self.state.clone(),
            services: Arc::new(self.services.clone()),
            base_url: self.config.base_url(),
        }
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
    /// Build the Axum router, merging `extra_router` (already fully stated —
    /// e.g. the management API) BEFORE the cross-cutting layers so it inherits
    /// CORS and, on a network bind, the Host allowlist. Pass `Router::new()`
    /// for the common no-extra-routes case.
    fn build_router_with(&self, extra_router: Router) -> Router {
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
        let mut mcp_routes =
            Router::new()
                .nest_service("/mcp", mcp_service)
                .layer(middleware::from_fn_with_state(
                    Arc::new(self.services.clone()),
                    mcp_oauth_middleware,
                ));

        // On a network bind, cap /mcp per (peer-IP, credential) and damp
        // credential stuffing. Layered OUTSIDE the OAuth middleware so it
        // observes the 401 a rejected request produces. Loopback binds stay
        // unlimited (local bulk workflows must not be throttled). The
        // Extension carries the limiter into the middleware.
        if self.config.is_network_bind() {
            let mcp_limiter =
                rate_limit::McpRateLimiter::new(rate_limit::McpRateLimitConfig::default());
            info!("[Gateway] Per-peer /mcp rate limiting active (network bind)");
            mcp_routes = mcp_routes
                .layer(middleware::from_fn(rate_limit::mcp_rate_limit_middleware))
                .layer(axum::Extension(mcp_limiter));
        }

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
            // Device pairing (public but token-gated): the claim page + the
            // token-exchange endpoint. Reachable on a network bind so a paired
            // device can claim its key; the single-use token is the gate.
            .route("/pair", get(handlers::pair_page))
            .route("/pair/claim", post(handlers::pair_claim))
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
            // Extra caller-supplied routes (management API) — merged here so
            // they sit under CORS + the Host allowlist below.
            .merge(extra_router)
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

        // Network binds enforce the Host allowlist across the WHOLE router
        // (rmcp's own check only guards /mcp). Outermost layer: rejected
        // requests never reach logging/rate-limit/handlers. Loopback binds
        // skip it (rmcp still enforces loopback hosts on /mcp), as does the
        // explicit allow-any-host escape hatch.
        if self.config.is_network_bind() && !self.config.allow_any_host {
            let allowed: Arc<std::collections::HashSet<String>> = Arc::new(
                self.config
                    .allowed_hosts()
                    .iter()
                    .map(|h| h.to_ascii_lowercase())
                    .collect(),
            );
            info!(
                "[Gateway] Host allowlist active on network bind ({} entries)",
                allowed.len()
            );
            router = router.layer(middleware::from_fn_with_state(
                allowed,
                enforce_allowed_hosts,
            ));
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
        self.run_with_shutdown_and_router(Router::new(), shutdown)
            .await
    }

    /// Like [`Self::run_with_shutdown`], but merges `extra_router` (already
    /// stated — e.g. a management API built via [`Self::app_state`]) into the
    /// served router, under the same CORS + Host-allowlist protections.
    pub async fn run_with_shutdown_and_router(
        self,
        extra_router: Router,
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
        let router = self_arc.build_router_with(extra_router);
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
    fn network_bind_builds_machine_allowlist_not_allow_all() {
        // Hardened behavior: a network bind enforces an allowlist built from
        // the machine's own identities instead of relaxing to allow-all.
        let hosts = config_on_host("0.0.0.0").allowed_hosts();
        assert!(!hosts.is_empty(), "network bind must NOT be allow-all");
        // Loopback names stay accepted (the machine itself).
        assert!(hosts.contains(&"localhost".to_string()), "{hosts:?}");
        // Wildcard bind addresses are not client-sendable Host values.
        assert!(!hosts.contains(&"0.0.0.0".to_string()), "{hosts:?}");
        // Loopback bind keeps the strict allow-list.
        assert!(config_on_host("127.0.0.1")
            .allowed_hosts()
            .contains(&"localhost".to_string()));
    }

    #[test]
    fn allow_any_host_escape_hatch_returns_empty_on_network_bind_only() {
        let cfg = GatewayConfig {
            host: "0.0.0.0".to_string(),
            allow_any_host: true,
            ..Default::default()
        };
        // Empty = rmcp allow-all; the router middleware is skipped too.
        assert!(cfg.allowed_hosts().is_empty());

        // On loopback the flag is irrelevant — the strict list stays.
        let cfg = GatewayConfig {
            allow_any_host: true,
            ..Default::default()
        };
        assert!(cfg.allowed_hosts().contains(&"localhost".to_string()));
    }

    #[test]
    fn additional_allowed_hosts_accepted_bare_and_with_port() {
        let cfg = GatewayConfig {
            host: "0.0.0.0".to_string(),
            port: 45818,
            additional_allowed_hosts: vec![
                "mybox.tail1234.ts.net".to_string(),
                "custom.example:9999".to_string(),
                "  ".to_string(), // blank entries are ignored
            ],
            ..Default::default()
        };
        let hosts = cfg.allowed_hosts();
        assert!(
            hosts.contains(&"mybox.tail1234.ts.net".to_string()),
            "{hosts:?}"
        );
        assert!(
            hosts.contains(&"mybox.tail1234.ts.net:45818".to_string()),
            "bare entries also match with the gateway port: {hosts:?}"
        );
        // Entries that already carry a port are taken verbatim.
        assert!(
            hosts.contains(&"custom.example:9999".to_string()),
            "{hosts:?}"
        );
        assert!(!hosts.iter().any(|h| h.trim().is_empty()));
    }

    #[test]
    fn network_bind_includes_public_base_url_host() {
        let cfg = GatewayConfig {
            host: "0.0.0.0".to_string(),
            public_base_url: Some("https://mcp.example.com".to_string()),
            ..Default::default()
        };
        let hosts = cfg.allowed_hosts();
        assert!(hosts.contains(&"mcp.example.com".to_string()), "{hosts:?}");
    }

    #[test]
    fn ipv6_hosts_get_bracketed_port_forms() {
        let mut hosts = Vec::new();
        super::push_host_forms(&mut hosts, "::1", 45818);
        assert!(hosts.contains(&"::1".to_string()));
        assert!(hosts.contains(&"[::1]".to_string()));
        assert!(hosts.contains(&"[::1]:45818".to_string()));

        let mut v4 = Vec::new();
        super::push_host_forms(&mut v4, "192.168.1.5", 45818);
        assert!(v4.contains(&"192.168.1.5".to_string()));
        assert!(v4.contains(&"192.168.1.5:45818".to_string()));
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
