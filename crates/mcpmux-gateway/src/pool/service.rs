//! Pool Service - Orchestrator for MCP server connections
//!
//! PoolService is the main entry point for managing server connections.
//! It orchestrates ConnectionService, FeatureService, and maintains the
//! map of active server instances.
//!
//! Key responsibilities:
//! - Managing active server instances (per space+server)
//! - Coordinating connect/disconnect operations
//! - Bulk connect on startup (reconnect_all_enabled)
//! - Providing access to server instances for routing

use std::sync::Arc;

use anyhow::Result;
use dashmap::DashMap;
use serde_json::Value;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::connection::{ConnectionResult, ConnectionService};
use super::context::ConnectionContext;
use super::features::{CachedFeatures, FeatureService};
use super::instance::{InstanceKey, InstanceState, ServerInstance};
use super::oauth::OutboundOAuthManager;
use super::token::TokenService;
use super::transport::{ResolvedTransport, TransportType};

/// Check if an error string indicates an authentication/authorization failure
fn is_auth_error(error_str: &str) -> bool {
    let lower = error_str.to_lowercase();
    let indicators = [
        "401",
        "unauthorized",
        "invalid_token",
        "token expired",
        "access token",
    ];
    indicators.iter().any(|s| lower.contains(s))
}

/// Result of bulk reconnect operation
#[derive(Debug, Default)]
pub struct ReconnectResult {
    /// Successfully connected servers
    pub connected: Vec<(String, Uuid)>,
    /// Servers that were already connected (reused)
    pub reused: Vec<(String, Uuid)>,
    /// Failed connections with error messages
    pub failed: Vec<(String, Uuid, String)>,
    /// Servers requiring OAuth interaction
    pub needs_oauth: Vec<(String, Uuid)>,
}

impl ReconnectResult {
    pub fn total_success(&self) -> usize {
        self.connected.len() + self.reused.len()
    }

    pub fn total_failed(&self) -> usize {
        self.failed.len() + self.needs_oauth.len()
    }
}

/// Pool statistics
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    pub total_instances: usize,
    pub connected_instances: usize,
    pub connecting_instances: usize,
    pub failed_instances: usize,
    pub oauth_pending_instances: usize,
}

/// Pool Service - main orchestrator for server connections
pub struct PoolService {
    /// Active server instances keyed by (space_id, server_id)
    instances: DashMap<(Uuid, String), Arc<ServerInstance>>,
    /// Connection service
    connection_service: Arc<ConnectionService>,
    /// Feature service
    feature_service: Arc<FeatureService>,
    /// Token service (exposed for routing)
    token_service: Arc<TokenService>,
}

impl PoolService {
    pub fn new(
        connection_service: Arc<ConnectionService>,
        feature_service: Arc<FeatureService>,
        token_service: Arc<TokenService>,
    ) -> Self {
        Self {
            instances: DashMap::new(),
            connection_service,
            feature_service,
            token_service,
        }
    }

    /// Get the token service for token operations
    pub fn token_service(&self) -> Arc<TokenService> {
        self.token_service.clone()
    }

    /// Get the feature service for feature resolution
    pub fn feature_service(&self) -> Arc<FeatureService> {
        self.feature_service.clone()
    }

    /// Get the OAuth manager for checking pending flows
    pub fn oauth_manager(&self) -> Arc<OutboundOAuthManager> {
        self.connection_service.oauth_manager()
    }

