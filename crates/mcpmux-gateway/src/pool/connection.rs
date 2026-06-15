//! Connection Service - Handles server connection lifecycle
//!
//! ConnectionService is responsible for:
//! - Connecting to MCP servers using the appropriate transport
//! - Disconnecting from servers (clearing tokens on logout)
//! - Managing OAuth flow initiation
//!
//! Uses TokenService for token management and TransportFactory for transport creation.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use mcpmux_core::{CredentialRepository, OutboundOAuthRepository, ServerLogManager};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::features::{CachedFeatures, FeatureService};
use super::instance::{DiscoveredFeatures, McpClientConnection, ServerInstance};
use super::oauth::{OAuthInitResult, OutboundOAuthManager};
use super::token::TokenService;
use super::transport::{
    ResolvedTransport, TransportConnectResult, TransportFactory, TransportType,
};

/// Default connection timeout
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

/// Result of a connection attempt
#[derive(Debug)]
pub enum ConnectionResult {
    /// Successfully connected
    Connected {
        /// Whether this was a reused instance
        reused: bool,
        /// Discovered features (typed as ServerFeature for DB caching)
        features: CachedFeatures,
    },
    /// OAuth required - need user interaction
    OAuthRequired {
        /// Authorization URL to open in browser
        auth_url: String,
    },
    /// Connection failed
    Failed {
        /// Error message
        error: String,
    },
}

/// Connection Service handles server connection lifecycle
pub struct ConnectionService {
    token_service: Arc<TokenService>,
    oauth_manager: Arc<OutboundOAuthManager>,
    credential_repo: Arc<dyn CredentialRepository>,
    backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
    prefix_cache: Arc<crate::services::PrefixCacheService>,
    log_manager: Option<Arc<ServerLogManager>>,
    connect_timeout: Duration,
    event_tx: Option<tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>>,
}

impl ConnectionService {
    pub fn new(
        token_service: Arc<TokenService>,
        oauth_manager: Arc<OutboundOAuthManager>,
        credential_repo: Arc<dyn CredentialRepository>,
        backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
        prefix_cache: Arc<crate::services::PrefixCacheService>,
    ) -> Self {
        Self {
            token_service,
            oauth_manager,
            credential_repo,
            backend_oauth_repo,
            prefix_cache,
            log_manager: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            event_tx: None,
        }
    }

    pub fn with_log_manager(mut self, log_manager: Arc<ServerLogManager>) -> Self {
        self.log_manager = Some(log_manager);
        self
    }

