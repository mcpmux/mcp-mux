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
#[serde(from = "RawRegistryBundle")]
pub struct RegistryBundle {
    pub version: String,
    pub updated_at: String,
    pub servers: Vec<ServerDefinition>,
    pub categories: Vec<Category>,
    pub ui: UiConfig,
    pub home: Option<HomeConfig>,
    /// Servers dropped while parsing because this client couldn't read them.
    /// Not serialized, so a disk-cached bundle reads back as 0.
    #[serde(skip)]
    pub skipped_servers: usize,
}

/// Wire form of [`RegistryBundle`], with each server left as raw JSON.
///
/// Servers are parsed one by one and any that fail are skipped. The registry
/// can publish schema values this client predates (a new auth type, badge,
/// transport, ...); one such server must not take down discovery for the
/// whole bundle.
#[derive(Deserialize)]
struct RawRegistryBundle {
    version: String,
    updated_at: String,
    servers: Vec<serde_json::Value>,
    categories: Vec<Category>,
    ui: UiConfig,
    home: Option<HomeConfig>,
}

impl From<RawRegistryBundle> for RegistryBundle {
    fn from(raw: RawRegistryBundle) -> Self {
        let total = raw.servers.len();
        let servers: Vec<ServerDefinition> = raw
            .servers
            .into_iter()
            .filter_map(|value| {
                let id = value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing id>")
                    .to_string();
                match serde_json::from_value::<ServerDefinition>(value) {
                    Ok(server) => Some(server),
                    Err(e) => {
                        tracing::warn!("Skipping registry server '{}': {}", id, e);
                        None
                    }
                }
            })
            .collect();

        Self {
            version: raw.version,
            updated_at: raw.updated_at,
            skipped_servers: total - servers.len(),
            servers,
            categories: raw.categories,
            ui: raw.ui,
            home: raw.home,
        }
    }
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

        let body = read_capped(response, MAX_BUNDLE_BYTES)
            .await
            .context("Failed to read registry bundle response")?;
        let bundle = parse_bundle(&body)?;

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

/// Largest registry bundle accepted. The real bundle is far smaller; the
/// cap keeps a broken or hostile registry from exhausting memory.
const MAX_BUNDLE_BYTES: usize = 32 * 1024 * 1024;

/// Read a response body, failing once it exceeds `max` bytes.
async fn read_capped(mut response: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|len| len > max as u64)
    {
        anyhow::bail!("registry bundle is larger than {max} bytes");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > max {
            anyhow::bail!("registry bundle is larger than {max} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Parse a `/v1/bundle` response body.
fn parse_bundle(body: &[u8]) -> Result<RegistryBundle> {
    let api_response: ApiResponse<RegistryBundle> =
        serde_json::from_slice(body).context("Failed to parse registry bundle JSON")?;
    Ok(api_response.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body without Content-Length is still cut off once it passes the
    /// cap, chunk by chunk, instead of being read to the end.
    #[tokio::test]
    async fn a_body_without_a_length_is_capped_while_reading() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/bundle", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            // No Content-Length: the body runs until the connection closes.
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n")
                .await;
            for _ in 0..64 {
                if stream.write_all(&[b'x'; 1024]).await.is_err() {
                    break;
                }
            }
            let _ = stream.shutdown().await;
        });

        let response = reqwest::get(&url).await.unwrap();
        assert!(response.content_length().is_none());
        let err = read_capped(response, 4096).await.unwrap_err();
        assert!(err.to_string().contains("larger than 4096 bytes"), "{err}");
    }

    /// Regression test: one server the client can't parse (here an unknown
    /// auth type) must be skipped, not fail the whole bundle.
    #[test]
    fn test_parse_bundle_skips_unparseable_servers() {
        let body = br#"{
            "data": {
                "version": "2.1.0",
                "updated_at": "2026-10-01T00:00:00Z",
                "servers": [
                    {
                        "id": "good",
                        "name": "Good",
                        "transport": { "type": "stdio", "command": "npx" },
                        "auth": { "type": "basic", "instructions": "user/pass" }
                    },
                    {
                        "id": "future",
                        "name": "Future",
                        "transport": { "type": "stdio", "command": "npx" },
                        "auth": { "type": "some_future_auth" }
                    }
                ],
                "categories": [],
                "ui": {
                    "filters": [],
                    "sort_options": [],
                    "default_sort": "name",
                    "items_per_page": 20
                }
            }
        }"#;

        let bundle = parse_bundle(body).expect("bundle should parse");

        let ids: Vec<&str> = bundle.servers.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["good"]);
        assert_eq!(bundle.skipped_servers, 1);

        // The disk cache stores the serialized bundle; the skip count is
        // runtime-only and must not leak into it.
        let cached = serde_json::to_value(&bundle).unwrap();
        assert!(cached.get("skipped_servers").is_none());
        let reloaded: RegistryBundle = serde_json::from_value(cached).unwrap();
        assert_eq!(reloaded.servers.len(), 1);
        assert_eq!(reloaded.skipped_servers, 0);
    }

    #[test]
    fn test_parse_bundle_rejects_malformed_envelope() {
        assert!(parse_bundle(br#"{"data": {"servers": "nope"}}"#).is_err());
    }

    #[tokio::test]
    async fn test_fetch_bundle_from_local() {
        // Uses deployed API by default, or MCPMUX_REGISTRY_URL env var
        let client = RegistryApiClient::new(
            std::env::var("MCPMUX_REGISTRY_URL")
                .unwrap_or_else(|_| "https://api.mcpmux.com".to_string()),
        );

        let result = client.fetch_bundle(None).await;

        // An unreachable registry is tolerated; a bundle we can't parse is not.
        let (bundle, etag) = match result {
            Ok(FetchBundleResult::Updated { bundle, etag }) => (bundle, etag),
            Ok(FetchBundleResult::NotModified) => panic!("no ETag was sent, expected a bundle"),
            Err(e) if e.chain().any(|c| c.is::<serde_json::Error>()) => {
                panic!("registry bundle failed to parse: {:#}", e)
            }
            Err(e) => {
                eprintln!("skipping: registry unreachable: {:#}", e);
                return;
            }
        };

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
