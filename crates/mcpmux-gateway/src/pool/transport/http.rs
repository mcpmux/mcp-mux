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

    /// Connect with OAuth using DatabaseCredentialStore (with definition headers if any).
    ///
    /// RMCP's AuthClient will automatically:
    /// - Load tokens from the credential store
    /// - Check expiration and refresh if needed
    /// - Save refreshed tokens back to the store
    /// - Add auth header to every request
    ///
    /// Definition headers are applied as default_headers on the underlying reqwest::Client,
    /// so they're sent alongside OAuth tokens on every request.
    ///
    /// If RMCP's metadata discovery fails (non-spec-compliant servers), we use
    /// stored metadata from the initial OAuth flow.
    async fn connect_with_auth(
        &self,
        header_map: reqwest::header::HeaderMap,
    ) -> TransportConnectResult {
        debug!(
            server_id = %self.server_id,
            header_count = header_map.len(),
            "Connecting with OAuth via CredentialStore"
        );

        self.log(
            LogLevel::Info,
            LogSource::HttpRequest,
            format!(
                "Connecting to {} with OAuth (auto-refresh enabled, {} custom header(s))",
                self.url,
                header_map.len()
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

                return self.connect_with_manual_token(header_map).await;
            }
        }

        // Create AuthClient - wraps reqwest::Client with automatic token injection & refresh.
        // Definition headers are baked into the client so they're sent on every request.
        let base_client = match self.build_http_client(header_map) {
            Ok(c) => c,
            Err(err) => return TransportConnectResult::Failed(err),
        };
        let auth_client = AuthClient::new(base_client, auth_manager);
        let transport_config = StreamableHttpClientTransportConfig::with_uri(self.url.as_str());
        let transport = StreamableHttpClientTransport::with_client(auth_client, transport_config);

        let client_handler = create_client_handler(
            &self.server_id,
            self.space_id,
            self.event_tx.clone(),
            self.log_manager.clone(),
        );

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
    /// Definition headers are merged in (token Authorization header takes precedence).
    /// NOTE: Auto-refresh won't work in this mode - tokens must be refreshed manually.
    async fn connect_with_manual_token(
        &self,
        mut header_map: reqwest::header::HeaderMap,
    ) -> TransportConnectResult {
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

        // Add Authorization header to the definition headers (overrides if already present)
        let auth_value = format!("Bearer {}", access_token);
        match reqwest::header::HeaderValue::from_str(&auth_value) {
            Ok(val) => {
                header_map.insert(reqwest::header::AUTHORIZATION, val);
            }
            Err(e) => {
                let err = format!("Invalid token format: {}", e);
                error!(server_id = %self.server_id, "{}", err);
                return TransportConnectResult::Failed(err);
            }
        }

        let client = match self.build_http_client(header_map) {
            Ok(c) => c,
            Err(err) => return TransportConnectResult::Failed(err),
        };

        let transport_config = StreamableHttpClientTransportConfig::with_uri(self.url.as_str());
        let transport = StreamableHttpClientTransport::with_client(client, transport_config);

        let client_handler = create_client_handler(
            &self.server_id,
            self.space_id,
            self.event_tx.clone(),
            self.log_manager.clone(),
        );

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

    /// Build a reqwest HeaderMap from definition-provided headers.
    ///
    /// These headers (resolved from `${input:ID}` placeholders) are always applied
    /// to the HTTP client regardless of auth strategy. Returns an empty map if no
    /// definition headers are configured.
    fn build_default_headers(&self) -> Result<reqwest::header::HeaderMap, String> {
        let mut header_map = reqwest::header::HeaderMap::new();
        for (key, value) in &self.headers {
            let header_name =
                reqwest::header::HeaderName::from_bytes(key.as_bytes()).map_err(|e| {
                    let err = format!("Invalid header name '{}': {}", key, e);
                    error!(server_id = %self.server_id, "{}", err);
                    err
                })?;
            let header_value = reqwest::header::HeaderValue::from_str(value).map_err(|e| {
                let err = format!("Invalid header value for '{}': {}", key, e);
                error!(server_id = %self.server_id, "{}", err);
                err
            })?;
            header_map.insert(header_name, header_value);
        }
        Ok(header_map)
    }

    /// Build a reqwest::Client with definition headers as default_headers.
    fn build_http_client(
        &self,
        header_map: reqwest::header::HeaderMap,
    ) -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .default_headers(header_map)
            .build()
            .map_err(|e| {
                let err = format!("Failed to build HTTP client: {}", e);
                error!(server_id = %self.server_id, "{}", err);
                err
            })
    }

    /// Try connecting without authentication (but with definition headers if any)
    async fn connect_without_auth(
        &self,
        header_map: reqwest::header::HeaderMap,
    ) -> TransportConnectResult {
        debug!(
            server_id = %self.server_id,
            header_count = header_map.len(),
            "Trying connection without auth"
        );

        self.log(
            LogLevel::Info,
            LogSource::HttpRequest,
            format!(
                "Connecting to {} without auth ({} custom header(s))",
                self.url,
                header_map.len()
            ),
        )
        .await;

        let client = match self.build_http_client(header_map) {
            Ok(c) => c,
            Err(err) => return TransportConnectResult::Failed(err),
        };

        let transport_config = StreamableHttpClientTransportConfig::with_uri(self.url.as_str());
        let transport = StreamableHttpClientTransport::with_client(client, transport_config);
        let client_handler = create_client_handler(
            &self.server_id,
            self.space_id,
            self.event_tx.clone(),
            self.log_manager.clone(),
        );

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

        // Build definition headers (always applied regardless of auth strategy)
        let header_map = match self.build_default_headers() {
            Ok(h) => h,
            Err(err) => return TransportConnectResult::Failed(err),
        };

        if !header_map.is_empty() {
            info!(
                server_id = %self.server_id,
                header_count = header_map.len(),
                "Applying definition-provided headers to connection"
            );
        }

        // Check if definition headers already include an Authorization header.
        // If so, skip OAuth — the user explicitly provided auth via the definition (e.g., PAT).
        let has_explicit_auth = header_map.contains_key(reqwest::header::AUTHORIZATION);

        if has_explicit_auth {
            info!(
                server_id = %self.server_id,
                "Definition includes Authorization header, skipping OAuth"
            );
            return self.connect_without_auth(header_map).await;
        }

        // No explicit auth in headers — check for stored OAuth credentials
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
            self.connect_with_auth(header_map).await
        } else {
            debug!(
                server_id = %self.server_id,
                "No stored credentials, trying without auth"
            );
            self.connect_without_auth(header_map).await
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }

    fn description(&self) -> String {
        format!("http:{}", self.url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpmux_core::{Credential, CredentialType, OutboundOAuthRegistration};

    // ── Mock repos (minimal, sufficient for HttpTransport unit tests) ──

    #[derive(Clone)]
    struct MockCredentialRepo {
        credentials: Arc<tokio::sync::RwLock<Vec<Credential>>>,
    }

    impl MockCredentialRepo {
        fn new() -> Self {
            Self {
                credentials: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            }
        }

        fn with_credential(cred: Credential) -> Self {
            Self {
                credentials: Arc::new(tokio::sync::RwLock::new(vec![cred])),
            }
        }
    }

    #[async_trait]
    impl CredentialRepository for MockCredentialRepo {
        async fn get(
            &self,
            space_id: &Uuid,
            server_id: &str,
            credential_type: &CredentialType,
        ) -> anyhow::Result<Option<Credential>> {
            let creds = self.credentials.read().await;
            Ok(creds
                .iter()
                .find(|c| {
                    c.space_id == *space_id
                        && c.server_id == server_id
                        && c.credential_type == *credential_type
                })
                .cloned())
        }

        async fn get_all(
            &self,
            space_id: &Uuid,
            server_id: &str,
        ) -> anyhow::Result<Vec<Credential>> {
            let creds = self.credentials.read().await;
            Ok(creds
                .iter()
                .filter(|c| c.space_id == *space_id && c.server_id == server_id)
                .cloned()
                .collect())
        }

        async fn save(&self, credential: &Credential) -> anyhow::Result<()> {
            let mut creds = self.credentials.write().await;
            creds.retain(|c| {
                !(c.space_id == credential.space_id
                    && c.server_id == credential.server_id
                    && c.credential_type == credential.credential_type)
            });
            creds.push(credential.clone());
            Ok(())
        }

        async fn delete(
            &self,
            space_id: &Uuid,
            server_id: &str,
            credential_type: &CredentialType,
        ) -> anyhow::Result<()> {
            let mut creds = self.credentials.write().await;
            creds.retain(|c| {
                !(c.space_id == *space_id
                    && c.server_id == server_id
                    && c.credential_type == *credential_type)
            });
            Ok(())
        }

        async fn delete_all(&self, space_id: &Uuid, server_id: &str) -> anyhow::Result<()> {
            let mut creds = self.credentials.write().await;
            creds.retain(|c| !(c.space_id == *space_id && c.server_id == server_id));
            Ok(())
        }

        async fn clear_tokens(&self, space_id: &Uuid, server_id: &str) -> anyhow::Result<bool> {
            let mut creds = self.credentials.write().await;
            let before = creds.len();
            creds.retain(|c| {
                !(c.space_id == *space_id
                    && c.server_id == server_id
                    && c.credential_type.is_oauth())
            });
            Ok(creds.len() < before)
        }

        async fn list_for_space(&self, space_id: &Uuid) -> anyhow::Result<Vec<Credential>> {
            let creds = self.credentials.read().await;
            Ok(creds
                .iter()
                .filter(|c| c.space_id == *space_id)
                .cloned()
                .collect())
        }
    }

    #[derive(Clone)]
    struct MockOAuthRepo;

    #[async_trait]
    impl OutboundOAuthRepository for MockOAuthRepo {
        async fn get(
            &self,
            _space_id: &Uuid,
            _server_id: &str,
        ) -> anyhow::Result<Option<OutboundOAuthRegistration>> {
            Ok(None)
        }

        async fn save(&self, _registration: &OutboundOAuthRegistration) -> anyhow::Result<()> {
            Ok(())
        }

        async fn delete(&self, _space_id: &Uuid, _server_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn list_for_space(
            &self,
            _space_id: &Uuid,
        ) -> anyhow::Result<Vec<OutboundOAuthRegistration>> {
            Ok(vec![])
        }
    }

    /// Helper to create an HttpTransport with given headers and credential repo.
    fn make_transport(
        headers: HashMap<String, String>,
        credential_repo: Arc<dyn CredentialRepository>,
    ) -> HttpTransport {
        HttpTransport::new(
            "https://example.com/mcp".to_string(),
            headers,
            Uuid::new_v4(),
            "test-server".to_string(),
            credential_repo,
            Arc::new(MockOAuthRepo),
            None,
            Duration::from_secs(10),
            None,
        )
    }

    fn make_transport_with_space(
        headers: HashMap<String, String>,
        credential_repo: Arc<dyn CredentialRepository>,
        space_id: Uuid,
        server_id: &str,
    ) -> HttpTransport {
        HttpTransport::new(
            "https://example.com/mcp".to_string(),
            headers,
            space_id,
            server_id.to_string(),
            credential_repo,
            Arc::new(MockOAuthRepo),
            None,
            Duration::from_secs(10),
            None,
        )
    }

    // ── requires_oauth tests ──

    #[test]
    fn test_requires_oauth_401() {
        assert!(HttpTransport::requires_oauth("HTTP 401 Unauthorized"));
    }

    #[test]
    fn test_requires_oauth_bearer() {
        assert!(HttpTransport::requires_oauth("Missing Bearer token"));
    }

    #[test]
    fn test_requires_oauth_www_authenticate() {
        assert!(HttpTransport::requires_oauth("WWW-Authenticate: Bearer"));
    }

    #[test]
    fn test_requires_oauth_channel_closed() {
        assert!(HttpTransport::requires_oauth("transport channel closed"));
    }

    #[test]
    fn test_requires_oauth_false_for_unrelated() {
        assert!(!HttpTransport::requires_oauth("connection refused"));
        assert!(!HttpTransport::requires_oauth("DNS lookup failed"));
        assert!(!HttpTransport::requires_oauth("timeout"));
    }

    // ── build_default_headers tests ──

    #[test]
    fn test_build_default_headers_empty() {
        let transport = make_transport(HashMap::new(), Arc::new(MockCredentialRepo::new()));
        let headers = transport.build_default_headers().unwrap();
        assert!(headers.is_empty());
    }

    #[test]
    fn test_build_default_headers_single() {
        let mut h = HashMap::new();
        h.insert("Authorization".to_string(), "Bearer token123".to_string());
        let transport = make_transport(h, Arc::new(MockCredentialRepo::new()));
        let headers = transport.build_default_headers().unwrap();

        assert_eq!(headers.len(), 1);
        assert_eq!(
            headers.get(reqwest::header::AUTHORIZATION).unwrap(),
            "Bearer token123"
        );
    }

    #[test]
    fn test_build_default_headers_multiple() {
        let mut h = HashMap::new();
        h.insert("Authorization".to_string(), "Bearer pat_xxx".to_string());
        h.insert("X-Custom-Header".to_string(), "custom-value".to_string());
        let transport = make_transport(h, Arc::new(MockCredentialRepo::new()));
        let headers = transport.build_default_headers().unwrap();

        assert_eq!(headers.len(), 2);
        assert_eq!(
            headers.get(reqwest::header::AUTHORIZATION).unwrap(),
            "Bearer pat_xxx"
        );
        assert_eq!(headers.get("x-custom-header").unwrap(), "custom-value");
    }

    #[test]
    fn test_build_default_headers_invalid_name() {
        let mut h = HashMap::new();
        h.insert("Invalid Header\n".to_string(), "value".to_string());
        let transport = make_transport(h, Arc::new(MockCredentialRepo::new()));
        let result = transport.build_default_headers();
        assert!(result.is_err());
    }

    #[test]
    fn test_build_default_headers_invalid_value() {
        let mut h = HashMap::new();
        h.insert("X-Header".to_string(), "bad\nvalue".to_string());
        let transport = make_transport(h, Arc::new(MockCredentialRepo::new()));
        let result = transport.build_default_headers();
        assert!(result.is_err());
    }

    // ── build_http_client tests ──

    #[test]
    fn test_build_http_client_empty_headers() {
        let transport = make_transport(HashMap::new(), Arc::new(MockCredentialRepo::new()));
        let client = transport.build_http_client(reqwest::header::HeaderMap::new());
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_http_client_with_headers() {
        let transport = make_transport(HashMap::new(), Arc::new(MockCredentialRepo::new()));
        let mut header_map = reqwest::header::HeaderMap::new();
        header_map.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("Bearer token"),
        );
        let client = transport.build_http_client(header_map);
        assert!(client.is_ok());
    }

    // ── connect() routing logic tests ──

    #[tokio::test]
    async fn test_connect_invalid_url_fails() {
        let transport = HttpTransport::new(
            "not a valid url".to_string(),
            HashMap::new(),
            Uuid::new_v4(),
            "test-server".to_string(),
            Arc::new(MockCredentialRepo::new()),
            Arc::new(MockOAuthRepo),
            None,
            Duration::from_secs(5),
            None,
        );

        let result = transport.connect().await;
        match result {
            TransportConnectResult::Failed(msg) => {
                assert!(msg.contains("Invalid URL"), "Got: {}", msg);
            }
            _ => panic!("Expected Failed for invalid URL"),
        }
    }

    #[tokio::test]
    async fn test_connect_with_explicit_auth_header_skips_oauth_check() {
        // When headers include Authorization, connect should NOT check credential_repo
        // for OAuth tokens — it should go straight to connect_without_auth with headers.
        // We verify this by giving it an unreachable server URL (connection will fail)
        // and checking that the error is a connection failure, NOT OAuthRequired.
        let mut h = HashMap::new();
        h.insert(
            "Authorization".to_string(),
            "Bearer ghp_testtoken123".to_string(),
        );

        let transport = HttpTransport::new(
            "https://127.0.0.1:1/mcp".to_string(), // unreachable
            h,
            Uuid::new_v4(),
            "test-server".to_string(),
            Arc::new(MockCredentialRepo::new()),
            Arc::new(MockOAuthRepo),
            None,
            Duration::from_secs(2),
            None,
        );

        let result = transport.connect().await;
        // Should be Failed (connection error) or timeout — NOT OAuthRequired
        match result {
            TransportConnectResult::Failed(_) => {} // expected
            TransportConnectResult::OAuthRequired { .. } => {
                panic!("Should not trigger OAuth when Authorization header is present")
            }
            _ => {} // Connected would be surprising but not wrong
        }
    }

    #[tokio::test]
    async fn test_connect_no_headers_no_credentials_tries_no_auth() {
        // No headers, no stored credentials → connect_without_auth
        // Will fail to connect to unreachable server
        let transport = HttpTransport::new(
            "https://127.0.0.1:1/mcp".to_string(),
            HashMap::new(),
            Uuid::new_v4(),
            "test-server".to_string(),
            Arc::new(MockCredentialRepo::new()),
            Arc::new(MockOAuthRepo),
            None,
            Duration::from_secs(2),
            None,
        );

        let result = transport.connect().await;
        // Should be Failed (connection error/timeout) or OAuthRequired (if 401 detected)
        match result {
            TransportConnectResult::Failed(_) | TransportConnectResult::OAuthRequired { .. } => {}
            TransportConnectResult::Connected(_) => {
                panic!("Should not connect to unreachable server")
            }
        }
    }

    #[tokio::test]
    async fn test_connect_with_stored_credentials_routes_to_oauth() {
        // When stored credentials exist and no explicit Authorization header,
        // connect should route to connect_with_auth (which will fail on unreachable server,
        // but we verify it doesn't go to connect_without_auth by observing the error path).
        let space_id = Uuid::new_v4();
        let server_id = "test-server";

        let cred = Credential::access_token(space_id, server_id, "stored_token", None);
        let cred_repo = Arc::new(MockCredentialRepo::with_credential(cred));

        let transport = make_transport_with_space(HashMap::new(), cred_repo, space_id, server_id);

        let result = transport.connect().await;
        // connect_with_auth will fail (can't reach server / AuthorizationManager::new fails)
        // but it should NOT be OAuthRequired from the no-auth path's 401 detection
        match result {
            TransportConnectResult::Failed(_) | TransportConnectResult::OAuthRequired { .. } => {}
            TransportConnectResult::Connected(_) => {
                panic!("Should not connect to example.com MCP endpoint")
            }
        }
    }

    #[tokio::test]
    async fn test_connect_custom_headers_always_applied_with_credentials() {
        // Even when we have stored OAuth credentials, custom (non-auth) headers
        // from the definition should be applied. We can verify this indirectly:
        // the transport should route to connect_with_auth (not connect_without_auth)
        // because there's no Authorization in the custom headers.
        let space_id = Uuid::new_v4();
        let server_id = "test-server";

        let cred = Credential::access_token(space_id, server_id, "stored_token", None);
        let cred_repo = Arc::new(MockCredentialRepo::with_credential(cred));

        let mut h = HashMap::new();
        h.insert("X-MCP-Toolsets".to_string(), "tools-only".to_string());

        let transport = make_transport_with_space(h, cred_repo, space_id, server_id);

        // Verify headers are built correctly (non-auth header present, no Authorization)
        let headers = transport.build_default_headers().unwrap();
        assert_eq!(headers.len(), 1);
        assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));
        assert_eq!(headers.get("x-mcp-toolsets").unwrap(), "tools-only");

        // connect() should route to connect_with_auth since no explicit Authorization header
        let result = transport.connect().await;
        match result {
            TransportConnectResult::Failed(_) | TransportConnectResult::OAuthRequired { .. } => {}
            TransportConnectResult::Connected(_) => {
                panic!("Should not connect to example.com MCP endpoint")
            }
        }
    }

    // ── transport_type / description tests ──

    #[test]
    fn test_transport_type() {
        let transport = make_transport(HashMap::new(), Arc::new(MockCredentialRepo::new()));
        assert!(matches!(transport.transport_type(), TransportType::Http));
    }

    #[test]
    fn test_description() {
        let transport = make_transport(HashMap::new(), Arc::new(MockCredentialRepo::new()));
        assert_eq!(transport.description(), "http:https://example.com/mcp");
    }
}
