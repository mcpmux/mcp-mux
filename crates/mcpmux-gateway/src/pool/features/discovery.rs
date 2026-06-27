//! Feature Discovery Service - SRP: Discovery & caching

use anyhow::Result;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use super::{convert_to_feature, resource_to_feature, CachedFeatures};
use crate::pool::instance::McpClient;
use mcpmux_core::ServerFeatureRepository;

/// Handles feature discovery and caching from MCP clients
pub struct FeatureDiscoveryService {
    feature_repo: Arc<dyn ServerFeatureRepository>,
}

impl FeatureDiscoveryService {
    const LIST_TIMEOUT: Duration = Duration::from_secs(10);

    pub fn new(feature_repo: Arc<dyn ServerFeatureRepository>) -> Self {
        Self { feature_repo }
    }

    async fn with_list_timeout<T, E, F>(label: &str, fut: F) -> Option<Result<T, E>>
    where
        F: Future<Output = Result<T, E>>,
    {
        match tokio::time::timeout(Self::LIST_TIMEOUT, fut).await {
            Ok(result) => Some(result),
            Err(_) => {
                warn!(
                    "[FeatureDiscovery] {} timed out after {:?}",
                    label,
                    Self::LIST_TIMEOUT
                );
                None
            }
        }
    }

    /// Discover features from a connected MCP client and cache them
    pub async fn discover_and_cache(
        &self,
        space_id: &str,
        server_id: &str,
        client: &McpClient,
    ) -> Result<CachedFeatures> {
        info!(
            "[FeatureDiscovery] Discovering features for {}/{}",
            space_id, server_id
        );

        let mut discovered = CachedFeatures::default();
        let capabilities = client.peer_info().map(|info| info.capabilities.clone());
        let capabilities_known = capabilities.is_some();
        let has_tools = capabilities
            .as_ref()
            .and_then(|c| c.tools.as_ref())
            .is_some();
        let has_prompts = capabilities
            .as_ref()
            .and_then(|c| c.prompts.as_ref())
            .is_some();
        let has_resources = capabilities
            .as_ref()
            .and_then(|c| c.resources.as_ref())
            .is_some();

        debug!(
            "[FeatureDiscovery] Capability gates for {}/{}: known={}, tools={}, prompts={}, resources={}",
            space_id, server_id, capabilities_known, has_tools, has_prompts, has_resources
        );

        if !capabilities_known || has_tools {
            match Self::with_list_timeout("tools/list", client.list_all_tools()).await {
                Some(Ok(tools)) => {
                    discovered.tools = tools
                        .into_iter()
                        .map(|t| convert_to_feature(space_id, server_id, t))
                        .collect();
                    debug!(
                        "[FeatureDiscovery] Discovered {} tools",
                        discovered.tools.len()
                    );
                }
                Some(Err(e)) => warn!("[FeatureDiscovery] Failed to list tools: {}", e),
                None => {}
            }
        } else {
            debug!(
                "[FeatureDiscovery] Skipping tools/list: server explicitly did not advertise tools capability"
            );
        }

        if !capabilities_known || has_prompts {
            match Self::with_list_timeout("prompts/list", client.list_all_prompts()).await {
                Some(Ok(prompts)) => {
                    discovered.prompts = prompts
                        .into_iter()
                        .map(|p| convert_to_feature(space_id, server_id, p))
                        .collect();
                    debug!(
                        "[FeatureDiscovery] Discovered {} prompts",
                        discovered.prompts.len()
                    );
                }
                Some(Err(e)) => warn!("[FeatureDiscovery] Failed to list prompts: {}", e),
                None => {}
            }
        } else {
            debug!("[FeatureDiscovery] Skipping prompts/list: server explicitly did not advertise prompts capability");
        }

        if !capabilities_known || has_resources {
            match Self::with_list_timeout("resources/list", client.list_all_resources()).await {
                Some(Ok(resources)) => {
                    discovered.resources = resources
                        .into_iter()
                        .map(|r| resource_to_feature(space_id, server_id, r))
                        .collect();
                    debug!(
                        "[FeatureDiscovery] Discovered {} resources",
                        discovered.resources.len()
                    );
                }
                Some(Err(e)) => warn!("[FeatureDiscovery] Failed to list resources: {}", e),
                None => {}
            }
        } else {
            debug!("[FeatureDiscovery] Skipping resources/list: server explicitly did not advertise resources capability");
        }

        // Cache all features in database
        let all_features = discovered.all_features();
        if !all_features.is_empty() {
            if let Err(e) = self.feature_repo.upsert_many(&all_features).await {
                warn!("[FeatureDiscovery] Failed to cache features: {}", e);
            } else {
                info!(
                    "[FeatureDiscovery] Cached {} features for {}/{}",
                    all_features.len(),
                    space_id,
                    server_id
                );
            }
        }

        Ok(discovered)
    }

    /// Mark all features for a server as unavailable (on disconnect)
    pub async fn mark_unavailable(&self, space_id: &str, server_id: &str) -> Result<()> {
        self.feature_repo
            .mark_unavailable(space_id, server_id)
            .await
    }

    /// Delete all features for a server (on uninstall)
    pub async fn delete_for_server(&self, space_id: &str, server_id: &str) -> Result<()> {
        self.feature_repo
            .delete_for_server(space_id, server_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;

    #[tokio::test]
    async fn with_list_timeout_returns_some_ok_when_future_completes() {
        let out =
            FeatureDiscoveryService::with_list_timeout("tools/list", async { Ok::<i32, &str>(42) })
                .await;
        assert!(matches!(out, Some(Ok(42))));
    }

    #[tokio::test]
    async fn with_list_timeout_propagates_inner_error() {
        let out = FeatureDiscoveryService::with_list_timeout("prompts/list", async {
            Err::<i32, &str>("boom")
        })
        .await;
        assert!(matches!(out, Some(Err("boom"))));
    }

    #[tokio::test(start_paused = true)]
    async fn with_list_timeout_returns_none_on_timeout() {
        // A future that never resolves. Under tokio's paused clock the runtime
        // auto-advances to the LIST_TIMEOUT deadline, so this resolves to a
        // timeout without actually waiting 10 seconds.
        let never = pending::<Result<i32, &str>>();
        let out = FeatureDiscoveryService::with_list_timeout("resources/list", never).await;
        assert!(out.is_none());
    }
}
