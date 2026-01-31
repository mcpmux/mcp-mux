//! Client Metadata Service
//! 
//! Orchestrates client resolution, including CIMD fetching, caching, and persistence.
//! This service follows SOLID principles by separating concerns:
//! - CIMD fetching (CimdMetadataFetcher from mcpmux-core)
//! - Persistence (InboundClientRepository from mcpmux-storage)
//! - Business logic (this service)

use anyhow::Result;
use mcpmux_core::CimdMetadataFetcher;
use mcpmux_storage::{InboundClient, InboundClientRepository, RegistrationType};
use std::sync::Arc;
use tracing::{debug, info};

/// Service for resolving and managing client metadata
/// 
/// Handles both traditional clients (DCR, pre-registered) and CIMD clients
pub struct ClientMetadataService {
    repository: Arc<InboundClientRepository>,
    cimd_fetcher: Arc<CimdMetadataFetcher>,
}

impl ClientMetadataService {
    /// Create a new client metadata service
    pub fn new(
        repository: Arc<InboundClientRepository>,
        cimd_fetcher: Arc<CimdMetadataFetcher>,
    ) -> Self {
        Self {
            repository,
            cimd_fetcher,
        }
    }

    /// Resolve a client by ID
    /// 
    /// Determines if the client_id is a CIMD URL or a traditional client_id,
    /// and fetches/retrieves accordingly.
    /// 
    /// For CIMD URLs:
    /// 1. Check cache validity
    /// 2. If stale/missing, fetch from URL
    /// 3. Save to database
    /// 
    /// For traditional client_ids:
    /// 1. Look up in database (DCR or pre-registered)
    pub async fn resolve_client(&self, client_id: &str) -> Result<Option<InboundClient>> {
        if CimdMetadataFetcher::is_cimd_url(client_id) {
            // CIMD flow
            Ok(Some(self.get_or_fetch_cimd_client(client_id).await?))
        } else {
            // Traditional flow (DCR or pre-registered)
            self.repository.get_client(client_id).await
        }
    }

    /// Get or fetch a CIMD client
    /// 
    /// If the client is cached and the cache is valid, returns the cached version.
    /// Otherwise, fetches fresh metadata from the CIMD URL.
    async fn get_or_fetch_cimd_client(&self, client_id_url: &str) -> Result<InboundClient> {
        // Try to load from database
        if let Some(existing) = self.repository.get_client(client_id_url).await? {
            if existing.registration_type == RegistrationType::Cimd
                && self.is_cimd_cache_valid(&existing)
            {
                debug!("[CIMD] Using cached metadata for: {}", client_id_url);
                return Ok(existing);
            }
        }

        // Fetch fresh metadata
        let metadata = self.cimd_fetcher.fetch(client_id_url).await?;
        
        // Convert to InboundClient
        let client = self.cimd_metadata_to_client(metadata);
        
        // Save to database
        self.repository.save_client(&client).await?;
        
        info!("[CIMD] Fetched and cached metadata for: {}", client_id_url);
        Ok(client)
    }

    /// Check if CIMD cache is still valid
    fn is_cimd_cache_valid(&self, client: &InboundClient) -> bool {
        let Some(cached_at_str) = &client.metadata_cached_at else {
            return false;
        };

        let Some(ttl) = client.metadata_cache_ttl else {
            return false;
        };

        let Ok(cached_at) = chrono::DateTime::parse_from_rfc3339(cached_at_str) else {
            return false;
        };

        let now = chrono::Utc::now();
        let age = now.signed_duration_since(cached_at.with_timezone(&chrono::Utc));

        age.num_seconds() < ttl
    }

    /// Convert CIMD metadata to InboundClient
    fn cimd_metadata_to_client(&self, metadata: mcpmux_core::CimdMetadata) -> InboundClient {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        InboundClient {
            client_id: metadata.client_id.clone(),
            registration_type: RegistrationType::Cimd,
            client_name: metadata.client_name,
            client_alias: None,
            redirect_uris: metadata.redirect_uris,
            grant_types: metadata.grant_types.unwrap_or_else(|| {
                vec!["authorization_code".to_string(), "refresh_token".to_string()]
            }),
            response_types: metadata.response_types.unwrap_or_else(|| {
                vec!["code".to_string()]
            }),
            token_endpoint_auth_method: metadata
                .token_endpoint_auth_method
                .unwrap_or_else(|| "none".to_string()),
            scope: metadata.scope,
            // Not approved until user explicitly consents
            approved: false,
            logo_uri: metadata.logo_uri,
            client_uri: metadata.client_uri,
            software_id: metadata.software_id,
            software_version: metadata.software_version,
            metadata_url: Some(metadata.client_id),
            metadata_cached_at: Some(now.clone()),
            metadata_cache_ttl: Some(3600), // 1 hour default
            connection_mode: "follow_active".to_string(),
            locked_space_id: None,
            last_seen: Some(now.clone()),
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

