//! Startup Orchestrator - Handles Gateway initialization tasks
//!
//! Follows Single Responsibility Principle - only concerned with startup logic.
//! Keeps GatewayServer focused on serving requests, not initialization.

use std::sync::Arc;

use anyhow::Result;
use mcpmux_core::InstalledServer;
use tracing::{info, warn};

use crate::pool::{ConnectionContext, ConnectionResult, PoolService, ServerManager};
use crate::services::PrefixCacheService;

use super::GatewayDependencies;

/// Orchestrates startup tasks for the Gateway
///
/// Keeps initialization logic separate from server logic (SRP).
/// Uses dependency injection for clean, testable code.
pub struct StartupOrchestrator {
    pool_service: Arc<PoolService>,
    server_manager: Arc<ServerManager>,
    dependencies: GatewayDependencies,
    prefix_cache_service: Arc<PrefixCacheService>,
}

impl StartupOrchestrator {
    /// Create a new startup orchestrator with dependency injection
    pub fn new(
        pool_service: Arc<PoolService>,
        server_manager: Arc<ServerManager>,
        dependencies: GatewayDependencies,
        prefix_cache_service: Arc<PrefixCacheService>,
    ) -> Self {
        Self {
            pool_service,
            server_manager,
            dependencies,
            prefix_cache_service,
        }
    }
    
    /// Mark all features as unavailable on startup
    /// 
    /// This ensures features don't appear available until servers reconnect.
    /// Should be called BEFORE auto-connecting servers.
    pub async fn mark_all_features_unavailable(&self) -> Result<()> {
        info!("[Startup] Marking all features as unavailable (will be restored when servers connect)...");
        
        // Get all installed servers
        let installed_servers = self.dependencies.installed_server_repo.list().await?;
        
        let mut count = 0;
        for server in installed_servers {
            if let Err(e) = self.dependencies.feature_repo
                .mark_unavailable(&server.space_id, &server.server_id)
                .await
            {
                warn!(
                    "[Startup] Failed to mark features unavailable for {}/{}: {}",
                    server.space_id, server.server_id, e
                );
            } else {
                count += 1;
            }
        }
        
        info!("[Startup] Marked features unavailable for {} servers", count);
        Ok(())
    }
    
    /// Resolve server prefixes for all spaces
    /// 
    /// Should be called BEFORE auto-connecting servers to ensure
    /// tools have correct prefixes when clients connect.
    pub async fn resolve_server_prefixes(&self) -> Result<()> {
        info!("[Startup] Resolving server prefixes for all spaces...");
        
        // Get all spaces
        let spaces = self.dependencies.space_repo.list().await?;
        
        for space in spaces {
            let space_id = space.id.to_string();
            match self.prefix_cache_service.resolve_prefixes_on_startup(&space_id).await {
                Ok(()) => {
                    info!("[Startup] ✓ Prefixes resolved for space: {}", space.name);
                }
                Err(e) => {
                    warn!("[Startup] Failed to resolve prefixes for space {}: {}", space.name, e);
                }
            }
        }
        
        info!("[Startup] Server prefix resolution complete");
        Ok(())
    }
    
    /// Refresh OAuth tokens for all HTTP/SSE servers before attempting connections
    ///
    /// **DEPRECATED**: This method is now a no-op.
    /// RMCP's AuthClient with DatabaseCredentialStore handles token refresh
    /// automatically on every request, so preemptive refresh is no longer needed.
    ///
    /// Keeping this method for backwards compatibility with Tauri commands.
    pub async fn refresh_oauth_tokens(&self) -> Result<TokenRefreshResult> {
        info!("[Startup] Token refresh skipped - RMCP AuthClient handles refresh per-request");
        Ok(TokenRefreshResult::default())
    }

    /// Auto-connect all enabled servers on startup
    ///
    /// This runs in the background and doesn't block Gateway startup.
    /// OAuth-based servers without tokens are skipped gracefully.
    pub async fn auto_connect_enabled_servers(&self) -> Result<AutoConnectResult> {
        info!("[Startup] Auto-connecting enabled servers...");

        let mut result = AutoConnectResult::default();

        // Get all installed servers
        let installed_servers = self
            .dependencies
            .installed_server_repo
            .list()
            .await?;

        // Filter to enabled servers only
        let enabled_servers: Vec<_> = installed_servers
            .into_iter()
            .filter(|server| server.enabled)
            .collect();

        info!(
            "[Startup] Found {} enabled server(s) to connect",
            enabled_servers.len()
        );

        // IMPORTANT: Pre-set all enabled servers to "Connecting" status BEFORE starting connections
        // This prevents UI from showing stale "auth_required" status during startup
        for server in &enabled_servers {
            let space_id = match uuid::Uuid::parse_str(&server.space_id) {
                Ok(id) => id,
                Err(e) => {
                    warn!("[Startup] Invalid space_id for {}: {}", server.server_id, e);
                    continue;
                }
            };
            let key = crate::pool::ServerKey::new(space_id, server.server_id.clone());
            let _ = self.server_manager.set_connecting(&key).await;
        }

        for server in enabled_servers {
            match self.connect_server(&server).await {
                Ok(ConnectOutcome::Connected) => {
                    info!(
                        "[Startup] ✓ Connected: {}/{}",
                        server.space_id, server.server_id
                    );
                    result.connected.push(server.server_id.clone());
                }
                Ok(ConnectOutcome::AlreadyConnected) => {
                    info!(
                        "[Startup] ✓ Already connected: {}/{}",
                        server.space_id, server.server_id
                    );
                    result.already_connected.push(server.server_id.clone());
                }
                Ok(ConnectOutcome::NeedsOAuth) => {
                    info!(
                        "[Startup] ⊗ Skipped (needs OAuth): {}/{}",
                        server.space_id, server.server_id
                    );
                    result.needs_oauth.push(server.server_id.clone());
                }
                Err(e) => {
                    warn!(
                        "[Startup] ✗ Failed to connect {}/{}: {}",
                        server.space_id, server.server_id, e
                    );
                    result
                        .failed
                        .push((server.server_id.clone(), e.to_string()));
                }
            }
        }

        info!(
            "[Startup] Auto-connect complete: {} connected, {} skipped (OAuth), {} failed",
            result.connected.len() + result.already_connected.len(),
            result.needs_oauth.len(),
            result.failed.len()
        );

        Ok(result)
    }

