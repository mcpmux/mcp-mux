//! HTTP transport for MCP servers
//!
//! Handles connecting to MCP servers over Streamable HTTP.
//! Uses RMCP's AuthClient with DatabaseCredentialStore for automatic OAuth token refresh.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mcpmux_core::{
    CredentialRepository, LogLevel, LogSource, OutboundOAuthRepository, ServerLog, ServerLogManager,
};
use rmcp::transport::auth::{AuthClient, AuthorizationManager};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use tracing::{debug, error, info};
use uuid::Uuid;

use super::TransportType;
use super::{create_client_handler, Transport, TransportConnectResult};
use crate::pool::credential_store::DatabaseCredentialStore;

/// HTTP transport for Streamable HTTP MCP servers
///
/// Uses RMCP's AuthClient with DatabaseCredentialStore for automatic token refresh.
/// The CredentialStore is backed by our database, so tokens are persisted and
/// automatically refreshed by RMCP on every request when needed.
pub struct HttpTransport {
    url: String,
    #[allow(dead_code)] // Reserved for future custom headers
    headers: HashMap<String, String>,
    space_id: Uuid,
    server_id: String,
    credential_repo: Arc<dyn CredentialRepository>,
    backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
    log_manager: Option<Arc<ServerLogManager>>,
    connect_timeout: Duration,
    event_tx: Option<tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>>,
}

