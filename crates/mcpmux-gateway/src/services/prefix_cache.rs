//! Prefix Cache Service
//!
//! Manages bidirectional caching of server prefixes (alias or server_id) for tool name qualification.
//! 
//! Key Principles:
//! - Startup: Priority-based resolution (verified > created_at)
//! - Runtime: Stable, first-come-first-served, no stealing
//!
//! This prevents client confusion from prefix changes during active connections.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use mcpmux_core::{InstalledServerRepository, ServerDiscoveryService};

/// Bidirectional cache mapping between server IDs and prefixes
/// 
/// Each server always has a prefix (either its alias or server_id with / -> .)
#[derive(Debug, Clone, Default)]
struct SpacePrefixCache {
    /// Forward: server_id -> resolved prefix (alias or server_id)
    server_to_prefix: HashMap<String, String>,
    
    /// Reverse: resolved prefix -> server_id (for routing)
    prefix_to_server: HashMap<String, String>,
}

impl SpacePrefixCache {
    fn new() -> Self {
        Self::default()
    }
    
    /// Assign a prefix to a server (bidirectional insert)
    fn assign(&mut self, server_id: String, prefix: String) {
        self.server_to_prefix.insert(server_id.clone(), prefix.clone());
        self.prefix_to_server.insert(prefix, server_id);
    }
    
    /// Remove a server's prefix assignment (bidirectional remove)
    fn remove(&mut self, server_id: &str) -> Option<String> {
        if let Some(prefix) = self.server_to_prefix.remove(server_id) {
            self.prefix_to_server.remove(&prefix);
            Some(prefix)
        } else {
            None
        }
    }
    
    /// Get the prefix for a server (for tools/list)
    fn get_prefix(&self, server_id: &str) -> Option<&str> {
        self.server_to_prefix.get(server_id).map(|s| s.as_str())
    }
    
    /// Get the server for a prefix (for tools/call routing)
    fn get_server(&self, prefix: &str) -> Option<&str> {
        self.prefix_to_server.get(prefix).map(|s| s.as_str())
    }
    
    /// Check if a prefix is available
    fn is_prefix_available(&self, prefix: &str) -> bool {
        !self.prefix_to_server.contains_key(prefix)
    }
    
    /// Clear all mappings
    fn clear(&mut self) {
        self.server_to_prefix.clear();
        self.prefix_to_server.clear();
    }
}

/// Service for managing server prefix resolution and caching
/// 
/// Handles:
/// - Startup resolution with priority
/// - Runtime assignment (stable, no stealing)
/// - Cache lookups for tools/list and tools/call
pub struct PrefixCacheService {
    /// Per-space caches (space_id -> cache)
    caches: Arc<RwLock<HashMap<String, SpacePrefixCache>>>,
    
    /// Installed server repository (for startup resolution)
    installed_server_repo: Option<Arc<dyn InstalledServerRepository>>,
    
    /// Server discovery service (for getting server definitions)
    server_discovery: Option<Arc<ServerDiscoveryService>>,
}

impl PrefixCacheService {
    pub fn new() -> Self {
        Self {
            caches: Arc::new(RwLock::new(HashMap::new())),
            installed_server_repo: None,
            server_discovery: None,
        }
    }
    
    /// Set dependencies (for startup resolution)
    pub fn with_dependencies(
        mut self,
        installed_server_repo: Arc<dyn InstalledServerRepository>,
        server_discovery: Arc<ServerDiscoveryService>,
    ) -> Self {
        self.installed_server_repo = Some(installed_server_repo);
        self.server_discovery = Some(server_discovery);
        self
    }
    