    /// Read a resource from a backend server
    ///
    /// On auth errors, automatically reconnects the server and retries once.
    pub async fn read_resource(
        &self,
        space_id: Uuid,
        server_id: &str,
        uri: &str,
    ) -> Result<Vec<Value>> {
        match self.try_read_resource(space_id, server_id, uri).await {
            Ok(content) => Ok(content),
            Err(e) if is_auth_error(&e.to_string()) => {
                warn!(
                    "[PoolService] Auth error on read_resource for {}/{}, attempting auto-reconnect",
                    server_id, uri
                );
                match self.reconnect_instance(space_id, server_id).await {
                    ConnectionResult::Connected { .. } => {
                        info!(
                            "[PoolService] Reconnected {}, retrying read_resource",
                            server_id
                        );
                        self.try_read_resource(space_id, server_id, uri).await
                    }
                    _ => Err(anyhow::anyhow!(
                        "Server '{}' auth error on read_resource. Auto-reconnect failed. Please disconnect and connect again.",
                        server_id
                    )),
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Internal: attempt to read a resource without retry logic
    async fn try_read_resource(
        &self,
        space_id: Uuid,
        server_id: &str,
        uri: &str,
    ) -> Result<Vec<Value>> {
        let instance = self
            .get_instance(space_id, server_id)
            .ok_or_else(|| anyhow::anyhow!("Server not connected: {}", server_id))?;

        let client_handle = instance.with_client(|client| client.peer().clone());

        match client_handle {
            Some(client) => {
                use rmcp::model::ReadResourceRequestParams;

                let params = ReadResourceRequestParams {
                    uri: uri.into(),
                    meta: None,
                };

                let res = client
                    .read_resource(params)
                    .await
                    .map_err(|e| anyhow::anyhow!("MCP read_resource failed: {}", e))?;

                let content: Vec<Value> = res
                    .contents
                    .into_iter()
                    .map(|c| serde_json::to_value(c).unwrap_or(Value::Null))
                    .collect();

                Ok(content)
            }
            None => Err(anyhow::anyhow!("Server instance has no active client")),
        }
    }

    /// Get a prompt from a backend server
    ///
    /// On auth errors, automatically reconnects the server and retries once.
    pub async fn get_prompt(
        &self,
        space_id: Uuid,
        server_id: &str,
        prompt_name: &str,
        arguments: Option<serde_json::Map<String, Value>>,
    ) -> Result<Value> {
        match self
            .try_get_prompt(space_id, server_id, prompt_name, arguments.clone())
            .await
        {
            Ok(value) => Ok(value),
            Err(e) if is_auth_error(&e.to_string()) => {
                warn!(
                    "[PoolService] Auth error on get_prompt for {}/{}, attempting auto-reconnect",
                    server_id, prompt_name
                );
                match self.reconnect_instance(space_id, server_id).await {
                    ConnectionResult::Connected { .. } => {
                        info!(
                            "[PoolService] Reconnected {}, retrying get_prompt",
                            server_id
                        );
                        self.try_get_prompt(space_id, server_id, prompt_name, arguments)
                            .await
                    }
                    _ => Err(anyhow::anyhow!(
                        "Server '{}' auth error on get_prompt. Auto-reconnect failed. Please disconnect and connect again.",
                        server_id
                    )),
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Internal: attempt to get a prompt without retry logic
    async fn try_get_prompt(
        &self,
        space_id: Uuid,
        server_id: &str,
        prompt_name: &str,
        arguments: Option<serde_json::Map<String, Value>>,
    ) -> Result<Value> {
        let instance = self
            .get_instance(space_id, server_id)
            .ok_or_else(|| anyhow::anyhow!("Server not connected: {}", server_id))?;

        let client_handle = instance.with_client(|client| client.peer().clone());

        match client_handle {
            Some(client) => {
                use rmcp::model::GetPromptRequestParams;

                let params = GetPromptRequestParams {
                    name: prompt_name.into(),
                    arguments,
                    meta: None,
                };

                let res = client
                    .get_prompt(params)
                    .await
                    .map_err(|e| anyhow::anyhow!("MCP get_prompt failed: {}", e))?;

                // Return the full response as JSON
                serde_json::to_value(res)
                    .map_err(|e| anyhow::anyhow!("Failed to serialize prompt response: {}", e))
            }
            None => Err(anyhow::anyhow!("Server instance has no active client")),
        }
    }

    /// Connect a server for a space
    pub async fn connect_server(&self, ctx: &ConnectionContext) -> ConnectionResult {
        let key = (ctx.space_id, ctx.server_id.to_string());

        // Check for existing instance
        if let Some(instance) = self.instances.get(&key) {
            if instance.is_healthy() {
                debug!(
                    "[PoolService] Reusing existing instance for {}/{}",
                    ctx.space_id, ctx.server_id
                );

                return ConnectionResult::Connected {
                    reused: true,
                    // Empty cached features - they're already in DB
                    features: CachedFeatures::default(),
                };
            }

            // Existing instance but not healthy - reconnect through it
            return self
                .connection_service
                .connect_with_instance(ctx, &instance, &self.feature_service)
                .await;
        }

        // Create new instance
        let transport_type = match &ctx.transport {
            ResolvedTransport::Stdio { .. } => TransportType::Stdio,
            ResolvedTransport::Http { .. } => TransportType::Http,
        };

        // Use proper InstanceKey constructors that include the URL
        let instance_key = match &ctx.transport {
            ResolvedTransport::Stdio { command, args, env } => {
                InstanceKey::stdio(ctx.space_id, command, args, env)
            }
            ResolvedTransport::Http { url, headers, .. } => {
                InstanceKey::http(ctx.space_id, url, headers)
            }
        };

        let instance = Arc::new(ServerInstance::new(
            instance_key,
            ctx.server_id.to_string(),
            transport_type,
        ));

        // Store instance - keyed by (space_id, server_id) for complete isolation
        self.instances.insert(key.clone(), instance.clone());

        // Connect through connection service
        let result = self
            .connection_service
            .connect_with_instance(ctx, &instance, &self.feature_service)
            .await;

        // If connection failed completely, remove the instance
        if let ConnectionResult::Failed { .. } = &result {
            self.instances.remove(&key);
        }

        result
    }

    /// Remove instance only (for disable - keeps tokens)
    pub fn remove_instance(&self, space_id: Uuid, server_id: &str) {
        let key = (space_id, server_id.to_string());

        if let Some((_, _instance)) = self.instances.remove(&key) {
            info!(
                "[PoolService] Removed instance for {}/{} (tokens preserved)",
                space_id, server_id
            );
        }
    }

    /// Disconnect a server (logout - clears tokens but keeps DCR)
    pub async fn disconnect_server(&self, space_id: Uuid, server_id: &str) -> Result<()> {
        // Cancel any pending OAuth flows first
        self.connection_service
            .oauth_manager()
            .cancel_flow_for_space(space_id, server_id);

        // Remove instance
        self.remove_instance(space_id, server_id);

        // Disconnect through connection service (clears tokens, marks features unavailable)
        self.connection_service
            .disconnect(space_id, server_id, &self.feature_service)
            .await
    }

    /// Get an instance for a space/server pair
    pub fn get_instance(&self, space_id: Uuid, server_id: &str) -> Option<Arc<ServerInstance>> {
        self.instances
            .get(&(space_id, server_id.to_string()))
            .map(|r| r.clone())
    }

    /// Check if a server is connected
    pub fn is_connected(&self, space_id: Uuid, server_id: &str) -> bool {
        self.get_instance(space_id, server_id)
            .map(|i| i.is_healthy())
            .unwrap_or(false)
    }

    /// Get all instances for a space
    pub fn instances_for_space(&self, space_id: Uuid) -> Vec<Arc<ServerInstance>> {
        self.instances
            .iter()
            .filter(|entry| entry.key().0 == space_id)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        let mut stats = PoolStats::default();

        for entry in self.instances.iter() {
            stats.total_instances += 1;
            match entry.value().state() {
                InstanceState::Connected => stats.connected_instances += 1,
                InstanceState::Connecting => stats.connecting_instances += 1,
                InstanceState::Failed => stats.failed_instances += 1,
                InstanceState::OAuthPending => stats.oauth_pending_instances += 1,
                InstanceState::Disconnected => {}
            }
        }

        stats
    }

    /// Reconnect an existing instance (e.g., after OAuth completes)
    ///
    /// This is called when OAuth flow completes to reconnect with the new token.
    /// Uses the connection service to reconnect using stored credentials.
    pub async fn reconnect_instance(&self, space_id: Uuid, server_id: &str) -> ConnectionResult {
        let key = (space_id, server_id.to_string());

        // Get existing instance
        let instance = match self.instances.get(&key) {
            Some(inst) => inst.clone(),
            None => {
                info!(
                    "[PoolService] No instance found for {}/{}, cannot reconnect",
                    space_id, server_id
                );
                return ConnectionResult::Failed {
                    error: "No instance found to reconnect".to_string(),
                };
            }
        };

        info!(
            "[PoolService] Reconnecting instance for {}/{} after OAuth",
            space_id, server_id
        );

        // Reconnect using connection service with OAuth tokens
        self.connection_service
            .reconnect_after_oauth(space_id, server_id, &instance, &self.feature_service)
            .await
    }

    /// Disconnect all servers in a space
    pub async fn disconnect_space(&self, space_id: Uuid) -> Result<()> {
        let server_ids: Vec<String> = self
            .instances
            .iter()
            .filter(|entry| entry.key().0 == space_id)
            .map(|entry| entry.key().1.clone())
            .collect();

        for server_id in server_ids {
            if let Err(e) = self.disconnect_server(space_id, &server_id).await {
                warn!(
                    "[PoolService] Failed to disconnect {}/{}: {}",
                    space_id, server_id, e
                );
            }
        }

        Ok(())
    }

    /// Reconnect all enabled servers on startup
    ///
    /// This is called when the gateway starts to restore connections
    /// for servers that were previously connected.
    pub async fn reconnect_all_enabled<F>(
        &self,
        installed_servers: Vec<InstalledServerInfo>,
        get_config: F,
    ) -> ReconnectResult
    where
        F: Fn(&InstalledServerInfo) -> Option<ResolvedTransport>,
    {
        let mut result = ReconnectResult::default();

        info!(
            "[PoolService] Reconnecting {} enabled servers",
            installed_servers.len()
        );

        for server in installed_servers {
            // Skip servers that don't have OAuth credentials if they require OAuth
            if server.requires_oauth && !server.has_credentials {
                debug!(
                    "[PoolService] Skipping {} - requires OAuth but no credentials",
                    server.server_id
                );
                result
                    .needs_oauth
                    .push((server.server_id.clone(), server.space_id));
                continue;
            }

            // Get transport config
            let config = match get_config(&server) {
                Some(c) => c,
                None => {
                    warn!("[PoolService] No transport config for {}", server.server_id);
                    result.failed.push((
                        server.server_id.clone(),
                        server.space_id,
                        "No transport config".to_string(),
                    ));
                    continue;
                }
            };

            // Attempt connection (auto-reconnect mode - no browser opening)
            let ctx = ConnectionContext::new(server.space_id, server.server_id.clone(), config)
                .with_auto_reconnect(true);
            match self.connect_server(&ctx).await {
                ConnectionResult::Connected { reused, .. } => {
                    if reused {
                        result.reused.push((server.server_id, server.space_id));
                    } else {
                        result.connected.push((server.server_id, server.space_id));
                    }
                }
                ConnectionResult::OAuthRequired { .. } => {
                    result.needs_oauth.push((server.server_id, server.space_id));
                }
                ConnectionResult::Failed { error } => {
                    result
                        .failed
                        .push((server.server_id, server.space_id, error));
                }
            }
        }

        info!(
            "[PoolService] Reconnect complete: {} connected, {} reused, {} failed, {} need OAuth",
            result.connected.len(),
            result.reused.len(),
            result.failed.len(),
            result.needs_oauth.len()
        );

        result
    }

    /// Get the server URL for an instance (for OAuth token refresh).
    /// Returns None for STDIO transports or if instance not found.
    pub fn get_server_url(&self, space_id: Uuid, server_id: &str) -> Option<String> {
        self.get_instance(space_id, server_id)
            .and_then(|instance| instance.get_url())
    }
}

/// Info about an installed server for reconnection
#[derive(Debug, Clone)]
pub struct InstalledServerInfo {
    pub space_id: Uuid,
    pub server_id: String,
    pub requires_oauth: bool,
    pub has_credentials: bool,
}