impl HttpTransport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        url: String,
        headers: HashMap<String, String>,
        space_id: Uuid,
        server_id: String,
        credential_repo: Arc<dyn CredentialRepository>,
        backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
        log_manager: Option<Arc<ServerLogManager>>,
        connect_timeout: Duration,
        event_tx: Option<tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>>,
    ) -> Self {
        Self {
            url,
            headers,
            space_id,
            server_id,
            credential_repo,
            backend_oauth_repo,
            log_manager,
            connect_timeout,
            event_tx,
        }
    }

    /// Log a message
    async fn log(&self, level: LogLevel, source: LogSource, message: String) {
        if let Some(log_manager) = &self.log_manager {
            let log = ServerLog::new(level, source, message);
            if let Err(e) = log_manager
                .append(&self.space_id.to_string(), &self.server_id, log)
                .await
            {
                error!("Failed to write log: {}", e);
            }
        }
    }

    /// Check if an error indicates OAuth is required
    fn requires_oauth(error_str: &str) -> bool {
        let error_lower = error_str.to_lowercase();
        let oauth_indicators = [
            "401",
            "unauthorized",
            "authrequired",
            "auth required",
            "invalid_token",
            "oauth",
            "channel closed",
            "transport channel closed",
            "www-authenticate",
            "access token",
            "missing or invalid",
            "bearer",
        ];
        oauth_indicators.iter().any(|s| error_lower.contains(s))
    }

    /// Connect with OAuth using DatabaseCredentialStore.
    ///
    /// RMCP's AuthClient will automatically:
    /// - Load tokens from the credential store
    /// - Check expiration and refresh if needed
    /// - Save refreshed tokens back to the store
    /// - Add auth header to every request
    ///
    /// If RMCP's metadata discovery fails (non-spec-compliant servers), we use
    /// stored metadata from the initial OAuth flow.
    async fn connect_with_auth(&self) -> TransportConnectResult {
        debug!(
            server_id = %self.server_id,
            "Connecting with OAuth via CredentialStore"
        );

        self.log(
            LogLevel::Info,
            LogSource::HttpRequest,
            format!(
                "Connecting to {} with OAuth (auto-refresh enabled)",
                self.url
            ),
        )
        .await;

        // Create DatabaseCredentialStore backed by our database
        let credential_store = DatabaseCredentialStore::new(
            self.space_id,
            &self.server_id,
            &self.url,
            Arc::clone(&self.credential_repo),
            Arc::clone(&self.backend_oauth_repo),
        );

        // Create authorization manager and set our credential store
        let mut auth_manager = match AuthorizationManager::new(&self.url).await {
            Ok(m) => m,
            Err(e) => {
                let err = format!("Failed to create auth manager: {}", e);
                error!(server_id = %self.server_id, "{}", err);
                self.log(LogLevel::Error, LogSource::HttpRequest, err.clone())
                    .await;
                return TransportConnectResult::Failed(err);
            }
        };

        // Set our database-backed credential store
        auth_manager.set_credential_store(credential_store);

        // Load stored metadata from initial OAuth flow
        // This bypasses RMCP's metadata discovery which can fail on non-spec-compliant servers
        let has_stored_metadata = if let Ok(Some(registration)) = self
            .backend_oauth_repo
            .get(&self.space_id, &self.server_id)
            .await
        {
            if let Some(stored_metadata) = registration.metadata {
                debug!(
                    server_id = %self.server_id,
                    space_id = %self.space_id,
                    "Using stored OAuth metadata (bypassing RMCP discovery)"
                );
                let rmcp_metadata =
                    crate::pool::oauth_utils::convert_from_stored_metadata(&stored_metadata);
                auth_manager.set_metadata(rmcp_metadata);
                true
            } else {
                // No metadata stored - will need re-auth if refresh is needed
                debug!(
                    server_id = %self.server_id,
                    space_id = %self.space_id,
                    "No stored metadata - token refresh may fail on non-spec servers"
                );
                false
            }
        } else {
            false
        };

        // Initialize from stored credentials
        let init_result = auth_manager.initialize_from_store().await;

        match init_result {
            Ok(true) => {
                debug!(
                    server_id = %self.server_id,
                    space_id = %self.space_id,
                    "Initialized from stored credentials (has_metadata={})", has_stored_metadata
                );
            }
            Ok(false) => {
                debug!(
                    server_id = %self.server_id,
                    "No stored credentials found"
                );
                // No stored credentials - OAuth required
                self.log(
                    LogLevel::Info,
                    LogSource::OAuth,
                    "No stored credentials, OAuth required".to_string(),
                )
                .await;
                return TransportConnectResult::OAuthRequired {
                    server_url: self.url.clone(),
                };
            }
            Err(e) => {
                // RMCP metadata discovery failed AND we don't have stored metadata
                // Fall back to manual token injection as last resort
                debug!(
                    server_id = %self.server_id,
                    "RMCP initialize_from_store failed: {} (stored_metadata={})", e, has_stored_metadata
                );

                if has_stored_metadata {
                    // We had metadata but RMCP still failed - this shouldn't happen
                    let err = format!("OAuth initialization failed despite stored metadata: {}", e);
                    error!(server_id = %self.server_id, "{}", err);
                    self.log(LogLevel::Error, LogSource::OAuth, err.clone())
                        .await;
                    return TransportConnectResult::Failed(err);
                }

                // No stored metadata - try manual token injection
                self.log(
                    LogLevel::Warn,
                    LogSource::OAuth,
                    format!(
                        "OAuth metadata discovery failed: {}, trying manual token injection",
                        e
                    ),
                )
                .await;

                return self.connect_with_manual_token().await;
            }
        }

        // Create AuthClient - this wraps reqwest::Client with automatic token injection & refresh
        let auth_client = AuthClient::new(reqwest::Client::default(), auth_manager);
        let transport_config = StreamableHttpClientTransportConfig::with_uri(self.url.as_str());
        let transport = StreamableHttpClientTransport::with_client(auth_client, transport_config);

        let client_handler =
            create_client_handler(&self.server_id, self.space_id, self.event_tx.clone());

        let connect_future = client_handler.serve(transport);
        match tokio::time::timeout(self.connect_timeout, connect_future).await {
            Ok(Ok(client)) => {
                info!(
                    server_id = %self.server_id,
                    "HTTP server connected with OAuth (auto-refresh enabled)"
                );
                self.log(
                    LogLevel::Info,
                    LogSource::HttpResponse,
                    "Connected successfully with OAuth".to_string(),
                )
                .await;
                TransportConnectResult::Connected(client)
            }
            Ok(Err(e)) => {
                let err_str = format!("{:#}", e);
                if Self::requires_oauth(&err_str) {
                    info!(
                        server_id = %self.server_id,
                        "OAuth authentication required or token invalid"
                    );
                    self.log(
                        LogLevel::Warn,
                        LogSource::OAuth,
                        "Token invalid/expired, re-authentication required".to_string(),
                    )
                    .await;
                    TransportConnectResult::OAuthRequired {
                        server_url: self.url.clone(),
                    }
                } else {
                    let err = format!("HTTP auth connection failed: {}", e);
                    error!(server_id = %self.server_id, "{}", err);
                    self.log(LogLevel::Error, LogSource::HttpResponse, err.clone())
                        .await;
                    TransportConnectResult::Failed(err)
                }
            }
            Err(_) => {
                let err = format!("Connection timeout ({:?})", self.connect_timeout);
                error!(server_id = %self.server_id, "{}", err);
                self.log(LogLevel::Error, LogSource::HttpRequest, err.clone())
                    .await;
                TransportConnectResult::Failed(err)
            }
        }
    }

    /// Connect with manual token injection when RMCP's metadata discovery fails.
    ///
    /// Some servers (like Cloudflare) don't serve OAuth metadata at the standard location
    /// that RMCP expects. In this case, we manually inject the stored token into requests.
    /// NOTE: Auto-refresh won't work in this mode - tokens must be refreshed manually.
    async fn connect_with_manual_token(&self) -> TransportConnectResult {
        debug!(
            server_id = %self.server_id,
            "Connecting with manual token injection (RMCP metadata failed)"
        );

        // Load access token from our database
        let access_token = match self
            .credential_repo
            .get(
                &self.space_id,
                &self.server_id,
                &mcpmux_core::CredentialType::AccessToken,
            )
            .await
        {
            Ok(Some(cred)) => cred.value,
            Ok(None) => {
                debug!(server_id = %self.server_id, "No stored token for manual injection");
                return TransportConnectResult::OAuthRequired {
                    server_url: self.url.clone(),
                };
            }
            Err(e) => {
                let err = format!("Failed to load credential: {}", e);
                error!(server_id = %self.server_id, "{}", err);
                return TransportConnectResult::Failed(err);
            }
        };

        self.log(
            LogLevel::Info,
            LogSource::HttpRequest,
            format!("Connecting to {} with manual token injection", self.url),
        )
        .await;

        // Build HTTP client with Authorization header
        let mut headers = reqwest::header::HeaderMap::new();
        let auth_value = format!("Bearer {}", access_token);
        match reqwest::header::HeaderValue::from_str(&auth_value) {
            Ok(val) => {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
            Err(e) => {
                let err = format!("Invalid token format: {}", e);
                error!(server_id = %self.server_id, "{}", err);
                return TransportConnectResult::Failed(err);
            }
        }

        let client = match reqwest::Client::builder().default_headers(headers).build() {
            Ok(c) => c,
            Err(e) => {
                let err = format!("Failed to build HTTP client: {}", e);
                error!(server_id = %self.server_id, "{}", err);
                return TransportConnectResult::Failed(err);
            }
        };

        let transport_config = StreamableHttpClientTransportConfig::with_uri(self.url.as_str());
        let transport = StreamableHttpClientTransport::with_client(client, transport_config);

        let client_handler =
            create_client_handler(&self.server_id, self.space_id, self.event_tx.clone());

        let connect_future = client_handler.serve(transport);
        match tokio::time::timeout(self.connect_timeout, connect_future).await {
            Ok(Ok(client)) => {
                info!(
                    server_id = %self.server_id,
                    "HTTP server connected with manual token (no auto-refresh)"
                );
                self.log(
                    LogLevel::Info,
                    LogSource::HttpResponse,
                    "Connected successfully with manual token".to_string(),
                )
                .await;
                TransportConnectResult::Connected(client)
            }
            Ok(Err(e)) => {
                let err_str = format!("{:#}", e);
                if Self::requires_oauth(&err_str) {
                    info!(
                        server_id = %self.server_id,
                        "Token invalid/expired, re-authentication required"
                    );
                    self.log(
                        LogLevel::Warn,
                        LogSource::OAuth,
                        "Token invalid/expired, re-authentication required".to_string(),
                    )
                    .await;
                    TransportConnectResult::OAuthRequired {
                        server_url: self.url.clone(),
                    }
                } else {
                    let err = format!("HTTP connection with manual token failed: {}", e);
                    error!(server_id = %self.server_id, "{}", err);
                    self.log(LogLevel::Error, LogSource::HttpResponse, err.clone())
                        .await;
                    TransportConnectResult::Failed(err)
                }
            }
            Err(_) => {
                let err = format!("Connection timeout ({:?})", self.connect_timeout);
                error!(server_id = %self.server_id, "{}", err);
                self.log(LogLevel::Error, LogSource::HttpRequest, err.clone())
                    .await;
                TransportConnectResult::Failed(err)
            }
        }
    }

    /// Try connecting without authentication
    async fn connect_without_auth(&self) -> TransportConnectResult {
        debug!(
            server_id = %self.server_id,
            "Trying connection without auth"
        );

        self.log(
            LogLevel::Info,
            LogSource::HttpRequest,
            format!("Connecting to {} without auth", self.url),
        )
        .await;

        let transport = StreamableHttpClientTransport::from_uri(self.url.as_str());
        let client_handler =
            create_client_handler(&self.server_id, self.space_id, self.event_tx.clone());

        let connect_future = client_handler.serve(transport);
        match tokio::time::timeout(self.connect_timeout, connect_future).await {
            Ok(Ok(client)) => {
                info!(
                    server_id = %self.server_id,
                    "HTTP server connected without auth"
                );
                self.log(
                    LogLevel::Info,
                    LogSource::HttpResponse,
                    "Connected successfully without auth".to_string(),
                )
                .await;
                TransportConnectResult::Connected(client)
            }
            Ok(Err(e)) => {
                let err_str = format!("{:#}", e);
                if Self::requires_oauth(&err_str) {
                    info!(
                        server_id = %self.server_id,
                        "Server requires OAuth authentication"
                    );
                    self.log(
                        LogLevel::Info,
                        LogSource::OAuth,
                        "Server requires OAuth authentication".to_string(),
                    )
                    .await;
                    TransportConnectResult::OAuthRequired {
                        server_url: self.url.clone(),
                    }
                } else {
                    let err = format!("HTTP connection failed: {}", e);
                    error!(server_id = %self.server_id, "{}", err);
                    self.log(LogLevel::Error, LogSource::HttpResponse, err.clone())
                        .await;
                    TransportConnectResult::Failed(err)
                }
            }
            Err(_) => {
                let err = format!("Connection timeout ({:?})", self.connect_timeout);
                error!(server_id = %self.server_id, "{}", err);
                self.log(LogLevel::Error, LogSource::HttpRequest, err.clone())
                    .await;
                TransportConnectResult::Failed(err)
            }
        }
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn connect(&self) -> TransportConnectResult {
        info!(
            server_id = %self.server_id,
            url = %self.url,
            "Connecting to HTTP server"
        );

        self.log(
            LogLevel::Info,
            LogSource::Connection,
            format!("Connecting to HTTP server: {}", self.url),
        )
        .await;

        // Validate URL
        if let Err(e) = url::Url::parse(&self.url) {
            let err = format!("Invalid URL: {}", e);
            self.log(LogLevel::Error, LogSource::Connection, err.clone())
                .await;
            return TransportConnectResult::Failed(err);
        }

        // Check if we have stored credentials for this server
        let has_credentials = self
            .credential_repo
            .get(
                &self.space_id,
                &self.server_id,
                &mcpmux_core::CredentialType::AccessToken,
            )
            .await
            .ok()
            .flatten()
            .is_some();

        if has_credentials {
            info!(
                server_id = %self.server_id,
                "Found stored credentials, connecting with OAuth (auto-refresh enabled)"
            );
            self.connect_with_auth().await
        } else {
            debug!(
                server_id = %self.server_id,
                "No stored credentials, trying without auth"
            );
            self.connect_without_auth().await
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }

    fn description(&self) -> String {
        format!("http:{}", self.url)
    }
}