    /// Resolve prefixes for a space on startup (priority-based)
    /// 
    /// Priority:
    /// 1. Verified servers (from registry)
    /// 2. First installed (created_at)
    /// 
    /// This should ONLY be called on app startup, before any MCP clients connect.
    pub async fn resolve_prefixes_on_startup(&self, space_id: &str) -> anyhow::Result<()> {
        let installed_server_repo = self.installed_server_repo.as_ref()
            .ok_or_else(|| anyhow::anyhow!("InstalledServerRepository not set"))?;
        let server_discovery = self.server_discovery.as_ref()
            .ok_or_else(|| anyhow::anyhow!("ServerDiscoveryService not set"))?;
        
        // Refresh server definitions
        server_discovery.refresh_if_needed().await?;
        
        info!("[PrefixCache] Resolving prefixes for space {} (startup)", space_id);
        
        // Get all installed servers for this space
        let mut servers = installed_server_repo.list_for_space(space_id).await?;
        
        // Sort by created_at (earliest first)
        // TODO: Add verified status priority when registry supports it
        servers.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        
        // Clear existing cache for this space
        self.clear_space(space_id).await;
        
        // Assign prefixes in priority order
        let mut caches = self.caches.write().await;
        let cache = caches.entry(space_id.to_string()).or_insert_with(SpacePrefixCache::new);
        
        for server in servers {
            // Skip disabled servers
            if !server.enabled {
                debug!(
                    "[PrefixCache] Skipping disabled server {} in space {}",
                    server.server_id, space_id
                );
                continue;
            }
            
            // Get desired alias from server discovery
            let desired_alias = server_discovery.get(&server.server_id).await
                .and_then(|s| s.alias.clone());
            
            // Try to assign alias, fallback to server_id if taken
            let prefix = if let Some(ref alias) = desired_alias {
                if cache.is_prefix_available(alias) {
                    info!(
                        "[PrefixCache] Startup: Assigned alias '{}' to server {} (priority)",
                        alias, server.server_id
                    );
                    alias.clone()
                } else {
                    // Alias taken by higher priority server
                    let fallback = self.normalize_server_id(&server.server_id);
                    info!(
                        "[PrefixCache] Startup: Alias '{}' taken, using fallback '{}' for server {}",
                        alias, fallback, server.server_id
                    );
                    fallback
                }
            } else {
                // No alias defined, use server_id
                self.normalize_server_id(&server.server_id)
            };
            
            cache.assign(server.server_id.clone(), prefix);
        }
        
        info!(
            "[PrefixCache] Startup resolution complete for space {}: {} servers processed",
            space_id,
            cache.server_to_prefix.len()
        );
        
        Ok(())
    }
    
    /// Get the prefix for a server (for tools/list)
    /// Returns the assigned prefix, or generates fallback if not found
    pub async fn get_prefix_for_server(&self, space_id: &str, server_id: &str) -> String {
        let caches = self.caches.read().await;
        
        if let Some(cache) = caches.get(space_id) {
            if let Some(prefix) = cache.get_prefix(server_id) {
                return prefix.to_string();
            }
        }
        
        // Fallback: server_id with / -> .
        self.normalize_server_id(server_id)
    }
    
    /// Get the server for a prefix (for tools/call routing)
    pub async fn get_server_for_prefix(&self, space_id: &str, prefix: &str) -> Option<String> {
        let caches = self.caches.read().await;
        
        caches.get(space_id)
            .and_then(|cache| cache.get_server(prefix))
            .map(|s| s.to_string())
    }
    
    /// Check if a prefix is available in a space
    pub async fn is_prefix_available(&self, space_id: &str, prefix: &str) -> bool {
        let caches = self.caches.read().await;
        
        caches.get(space_id)
            .map(|cache| cache.is_prefix_available(prefix))
            .unwrap_or(true) // If no cache for space, all prefixes available
    }
    
    /// Assign a prefix to a server (runtime only - no stealing)
    /// Returns the actual prefix assigned (may be fallback if desired was taken)
    pub async fn assign_prefix_runtime(
        &self,
        space_id: &str,
        server_id: &str,
        desired_alias: Option<&str>,
    ) -> String {
        let mut caches = self.caches.write().await;
        let cache = caches.entry(space_id.to_string()).or_insert_with(SpacePrefixCache::new);
        
        // Check if we already have a prefix (idempotent)
        if let Some(existing) = cache.get_prefix(server_id) {
            debug!(
                "[PrefixCache] Server {} already has prefix '{}' in space {}",
                server_id, existing, space_id
            );
            return existing.to_string();
        }
        
        // Try to use alias if available
        let prefix = if let Some(alias) = desired_alias {
            if cache.is_prefix_available(alias) {
                info!(
                    "[PrefixCache] Assigning alias '{}' to server {} in space {}",
                    alias, server_id, space_id
                );
                alias.to_string()
            } else {
                // Alias taken, use fallback
                let fallback = self.normalize_server_id(server_id);
                warn!(
                    "[PrefixCache] Alias '{}' already taken in space {}, using fallback '{}' for server {}",
                    alias, space_id, fallback, server_id
                );
                fallback
            }
        } else {
            // No alias, use server_id
            self.normalize_server_id(server_id)
        };
        
        cache.assign(server_id.to_string(), prefix.clone());
        prefix
    }
    
