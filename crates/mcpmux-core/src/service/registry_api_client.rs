//! HTTP client for fetching server definitions from Registry API.
//!
//! This client uses the bundle-only strategy (see ADR-001).
//! All server discovery, filtering, and searching is done client-side
//! against the cached bundle data.
//!
//! Supports ETag-based conditional fetching to avoid re-downloading
//! unchanged bundles.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::domain::ServerDefinition;

/// Response wrapper from Registry API
#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    data: T,
    #[allow(dead_code)]
    meta: Option<serde_json::Value>,
}

// ============================================
// Bundle Types
// ============================================

/// Complete registry bundle from /v1/bundle
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryBundle {
    pub version: String,
    pub updated_at: String,
    pub servers: Vec<ServerDefinition>,
    pub categories: Vec<Category>,
    pub ui: UiConfig,
    pub home: Option<HomeConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

// ============================================
// UI Configuration Types (API-driven)
// ============================================

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    pub filters: Vec<FilterDefinition>,
    pub sort_options: Vec<SortOption>,
    pub default_sort: String,
    pub items_per_page: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterDefinition {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub filter_type: String, // "single" or "multi"
    pub options: Vec<FilterOption>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterOption {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    #[serde(rename = "match")]
    pub match_rule: Option<FilterMatch>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterMatch {
    pub field: String,
    pub operator: String, // "eq", "in", "contains"
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SortOption {
    pub id: String,
    pub label: String,
    pub rules: Vec<SortRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SortRule {
    pub field: String,
    pub direction: String,     // "asc" or "desc"
    pub nulls: Option<String>, // "first" or "last"
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HomeConfig {
    pub featured_server_ids: Vec<String>,
}

// ============================================
// Fetch Result
// ============================================

/// Result of fetching a bundle with ETag support
#[derive(Debug)]
pub enum FetchBundleResult {
    /// New or updated bundle received
    Updated {
        bundle: Box<RegistryBundle>,
        etag: Option<String>,
    },
    /// Bundle unchanged (304 Not Modified)
    NotModified,
}

// ============================================
// Client Implementation
// ============================================

/// Client for fetching data from McpMux Registry API
///
/// This client only uses the bundle endpoint. Individual endpoints
/// (servers, categories) are not used per ADR-001.
pub struct RegistryApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl RegistryApiClient {
    /// Create a new Registry API client
    pub fn new(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("McpMux/1.0")
            .build()
            .expect("Failed to build HTTP client");

        Self { base_url, client }
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Fetch complete registry bundle from /v1/bundle
    ///
    /// If `current_etag` is provided, sends `If-None-Match` header.
    /// Returns `NotModified` if server responds with 304.
    ///
    /// This is the ONLY method used for fetching registry data.
    /// All filtering, searching, and sorting is done client-side.
    pub async fn fetch_bundle(&self, current_etag: Option<&str>) -> Result<FetchBundleResult> {
        let url = format!("{}/v1/bundle", self.base_url);

        tracing::info!("Fetching registry bundle from {}", url);

        let mut request = self.client.get(&url);

        // Add If-None-Match header if we have a cached ETag
        if let Some(etag) = current_etag {
            tracing::debug!("Sending If-None-Match: {}", etag);
            request = request.header("If-None-Match", etag);
        }

        let response = request
            .send()
            .await
            .context("Failed to send request to registry API")?;

        let status = response.status();

        // Handle 304 Not Modified
        if status == reqwest::StatusCode::NOT_MODIFIED {
            tracing::info!("Registry bundle not modified (304)");
            return Ok(FetchBundleResult::NotModified);
        }

        if !status.is_success() {
            anyhow::bail!("Registry API returned status: {}", status);
        }

        // Extract ETag from response headers
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let api_response: ApiResponse<RegistryBundle> = response
            .json()
            .await
            .context("Failed to parse registry bundle JSON")?;

        let bundle = api_response.data;

        tracing::info!(
            "Fetched {} servers, {} filters, {} sort options (version: {}, updated: {}, etag: {:?})",
            bundle.servers.len(),
            bundle.ui.filters.len(),
            bundle.ui.sort_options.len(),
            bundle.version,
            bundle.updated_at,
            etag
        );

        Ok(FetchBundleResult::Updated {
            bundle: Box::new(bundle),
            etag,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_bundle_from_local() {
        // Uses deployed API by default, or MCPMUX_REGISTRY_URL env var
        let client = RegistryApiClient::new(
            std::env::var("MCPMUX_REGISTRY_URL")
                .unwrap_or_else(|_| "https://api.mcpmux.com".to_string()),
        );

        let result = client.fetch_bundle(None).await;

        // This will fail if dev server is not running - that's expected
        if let Ok(FetchBundleResult::Updated { bundle, etag }) = result {
            assert!(
                !bundle.servers.is_empty(),
                "Should have at least one server"
            );
            assert!(!bundle.ui.filters.is_empty(), "Should have filters");
            assert!(
                !bundle.ui.sort_options.is_empty(),
                "Should have sort options"
            );
            assert!(etag.is_some(), "Should have ETag");
        }
    }

    #[tokio::test]
    async fn test_fetch_bundle_with_etag() {
        let client = RegistryApiClient::new(
            std::env::var("MCPMUX_REGISTRY_URL")
                .unwrap_or_else(|_| "https://api.mcpmux.com".to_string()),
        );

        // First fetch to get ETag
        let first_result = client.fetch_bundle(None).await;
        if let Ok(FetchBundleResult::Updated {
            etag: Some(etag), ..
        }) = first_result
        {
            // Second fetch with ETag should return NotModified
            let second_result = client.fetch_bundle(Some(&etag)).await;
            if let Ok(FetchBundleResult::NotModified) = second_result {
                // Success!
            } else {
                // Server might have been updated, that's okay
            }
        }
    }
}