    pub fn with_event_tx(
        mut self,
        event_tx: tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>,
    ) -> Self {
        self.event_tx = Some(event_tx);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Get the OAuth manager for checking pending flows
    pub fn oauth_manager(&self) -> Arc<OutboundOAuthManager> {
        self.oauth_manager.clone()
    }

    /// Get the token service
    pub fn token_service(&self) -> Arc<TokenService> {
        self.token_service.clone()
    }

    /// Get the log manager
    pub fn log_manager(&self) -> Option<Arc<ServerLogManager>> {
        self.log_manager.clone()
    }

    /// Helper method to log connection events to server-specific log files
    async fn log_connection_event(
        &self,
        space_id: &Uuid,
        server_id: &str,
        level: mcpmux_core::LogLevel,
        message: impl Into<String>,
        metadata: Option<serde_json::Value>,
    ) {
        if let Some(log_manager) = &self.log_manager {
            let mut log = mcpmux_core::ServerLog::new(
                level,
                mcpmux_core::LogSource::Connection,
                message.into(),
            );
            if let Some(meta) = metadata {
                log = log.with_metadata(meta);
            }
            let _ = log_manager
                .append(&space_id.to_string(), server_id, log)
                .await;
        }
    }

    /// Connect to a server
    ///
    /// Creates the appropriate transport and attempts connection.
    /// For OAuth-protected servers, initiates OAuth flow if needed (unless auto_reconnect=true).
    ///
    /// # Parameters
    /// - `ctx`: Connection context with space_id, server_id, transport config, and auto_reconnect flag
    pub async fn connect(
        &self,
        ctx: &super::ConnectionContext,
        feature_service: &FeatureService,
    ) -> ConnectionResult {
        let space_id = ctx.space_id;
        let server_id = &ctx.server_id;
        let config = &ctx.transport;
        let auto_reconnect = ctx.auto_reconnect;

        // Determine the actual config to use (checking for DCR override)
        let mut final_config = config.clone();

        // If HTTP, check if we have a DCR registration with a different URL
        if let Some(config_url) = config.url() {
            if let Ok(Some(registration)) = self.backend_oauth_repo.get(&space_id, server_id).await
            {
                if registration.server_url != config_url {
                    info!(
                        "[ConnectionService] Overriding config URL with DCR URL: {}",
                        registration.server_url
                    );
                    if let ResolvedTransport::Http { url, .. } = &mut final_config {
                        *url = registration.server_url;
                    }
                }
            }
        }

        info!(
            "[ConnectionService] Connecting {}/{} via {:?}",
            space_id,
            server_id,
            final_config.transport_type()
        );

        // Log connection attempt to server log
        let transport_name = match &final_config {
            ResolvedTransport::Stdio { .. } => "STDIO",
            ResolvedTransport::Http { .. } => "HTTP",
        };
        self.log_connection_event(
            &space_id,
            server_id,
            mcpmux_core::LogLevel::Info,
            format!("Attempting connection via {}", transport_name),
            Some(serde_json::json!({ "transport_type": transport_name })),
        )
        .await;

        // Create transport
        let transport = TransportFactory::create(
            &final_config,
            space_id,
            server_id.to_string(),
            Arc::clone(&self.credential_repo),
            Arc::clone(&self.backend_oauth_repo),
            self.log_manager.clone(),
            self.connect_timeout,
            self.event_tx.clone(),
        );

        // Attempt connection
        match transport.connect().await {
            TransportConnectResult::Connected(client) => {
                // Discover and cache features
                let features = match feature_service
                    .discover_and_cache(&space_id.to_string(), server_id, &client)
                    .await
                {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("[ConnectionService] Feature discovery failed: {}", e);

                        self.log_connection_event(
                            &space_id,
                            server_id,
                            mcpmux_core::LogLevel::Warn,
                            format!("Feature discovery failed: {}", e),
                            None,
                        )
                        .await;

                        CachedFeatures::default()
                    }
                };

                info!(
                    "[ConnectionService] Connected {}/{} - {} features",
                    space_id,
                    server_id,
                    features.total_count()
                );

                // Log successful connection to server log
                self.log_connection_event(
                    &space_id,
                    server_id,
                    mcpmux_core::LogLevel::Info,
                    format!(
                        "Connection established successfully - discovered {} features",
                        features.total_count()
                    ),
                    Some(serde_json::json!({
                        "tools": features.tools.len(),
                        "prompts": features.prompts.len(),
                        "resources": features.resources.len()
                    })),
                )
                .await;

                ConnectionResult::Connected {
                    reused: false,
                    features,
                }
            }
            TransportConnectResult::OAuthRequired { server_url } => {
                // Log OAuth requirement to server log
                self.log_connection_event(
                    &space_id,
                    server_id,
                    mcpmux_core::LogLevel::Info,
                    "OAuth authentication required - waiting for user authorization",
                    Some(serde_json::json!({ "server_url": server_url })),
                )
                .await;

                self.handle_oauth_required(space_id, server_id, &server_url, auto_reconnect)
                    .await
            }
            TransportConnectResult::Failed(error) => {
                // Log connection failure to server log
                self.log_connection_event(
                    &space_id,
                    server_id,
                    mcpmux_core::LogLevel::Error,
                    format!("Connection failed: {}", error),
                    Some(serde_json::json!({ "error": &error })),
                )
                .await;

                ConnectionResult::Failed { error }
            }
        }
    }