    /// Assign prefix for a server, fetching alias from server discovery internally
    /// This is the recommended method for runtime prefix assignment.
    /// Returns the actual prefix assigned.
    pub async fn assign_prefix_for_server(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> String {
        // Fetch alias from server discovery if available
        let desired_alias = if let Some(ref discovery) = self.server_discovery {
            discovery.get(server_id).await.and_then(|s| s.alias.clone())
        } else {
            None
        };
        
        // Delegate to existing assign_prefix_runtime
        self.assign_prefix_runtime(space_id, server_id, desired_alias.as_deref()).await
    }
    
    /// Release a server's prefix (runtime only - no reassignment)
    pub async fn release_prefix_runtime(&self, space_id: &str, server_id: &str) {
        let mut caches = self.caches.write().await;
        
        if let Some(cache) = caches.get_mut(space_id) {
            if let Some(prefix) = cache.remove(server_id) {
                info!(
                    "[PrefixCache] Released prefix '{}' from server {} in space {}",
                    prefix, server_id, space_id
                );
            }
        }
    }
    
    /// Clear cache for a space (used during startup resolution)
    pub async fn clear_space(&self, space_id: &str) {
        let mut caches = self.caches.write().await;
        
        if let Some(cache) = caches.get_mut(space_id) {
            cache.clear();
            debug!("[PrefixCache] Cleared cache for space {}", space_id);
        }
    }
    
    /// Normalize server_id to be MCP-compliant (replace / with .)
    fn normalize_server_id(&self, server_id: &str) -> String {
        server_id.replace('/', ".")
    }
    
    /// Resolve a qualified name into (server_id, feature_name)
    ///
    /// Qualified format: prefix_feature_name (underscore is the ONLY delimiter)
    /// Prefixes must never contain underscores - they use hyphens/alphanumeric only.
    /// Returns None if format is invalid (no underscore).
    /// Returns (server_id, feature_name) - where server_id is resolved from prefix
    pub async fn resolve_qualified_name(&self, space_id: &str, qualified_name: &str) -> Option<(String, String)> {
        // Split on first underscore - this is unambiguous because prefixes cannot contain underscores
        let (prefix, feature_name) = qualified_name.split_once('_')?;
        
        // Resolve prefix to server_id
        let server_id = self.get_server_for_prefix(space_id, prefix)
            .await
            .unwrap_or_else(|| prefix.to_string());
            
        Some((server_id, feature_name.to_string()))
    }
}

impl Default for PrefixCacheService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_assign_and_lookup() {
        let service = PrefixCacheService::new();
        let space_id = "test-space";
        let server_id = "com.cloudflare/docs";
        
        // Assign with alias
        let prefix = service.assign_prefix_runtime(space_id, server_id, Some("cf")).await;
        assert_eq!(prefix, "cf");
        
        // Lookup forward
        let result = service.get_prefix_for_server(space_id, server_id).await;
        assert_eq!(result, "cf");
        
        // Lookup reverse
        let result = service.get_server_for_prefix(space_id, "cf").await;
        assert_eq!(result, Some(server_id.to_string()));
    }
    
    #[tokio::test]
    async fn test_conflict_uses_fallback() {
        let service = PrefixCacheService::new();
        let space_id = "test-space";
        
        // First server gets the alias
        let prefix1 = service.assign_prefix_runtime(space_id, "server-a", Some("api")).await;
        assert_eq!(prefix1, "api");
        
        // Second server with same alias gets fallback
        let prefix2 = service.assign_prefix_runtime(space_id, "server-b", Some("api")).await;
        assert_eq!(prefix2, "server-b"); // Uses normalized server_id
    }
    
    #[tokio::test]
    async fn test_release_prefix() {
        let service = PrefixCacheService::new();
        let space_id = "test-space";
        let server_id = "com.cloudflare/docs";
        
        // Assign
        service.assign_prefix_runtime(space_id, server_id, Some("cf")).await;
        
        // Verify assigned
        assert_eq!(service.get_prefix_for_server(space_id, server_id).await, "cf");
        
        // Release
        service.release_prefix_runtime(space_id, server_id).await;
        
        // Verify released (falls back to normalized server_id)
        assert_eq!(service.get_prefix_for_server(space_id, server_id).await, "com.cloudflare.docs");
        
        // Prefix should be available again
        assert!(service.is_prefix_available(space_id, "cf").await);
    }
    
    #[tokio::test]
    async fn test_resolve_qualified_name() {
        let service = PrefixCacheService::new();
        let space_id = "test-space";
        
        // Server with hyphenated prefix (no underscores!)
        let server_id = "azure-mcp-server";
        service.assign_prefix_runtime(space_id, server_id, Some("azuremcp")).await;
        
        // Resolve "azuremcp_group_list" - underscore separates prefix from feature
        let result = service.resolve_qualified_name(space_id, "azuremcp_group_list").await;
        assert_eq!(result, Some((server_id.to_string(), "group_list".to_string())));
        
        // Resolve "azuremcp_documentation" 
        let result = service.resolve_qualified_name(space_id, "azuremcp_documentation").await;
        assert_eq!(result, Some((server_id.to_string(), "documentation".to_string())));
    }
    
    #[tokio::test]
    async fn test_resolve_simple_prefix() {
        let service = PrefixCacheService::new();
        let space_id = "test-space";
        
        // Server with simple single-word prefix
        let server_id = "github-server";
        service.assign_prefix_runtime(space_id, server_id, Some("gh")).await;
        
        // Resolve "gh_get_me" should find "gh" as prefix
        let result = service.resolve_qualified_name(space_id, "gh_get_me").await;
        assert_eq!(result, Some((server_id.to_string(), "get_me".to_string())));
    }
}

