//! CIMD (Client ID Metadata Document) fetcher
//! 
//! Handles HTTP fetching of client metadata from URLs per the OAuth Client ID
//! Metadata Document specification (draft).

use anyhow::Result;
use serde::Deserialize;
use tracing::info;

/// Client metadata from CIMD document
#[derive(Debug, Clone, Deserialize)]
pub struct CimdMetadata {
    pub client_id: String,
    pub client_name: String,
    pub logo_uri: Option<String>,
    pub client_uri: Option<String>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub redirect_uris: Vec<String>,
    pub grant_types: Option<Vec<String>>,
    pub response_types: Option<Vec<String>>,
    pub token_endpoint_auth_method: Option<String>,
    pub scope: Option<String>,
}

/// Fetches client metadata from CIMD URLs
/// 
/// Single responsibility: HTTP operations only, no persistence
pub struct CimdMetadataFetcher {
    http_client: reqwest::Client,
}

impl CimdMetadataFetcher {
    /// Create a new CIMD fetcher with default HTTP client
    pub fn new() -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        
        Ok(Self { http_client })
    }

    /// Create with a custom HTTP client (useful for testing)
    pub fn with_client(http_client: reqwest::Client) -> Self {
        Self { http_client }
    }

    /// Fetch metadata from a CIMD URL
    /// 
    /// Returns the parsed metadata or an error if fetching fails
    pub async fn fetch(&self, client_id_url: &str) -> Result<CimdMetadata> {
        info!("[CIMD] Fetching client metadata from: {}", client_id_url);
        
        let response = self.http_client
            .get(client_id_url)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to fetch CIMD metadata: HTTP {}",
                response.status()
            );
        }

        let metadata: CimdMetadata = response.json().await?;

        // Validate that client_id in metadata matches the URL
        if metadata.client_id != client_id_url {
            anyhow::bail!(
                "CIMD client_id mismatch: URL='{}', metadata.client_id='{}'",
                client_id_url,
                metadata.client_id
            );
        }

        info!("[CIMD] Successfully fetched metadata for: {}", client_id_url);
        Ok(metadata)
    }

    /// Check if a string looks like a CIMD URL
    pub fn is_cimd_url(client_id: &str) -> bool {
        client_id.starts_with("https://") || client_id.starts_with("http://")
    }
}

impl Default for CimdMetadataFetcher {
    fn default() -> Self {
        Self::new().expect("Failed to create default CIMD fetcher")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_cimd_url() {
        assert!(CimdMetadataFetcher::is_cimd_url("https://example.com/client.json"));
        assert!(CimdMetadataFetcher::is_cimd_url("http://localhost:3000/client"));
        assert!(!CimdMetadataFetcher::is_cimd_url("mcp_abc123"));
        assert!(!CimdMetadataFetcher::is_cimd_url("client-name"));
    }
}