    /// Connect to a server with an existing instance (used for reconnection)
    pub async fn connect_with_instance(
        &self,
        ctx: &super::ConnectionContext,
        instance: &Arc<ServerInstance>,
        feature_service: &FeatureService,
    ) -> ConnectionResult {
        let space_id = ctx.space_id;
        let server_id = &ctx.server_id;
        let config = &ctx.transport;
        let auto_reconnect = ctx.auto_reconnect;

        // Assign prefix for this server (fetches alias from registry internally)
        let space_id_str = space_id.to_string();
        let _ = self
            .prefix_cache
            .assign_prefix_for_server(&space_id_str, server_id)
            .await;

        // If already healthy, just return
        if instance.is_healthy() && instance.get_features().is_some() {
            // Log reuse to server log
            self.log_connection_event(
                &space_id,
                server_id,
                mcpmux_core::LogLevel::Debug,
                "Reusing existing healthy connection",
                None,
            )
            .await;

            return ConnectionResult::Connected {
                reused: true,
                // Empty features - they're already cached in DB
                features: CachedFeatures::default(),
            };
        }

        // Log reconnection attempt to server log
        self.log_connection_event(
            &space_id,
            server_id,
            mcpmux_core::LogLevel::Info,
            "Reconnecting to server...",
            None,
        )
        .await;

        instance.mark_connecting();

        // Create transport
        let transport = TransportFactory::create(
            config,
            space_id,
            server_id.to_string(),
            Arc::clone(&self.credential_repo),
            Arc::clone(&self.backend_oauth_repo),
            self.log_manager.clone(),
            self.connect_timeout,
            self.event_tx.clone(),
        );

        // Attempt connection
        match transport.connect().await {
            TransportConnectResult::Connected(client) => {
                // Discover and cache features
                let features = match feature_service
                    .discover_and_cache(&space_id.to_string(), server_id, &client)
                    .await
                {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("[ConnectionService] Feature discovery failed: {}", e);
                        CachedFeatures::default()
                    }
                };

                // Convert CachedFeatures to DiscoveredFeatures for instance state
                let discovered_features = DiscoveredFeatures {
                    tools: features
                        .tools
                        .iter()
                        .map(|t| serde_json::to_value(t).unwrap_or_default())
                        .collect(),
                    prompts: features
                        .prompts
                        .iter()
                        .map(|p| serde_json::to_value(p).unwrap_or_default())
                        .collect(),
                    resources: features
                        .resources
                        .iter()
                        .map(|r| serde_json::to_value(r).unwrap_or_default())
                        .collect(),
                };

                let connection = match config.transport_type() {
                    TransportType::Stdio => McpClientConnection::Stdio { client },
                    TransportType::Http => McpClientConnection::Http { client },
                };

                instance.mark_connected(discovered_features, connection);

                info!(
                    "[ConnectionService] Connected {}/{} - {} features",
                    space_id,
                    server_id,
                    features.total_count()
                );

                ConnectionResult::Connected {
                    reused: false,
                    features,
                }
            }
            TransportConnectResult::OAuthRequired { server_url } => {
                instance.mark_oauth_pending();
                self.handle_oauth_required(space_id, server_id, &server_url, auto_reconnect)
                    .await
            }
            TransportConnectResult::Failed(error) => {
                instance.mark_failed(error.clone());
                ConnectionResult::Failed { error }
            }
        }
    }

    /// Disconnect from a server (logout)
    ///
    /// Clears OAuth tokens but preserves client_id for DCR reuse.
    pub async fn disconnect(
        &self,
        space_id: Uuid,
        server_id: &str,
        feature_service: &FeatureService,
    ) -> Result<()> {
        info!(
            "[ConnectionService] Disconnecting {}/{}",
            space_id, server_id
        );

        // Log disconnection to server log
        self.log_connection_event(
            &space_id,
            server_id,
            mcpmux_core::LogLevel::Info,
            "Server disconnected",
            None,
        )
        .await;

        // Clear tokens (keeps client_id for re-auth)
        self.token_service.clear_tokens(space_id, server_id).await?;

        // Mark features as unavailable
        feature_service
            .mark_unavailable(&space_id.to_string(), server_id)
            .await?;

        Ok(())
    }

    /// Reconnect after OAuth completes
    ///
    /// Uses the stored server URL and tokens from OAuth registration
    /// to reconnect without needing the original transport config.
    pub async fn reconnect_after_oauth(
        &self,
        space_id: Uuid,
        server_id: &str,
        instance: &Arc<ServerInstance>,
        feature_service: &FeatureService,
    ) -> ConnectionResult {
        info!(
            "[ConnectionService] Reconnecting {}/{} after OAuth",
            space_id, server_id
        );

        // Get server URL from OAuth registration
        let server_url = match self.backend_oauth_repo.get(&space_id, server_id).await {
            Ok(Some(registration)) => registration.server_url,
            Ok(None) => {
                return ConnectionResult::Failed {
                    error: "No OAuth registration found - cannot determine server URL".to_string(),
                };
            }
            Err(e) => {
                return ConnectionResult::Failed {
                    error: format!("Failed to get OAuth registration: {}", e),
                };
            }
        };

        info!(
            "[ConnectionService] Reconnecting to {} with OAuth token",
            server_url
        );

        instance.mark_connecting();

        // Create transport config with the stored URL, preserving transport type
        let config = match instance.transport_type {
            TransportType::Http => ResolvedTransport::Http {
                url: server_url.clone(),
                headers: std::collections::HashMap::new(),
            },
            TransportType::Stdio => {
                // Should not happen for OAuth, but fallback to Http if somehow we got here
                warn!("[ConnectionService] Unexpected STDIO transport for OAuth reconnection, defaulting to HTTP");
                ResolvedTransport::Http {
                    url: server_url.clone(),
                    headers: std::collections::HashMap::new(),
                }
            }
        };

        // Create transport with credential repositories (will inject OAuth token via CredentialStore)
        let transport = TransportFactory::create(
            &config,
            space_id,
            server_id.to_string(),
            Arc::clone(&self.credential_repo),
            Arc::clone(&self.backend_oauth_repo),
            self.log_manager.clone(),
            self.connect_timeout,
            self.event_tx.clone(),
        );

        // Attempt connection
        match transport.connect().await {
            TransportConnectResult::Connected(client) => {
                // Discover and cache features
                let features = match feature_service
                    .discover_and_cache(&space_id.to_string(), server_id, &client)
                    .await
                {
                    Ok(f) => f,
                    Err(e) => {
                        warn!(
                            "[ConnectionService] Feature discovery failed after OAuth: {}",
                            e
                        );
                        CachedFeatures::default()
                    }
                };

                // Convert CachedFeatures to DiscoveredFeatures for instance state
                let discovered_features = DiscoveredFeatures {
                    tools: features
                        .tools
                        .iter()
                        .map(|t| serde_json::to_value(t).unwrap_or_default())
                        .collect(),
                    prompts: features
                        .prompts
                        .iter()
                        .map(|p| serde_json::to_value(p).unwrap_or_default())
                        .collect(),
                    resources: features
                        .resources
                        .iter()
                        .map(|r| serde_json::to_value(r).unwrap_or_default())
                        .collect(),
                };

                let connection = match config.transport_type() {
                    TransportType::Stdio => McpClientConnection::Stdio { client },
                    TransportType::Http => McpClientConnection::Http { client },
                };

                instance.mark_connected(discovered_features, connection);

                info!(
                    "[ConnectionService] Connected {}/{} after OAuth - {} features",
                    space_id,
                    server_id,
                    features.total_count()
                );

                ConnectionResult::Connected {
                    reused: false,
                    features,
                }
            }
            TransportConnectResult::OAuthRequired { .. } => {
                // This shouldn't happen after successful OAuth
                instance.mark_oauth_pending();
                warn!(
                    "[ConnectionService] Still requires OAuth after token exchange - token may be invalid"
                );
                ConnectionResult::OAuthRequired {
                    auth_url: String::new(),
                }
            }
            TransportConnectResult::Failed(error) => {
                instance.mark_failed(error.clone());
                ConnectionResult::Failed { error }
            }
        }
    }

    /// Handle OAuth required - initiate OAuth flow (only for manual connects, not auto-reconnect)
    async fn handle_oauth_required(
        &self,
        space_id: Uuid,
        server_id: &str,
        server_url: &str,
        auto_reconnect: bool,
    ) -> ConnectionResult {
        if auto_reconnect {
            // Auto-reconnect: just return OAuthRequired without starting flow or opening browser
            debug!(
                "[ConnectionService] OAuth required for {}/{} (auto-reconnect, not starting flow)",
                space_id, server_id
            );

            // Return OAuthRequired with empty auth_url - this won't be used
            return ConnectionResult::OAuthRequired {
                auth_url: String::new(),
            };
        }

        // Manual connect: start OAuth flow
        info!(
            "[ConnectionService] OAuth required for {}/{}, initiating flow",
            space_id, server_id
        );

        match self
            .oauth_manager
            .start_oauth_flow(
                self.credential_repo.clone(),
                self.backend_oauth_repo.clone(),
                space_id,
                server_id,
                server_url,
            )
            .await
        {
            Ok(OAuthInitResult::Initiated { auth_url }) => {
                // Note: OAuth manager also logs this, but we log from connection service for consistency
                self.log_connection_event(
                    &space_id,
                    server_id,
                    mcpmux_core::LogLevel::Info,
                    "OAuth flow initiated - opening browser with authorization URL",
                    Some(serde_json::json!({
                        "auth_url": &auth_url,
                    })),
                )
                .await;

                ConnectionResult::OAuthRequired { auth_url }
            }
            Ok(OAuthInitResult::AlreadyAuthorized) => {
                // This shouldn't happen if we got here, but handle it
                debug!("[ConnectionService] AlreadyAuthorized but got OAuthRequired - retrying");
                ConnectionResult::Failed {
                    error: "OAuth state mismatch - please retry".to_string(),
                }
            }
            Ok(OAuthInitResult::NotSupported(reason)) => ConnectionResult::Failed {
                error: format!("OAuth not supported: {}", reason),
            },
            Err(e) => ConnectionResult::Failed {
                error: format!("OAuth flow failed: {}", e),
            },
        }
    }
}
