//! MCP Client Pool
//!
//! Manages connections to MCP servers using config-hash-based pooling.
//!
//! The pool key is computed as: `server_id + ":" + sha256(final_config)[:16]`
//! where final_config includes the server configuration and credential values.
//! This ensures that:
//! - Different credentials get different clients
//! - Same credentials share the same client
//! - Token refresh naturally creates new clients

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use ring::digest::{Context, SHA256};
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::transports::{ConnectionStatus, McpSession};

/// Configuration that uniquely identifies a pooled connection
#[derive(Debug, Clone, Serialize)]
pub struct PoolConfig {
    /// Server ID
    pub server_id: String,
    /// Transport configuration (serialized)
    pub transport_config: String,
    /// Environment variables including credentials
    pub env_vars: HashMap<String, String>,
}

impl PoolConfig {
    /// Compute the pool key for this configuration
    pub fn pool_key(&self) -> String {
        let config_json = serde_json::to_string(self).unwrap_or_default();
        let hash = sha256_hex(&config_json);
        format!("{}:{}", self.server_id, &hash[..16])
    }
}

/// Compute SHA256 hash of a string and return as hex
fn sha256_hex(input: &str) -> String {
    let mut context = Context::new(&SHA256);
    context.update(input.as_bytes());
    let digest = context.finish();
    hex::encode(digest.as_ref())
}

/// A pooled MCP client with usage tracking
pub struct PooledClient {
    /// The MCP session
    pub session: McpSession,
    /// Pool key for this client
    pub pool_key: String,
    /// Number of active references
    pub ref_count: usize,
    /// Last activity timestamp
    pub last_activity: Instant,
}

impl PooledClient {
    /// Create a new pooled client
    pub fn new(session: McpSession, pool_key: String) -> Self {
        Self {
            session,
            pool_key,
            ref_count: 1,
            last_activity: Instant::now(),
        }
    }

    /// Check if the client is idle (no references)
    pub fn is_idle(&self) -> bool {
        self.ref_count == 0
    }

    /// Get idle duration
    pub fn idle_duration(&self) -> Duration {
        self.last_activity.elapsed()
    }
}

/// Default idle timeout (5 minutes)
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Pool of MCP clients keyed by config hash
pub struct ClientPool {
    /// Clients by pool key
    clients: Arc<RwLock<HashMap<String, Arc<RwLock<PooledClient>>>>>,
    /// Idle timeout for cleanup
    idle_timeout: Duration,
}

impl ClientPool {
    /// Create a new client pool
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    /// Create with custom idle timeout
    pub fn with_idle_timeout(idle_timeout: Duration) -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            idle_timeout,
        }
    }

    /// Get or create a client for the given configuration
    pub async fn get_or_create<F, Fut>(
        &self,
        config: &PoolConfig,
        create_fn: F,
    ) -> Result<Arc<RwLock<PooledClient>>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<McpSession>>,
    {
        let pool_key = config.pool_key();

        // Check if we have an existing client
        {
            let clients = self.clients.read().await;
            if let Some(client) = clients.get(&pool_key) {
                let mut client_guard = client.write().await;
                client_guard.ref_count += 1;
                client_guard.last_activity = Instant::now();
                debug!(pool_key = %pool_key, ref_count = client_guard.ref_count, "Reusing pooled client");
                return Ok(Arc::clone(client));
            }
        }

        // Create new client
        info!(pool_key = %pool_key, "Creating new pooled client");
        let session = create_fn().await?;
        let pooled_client = PooledClient::new(session, pool_key.clone());
        let client_arc = Arc::new(RwLock::new(pooled_client));

        // Store in pool
        {
            let mut clients = self.clients.write().await;
            clients.insert(pool_key, Arc::clone(&client_arc));
        }

        Ok(client_arc)
    }

    /// Release a client reference
    pub async fn release(&self, pool_key: &str) {
        let clients = self.clients.read().await;
        if let Some(client) = clients.get(pool_key) {
            let mut client_guard = client.write().await;
            if client_guard.ref_count > 0 {
                client_guard.ref_count -= 1;
                client_guard.last_activity = Instant::now();
            }
            debug!(pool_key = %pool_key, ref_count = client_guard.ref_count, "Released client reference");
        }
    }

    /// Clean up idle clients
    pub async fn cleanup_idle(&self) -> usize {
        let mut removed = 0;
        let mut to_remove = Vec::new();

        {
            let clients = self.clients.read().await;
            for (key, client) in clients.iter() {
                let client_guard = client.read().await;
                if client_guard.is_idle() && client_guard.idle_duration() > self.idle_timeout {
                    to_remove.push(key.clone());
                }
            }
        }

        for key in to_remove {
            if let Some(client) = self.clients.write().await.remove(&key) {
                // Disconnect the session
                if let Ok(client) = Arc::try_unwrap(client) {
                    let pooled = client.into_inner();
                    let _ = pooled.session.disconnect().await;
                    removed += 1;
                    info!(pool_key = %key, "Cleaned up idle client");
                }
            }
        }

        removed
    }

    /// Get the number of clients in the pool
    pub async fn len(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Check if the pool is empty
    pub async fn is_empty(&self) -> bool {
        self.clients.read().await.is_empty()
    }

    /// Get all pool keys
    pub async fn keys(&self) -> Vec<String> {
        self.clients.read().await.keys().cloned().collect()
    }

    /// Get client status by pool key
    pub async fn status(&self, pool_key: &str) -> Option<ConnectionStatus> {
        let clients = self.clients.read().await;
        if let Some(client) = clients.get(pool_key) {
            Some(client.read().await.session.status.clone())
        } else {
            None
        }
    }
}

impl Default for ClientPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_key_generation() {
        let config = PoolConfig {
            server_id: "github".to_string(),
            transport_config:
                r#"{"command":"npx","args":["-y","@modelcontextprotocol/server-github"]}"#
                    .to_string(),
            env_vars: [("GITHUB_TOKEN".to_string(), "ghp_xxx".to_string())]
                .into_iter()
                .collect(),
        };

        let key = config.pool_key();
        assert!(key.starts_with("github:"));
        assert_eq!(key.len(), "github:".len() + 16);
    }

    #[test]
    fn test_same_config_same_key() {
        let config1 = PoolConfig {
            server_id: "github".to_string(),
            transport_config: "same".to_string(),
            env_vars: [("TOKEN".to_string(), "xxx".to_string())]
                .into_iter()
                .collect(),
        };

        let config2 = PoolConfig {
            server_id: "github".to_string(),
            transport_config: "same".to_string(),
            env_vars: [("TOKEN".to_string(), "xxx".to_string())]
                .into_iter()
                .collect(),
        };

        assert_eq!(config1.pool_key(), config2.pool_key());
    }

    #[test]
    fn test_different_token_different_key() {
        let config1 = PoolConfig {
            server_id: "github".to_string(),
            transport_config: "same".to_string(),
            env_vars: [("TOKEN".to_string(), "xxx".to_string())]
                .into_iter()
                .collect(),
        };

        let config2 = PoolConfig {
            server_id: "github".to_string(),
            transport_config: "same".to_string(),
            env_vars: [("TOKEN".to_string(), "yyy".to_string())]
                .into_iter()
                .collect(),
        };

        assert_ne!(config1.pool_key(), config2.pool_key());
    }

    #[tokio::test]
    async fn test_client_pool_new() {
        let pool = ClientPool::new();
        assert!(pool.is_empty().await);
    }
}