    /// Connect a single server
    async fn connect_server(&self, server: &InstalledServer) -> Result<ConnectOutcome> {
        // Get server definition: prefer cached definition, fallback to registry for legacy
        let definition = match server.get_definition() {
            Some(def) => def,
            None => {
                // Fallback: try registry for servers installed before caching was added
                self.dependencies.server_discovery.refresh_if_needed().await?;
                self.dependencies.server_discovery.get(&server.server_id).await
                    .ok_or_else(|| anyhow::anyhow!(
                        "No cached definition and not found in registry: {}", 
                        server.server_id
                    ))?
            }
        };

        // Parse space_id to UUID
        let space_id = uuid::Uuid::parse_str(&server.space_id)
            .map_err(|e| anyhow::anyhow!("Invalid space_id: {}", e))?;

        // Check if server requires OAuth but hasn't been approved yet
        // This prevents auto-connect from setting "Connected" status without user approval
        let requires_oauth = matches!(definition.auth, Some(mcpmux_core::domain::AuthConfig::Oauth));
        
        if requires_oauth && !server.oauth_connected {
            info!(
                "[Startup] Skipping {}/{} - requires OAuth approval",
                server.space_id, server.server_id
            );
            let key = crate::pool::ServerKey::new(space_id, server.server_id.clone());
            self.server_manager.set_auth_required(&key, Some("OAuth authentication required".to_string())).await;
            return Ok(ConnectOutcome::NeedsOAuth);
        }

        // Build transport config using cached definition
        let transport_config = crate::pool::transport::resolution::build_transport_config(
            &definition.transport,
            server,
            self.dependencies.state_dir.as_deref(),
        );

        // Explicitly set state to connecting in ServerManager BEFORE starting connection
        // This ensures the UI reflects the "Connecting" state during startup
        let key = crate::pool::ServerKey::new(space_id, server.server_id.clone());
        let _ = self.server_manager.set_connecting(&key).await;

        // Attempt connection through pool service (auto-connect mode: don't start OAuth flow)
        // For auto-connect, we pass auto_reconnect=true so OAuth-required servers just return
        // OAuthRequired without starting the callback server or opening browser
        let ctx = ConnectionContext::new(space_id, server.server_id.clone(), transport_config)
            .with_auto_reconnect(true);
        let connection_result = self
            .pool_service
            .connect_server(&ctx)
            .await;

        match connection_result {
            ConnectionResult::Connected { reused, features } => {
                // Explicitly update ServerManager status to Connected
                // While PoolService might update instance state, ServerManager is the source of truth for UI events
                self.server_manager.set_connected(&key, features).await;
                
                if reused {
                    Ok(ConnectOutcome::AlreadyConnected)
                } else {
                    Ok(ConnectOutcome::Connected)
                }
            }
            ConnectionResult::OAuthRequired { .. } => {
                // Explicitly set status to AuthRequired
                self.server_manager.set_auth_required(&key, None).await;
                Ok(ConnectOutcome::NeedsOAuth)
            },
            ConnectionResult::Failed { error } => {
                // Explicitly set status to Error
                self.server_manager.set_error(&key, error.clone()).await;
                Err(anyhow::anyhow!("Connection failed: {}", error))
            }
        }
    }
}

/// Result of auto-connect operation
#[derive(Debug, Default)]
pub struct AutoConnectResult {
    pub connected: Vec<String>,
    pub already_connected: Vec<String>,
    pub needs_oauth: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Result of token refresh operation
#[derive(Debug, Default)]
pub struct TokenRefreshResult {
    pub servers_checked: usize,
    pub tokens_refreshed: usize,
    pub refresh_failed: usize,
}

/// Outcome of connecting a single server
enum ConnectOutcome {
    Connected,
    AlreadyConnected,
    NeedsOAuth,
}
