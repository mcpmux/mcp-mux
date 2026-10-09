//! Feature Discovery Service - SRP: Discovery & caching

use anyhow::Result;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use super::{convert_to_feature, resource_to_feature, CachedFeatures};
use crate::pool::instance::McpClient;
use mcpmux_core::{ServerFeature, ServerFeatureRepository};
use rmcp::model::PaginatedRequestParams;
use rmcp::service::ServiceError;

/// Most tools, prompts or resources (each) kept from one server. Real servers
/// expose at most a few hundred; the cap keeps a misbehaving one from
/// filling memory, the database and every client's tool list.
pub const MAX_FEATURES_PER_KIND: usize = 2000;
/// Most list pages requested per kind (a server that keeps returning a
/// cursor would otherwise be paged until the timeout).
const MAX_LIST_PAGES: usize = 100;
/// Longest feature name kept; features with longer names are skipped.
const MAX_NAME_LEN: usize = 512;
/// Longest description kept; longer ones are truncated.
const MAX_DESCRIPTION_LEN: usize = 16 * 1024;
/// Largest serialized feature definition (input schema included) kept;
/// larger ones are skipped.
const MAX_DEFINITION_BYTES: usize = 256 * 1024;

/// Page through a list endpoint, stopping at [`MAX_FEATURES_PER_KIND`]
/// items or [`MAX_LIST_PAGES`] pages.
async fn list_capped<T, F, Fut>(label: &str, mut page: F) -> Result<Vec<T>, ServiceError>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<(Vec<T>, Option<String>), ServiceError>>,
{
    let mut items = Vec::new();
    let mut cursor = None;
    for _ in 0..MAX_LIST_PAGES {
        let (batch, next) = page(cursor).await?;
        items.extend(batch);
        if items.len() >= MAX_FEATURES_PER_KIND {
            warn!(
                "[FeatureDiscovery] {} returned more than {} items; keeping the first {}",
                label, MAX_FEATURES_PER_KIND, MAX_FEATURES_PER_KIND
            );
            items.truncate(MAX_FEATURES_PER_KIND);
            return Ok(items);
        }
        match next {
            Some(next) => cursor = Some(next),
            None => return Ok(items),
        }
    }
    warn!(
        "[FeatureDiscovery] {} still had more pages after {}; stopping",
        label, MAX_LIST_PAGES
    );
    Ok(items)
}

/// Cut a description longer than [`MAX_DESCRIPTION_LEN`] on a char boundary
/// and mark it with "…".
fn shorten_description(desc: &mut String) {
    if desc.len() > MAX_DESCRIPTION_LEN {
        let mut cut = MAX_DESCRIPTION_LEN;
        while !desc.is_char_boundary(cut) {
            cut -= 1;
        }
        desc.truncate(cut);
        desc.push('…');
    }
}

/// Keep a discovered feature only within the size limits: names and whole
/// definitions above their caps are dropped, long descriptions shortened.
fn within_limits(mut feature: ServerFeature) -> Option<ServerFeature> {
    if feature.feature_name.len() > MAX_NAME_LEN {
        warn!(
            "[FeatureDiscovery] Skipping {:?} with a {}-byte name",
            feature.feature_type,
            feature.feature_name.len()
        );
        return None;
    }
    if let Some(desc) = feature.description.as_mut() {
        shorten_description(desc);
    }
    // Clients are served the stored definition, not the description field:
    // shorten it there too.
    if let Some(desc) = feature
        .raw_json
        .as_mut()
        .and_then(|json| json.get_mut("description"))
        .and_then(|d| match d {
            serde_json::Value::String(s) => Some(s),
            _ => None,
        })
    {
        shorten_description(desc);
    }
    let definition_len = feature
        .raw_json
        .as_ref()
        .and_then(|json| serde_json::to_vec(json).ok())
        .map_or(0, |bytes| bytes.len());
    if definition_len > MAX_DEFINITION_BYTES {
        warn!(
            "[FeatureDiscovery] Skipping {} {}: {}-byte definition",
            feature.feature_type.as_str(),
            feature.feature_name,
            definition_len
        );
        return None;
    }
    Some(feature)
}

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
            let list = list_capped("tools/list", |cursor| async move {
                client
                    .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                    .await
                    .map(|r| (r.tools, r.next_cursor))
            });
            match Self::with_list_timeout("tools/list", list).await {
                Some(Ok(tools)) => {
                    discovered.tools = tools
                        .into_iter()
                        .map(|t| convert_to_feature(space_id, server_id, t))
                        .filter_map(within_limits)
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
            let list = list_capped("prompts/list", |cursor| async move {
                client
                    .list_prompts(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                    .await
                    .map(|r| (r.prompts, r.next_cursor))
            });
            match Self::with_list_timeout("prompts/list", list).await {
                Some(Ok(prompts)) => {
                    discovered.prompts = prompts
                        .into_iter()
                        .map(|p| convert_to_feature(space_id, server_id, p))
                        .filter_map(within_limits)
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
            let list = list_capped("resources/list", |cursor| async move {
                client
                    .list_resources(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                    .await
                    .map(|r| (r.resources, r.next_cursor))
            });
            match Self::with_list_timeout("resources/list", list).await {
                Some(Ok(resources)) => {
                    discovered.resources = resources
                        .into_iter()
                        .map(|r| resource_to_feature(space_id, server_id, r))
                        .filter_map(within_limits)
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

    #[tokio::test]
    async fn listing_stops_at_the_item_cap() {
        // A server that always has another page of 500 items.
        let items = super::list_capped("tools/list", |_cursor| async {
            Ok::<_, rmcp::service::ServiceError>((vec![0u8; 500], Some("more".to_string())))
        })
        .await
        .unwrap();
        assert_eq!(items.len(), super::MAX_FEATURES_PER_KIND);
    }

    #[tokio::test]
    async fn listing_stops_at_the_page_cap() {
        let mut calls = 0;
        let items = super::list_capped("tools/list", |_cursor| {
            calls += 1;
            async {
                Ok::<_, rmcp::service::ServiceError>((Vec::<u8>::new(), Some("again".to_string())))
            }
        })
        .await
        .unwrap();
        assert!(items.is_empty());
        assert_eq!(calls, super::MAX_LIST_PAGES);
    }

    #[test]
    fn oversized_features_are_skipped_or_shortened() {
        use mcpmux_core::ServerFeature;
        let ok = ServerFeature::tool("s", "srv", "fine");
        assert!(super::within_limits(ok).is_some());

        let long_name = ServerFeature::tool("s", "srv", &"n".repeat(super::MAX_NAME_LEN + 1));
        assert!(super::within_limits(long_name).is_none());

        let long = "é".repeat(super::MAX_DESCRIPTION_LEN);
        let long_desc = ServerFeature::tool("s", "srv", "t")
            .with_description(long.clone())
            .with_raw_json(serde_json::json!({"name": "t", "description": long}));
        let kept = super::within_limits(long_desc).unwrap();
        assert!(kept.description.unwrap().len() <= super::MAX_DESCRIPTION_LEN + '…'.len_utf8());
        // What tools/list serves is shortened too.
        let served = kept.raw_json.unwrap()["description"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(served.len() <= super::MAX_DESCRIPTION_LEN + '…'.len_utf8());
        assert!(served.ends_with('…'));

        let big_schema = ServerFeature::tool("s", "srv", "t").with_raw_json(serde_json::json!({
            "inputSchema": {"description": "x".repeat(super::MAX_DEFINITION_BYTES)}
        }));
        assert!(super::within_limits(big_schema).is_none());
    }

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
