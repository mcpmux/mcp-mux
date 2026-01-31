//! Service for discovering and loading MCP servers from various sources.
//!
//! This service uses the bundle-only strategy (see ADR-001).
//! All filtering and searching is done client-side against cached data.
//! 
//! Offline support: The bundle is cached to disk after successful fetch,
//! and loaded from disk when the API is unreachable.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::domain::{ServerDefinition, ServerSource, UserSpaceConfig};
use crate::service::registry_api_client::{RegistryApiClient, RegistryBundle, UiConfig, HomeConfig, FetchBundleResult};
use crate::service::app_settings_service::{AppSettingsService, keys};

const BUNDLE_CACHE_FILENAME: &str = "registry-bundle.json";

/// Default UI config used when no bundle is available
fn default_ui_config() -> UiConfig {
    UiConfig {
        filters: vec![],
        sort_options: vec![],
        default_sort: "name_asc".to_string(),
        items_per_page: 24,
    }
}

pub struct ServerDiscoveryService {
    /// In-memory cache of all discovered servers, keyed by ID.
    servers: Arc<RwLock<HashMap<String, ServerDefinition>>>,
    /// Path to user spaces directory (e.g. %LOCALAPPDATA%/mcpmux/spaces)
    spaces_dir: PathBuf,
    /// Path to app data directory (e.g. %LOCALAPPDATA%/mcpmux)
    data_dir: PathBuf,
    /// HTTP client for fetching from Registry API
    registry_client: Option<RegistryApiClient>,
    /// App settings service for persistent storage
    settings_service: Option<Arc<AppSettingsService>>,
    /// Last refresh timestamp
    last_refresh: Arc<RwLock<Option<Instant>>>,
    /// Cached UI configuration from bundle
    ui_config: Arc<RwLock<UiConfig>>,
    /// Cached home configuration from bundle
    home_config: Arc<RwLock<Option<HomeConfig>>>,
    /// Whether currently running from disk cache (offline mode)
    is_offline: Arc<RwLock<bool>>,
    /// Cached ETag from last successful API fetch (in-memory cache)
    cached_etag: Arc<RwLock<Option<String>>>,
}

impl ServerDiscoveryService {
    /// Create a new server discovery service.
    /// 
    /// - `data_dir`: App data directory (e.g. %LOCALAPPDATA%/mcpmux)
    /// - `spaces_dir`: User spaces directory (e.g. %LOCALAPPDATA%/mcpmux/spaces)
    pub fn new(data_dir: PathBuf, spaces_dir: PathBuf) -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            spaces_dir,
            data_dir,
            registry_client: None,
            settings_service: None,
            last_refresh: Arc::new(RwLock::new(None)),
            ui_config: Arc::new(RwLock::new(default_ui_config())),
            home_config: Arc::new(RwLock::new(None)),
            is_offline: Arc::new(RwLock::new(false)),
            cached_etag: Arc::new(RwLock::new(None)),
        }
    }

    /// Create with Registry API client enabled
    pub fn with_registry_api(mut self, base_url: String) -> Self {
        self.registry_client = Some(RegistryApiClient::new(base_url));
        self
    }

    /// Create with App Settings service for persistent ETag storage
    pub fn with_settings_service(mut self, settings: Arc<AppSettingsService>) -> Self {
        self.settings_service = Some(settings);
        self
    }

    /// Check if cache should be refreshed (> 5 minutes old)
    pub async fn should_refresh(&self) -> bool {
        let last = self.last_refresh.read().await;
        match *last {
            Some(time) => time.elapsed() > Duration::from_secs(300), // 5 minutes
            None => true,
        }
    }

    /// Check if running in offline mode (using disk cache)
    pub async fn is_offline(&self) -> bool {
        *self.is_offline.read().await
    }

    // ============================================
    // Bundle Disk Cache
    // ============================================

    /// Get the path to the cached bundle file
    fn bundle_cache_path(&self) -> PathBuf {
        self.data_dir.join("cache").join(BUNDLE_CACHE_FILENAME)
    }

    /// Save bundle to disk for offline use
    async fn save_bundle_to_disk(&self, bundle: &RegistryBundle) -> anyhow::Result<()> {
        // Ensure cache directory exists
        let cache_dir = self.data_dir.join("cache");
        if !cache_dir.exists() {
            tokio::fs::create_dir_all(&cache_dir).await?;
        }

        let path = self.bundle_cache_path();
        let json = serde_json::to_string_pretty(bundle)?;
        tokio::fs::write(&path, json).await?;
        
        info!("Saved registry bundle to disk cache: {}", path.display());
        Ok(())
    }

    /// Load bundle from disk cache
    async fn load_bundle_from_disk(&self) -> Option<RegistryBundle> {
        let path = self.bundle_cache_path();
        
        if !path.exists() {
            return None;
        }

        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                match serde_json::from_str::<RegistryBundle>(&content) {
                    Ok(bundle) => {
                        info!(
                            "Loaded registry bundle from disk cache: {} servers (v{}, updated {})",
                            bundle.servers.len(),
                            bundle.version,
                            bundle.updated_at
                        );
                        Some(bundle)
                    }
                    Err(e) => {
                        warn!("Failed to parse cached bundle: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read cached bundle: {}", e);
                None
            }
        }
    }

    // ============================================
    // ETag Storage (via AppSettings)
    // ============================================

    /// Save ETag to persistent storage
    async fn save_etag(&self, etag: &str) {
        if let Some(ref settings) = self.settings_service {
            if let Err(e) = settings.set_string(keys::registry::BUNDLE_ETAG, etag).await {
                warn!("Failed to save ETag to settings: {}", e);
            }
        }
    }

    /// Load ETag from persistent storage
    async fn load_etag(&self) -> Option<String> {
        if let Some(ref settings) = self.settings_service {
            settings.get_string(keys::registry::BUNDLE_ETAG).await
        } else {
            None
        }
    }

    // ============================================
    // Refresh Logic
    // ============================================

    /// Initialize the service by loading from Registry API (with disk cache fallback) and user spaces.
    /// 
    /// Uses ETag-based conditional fetching to avoid re-downloading unchanged bundles.
    pub async fn refresh(&self) -> anyhow::Result<()> {
        let mut merged_servers = HashMap::new();
        let mut offline_mode = false;

        // Get current ETag (from memory, or load from settings on first run)
        // IMPORTANT: Only use ETag if cache file exists, otherwise force fresh fetch
        let cache_file_exists = self.bundle_cache_path().exists();
        let current_etag = if cache_file_exists {
            let etag = self.cached_etag.read().await;
            if etag.is_some() {
                etag.clone()
            } else {
                drop(etag);
                // Try loading from settings
                let disk_etag = self.load_etag().await;
                if let Some(ref e) = disk_etag {
                    let mut etag_lock = self.cached_etag.write().await;
                    *etag_lock = Some(e.clone());
                }
                disk_etag
            }
        } else {
            // No cache file - don't send ETag (force fresh fetch)
            info!("Cache file missing, forcing fresh fetch (ignoring stored ETag)");
            None
        };

        // 1. Try to load from Registry API first
        let bundle_result = if let Some(ref client) = self.registry_client {
            match client.fetch_bundle(current_etag.as_deref()).await {
                Ok(FetchBundleResult::NotModified) => {
                    // Bundle unchanged - but we still need to ensure memory is populated
                    info!("Registry bundle unchanged (304 Not Modified)");
                    
                    // Check if in-memory cache is empty (e.g., after app restart)
                    let memory_empty = {
                        let servers = self.servers.read().await;
                        servers.is_empty()
                    };
                    
                    if memory_empty {
                        // Load from disk cache to populate memory
                        info!("Memory empty, loading bundle from disk cache");
                        if let Some(cached_bundle) = self.load_bundle_from_disk().await {
                            // Use the cached bundle (don't return early)
                            Some(cached_bundle)
                        } else {
                            warn!("No disk cache available despite 304 response");
                            None
                        }
                    } else {
                        // Memory already has data, just update timestamp
                        let mut last_refresh = self.last_refresh.write().await;
                        *last_refresh = Some(Instant::now());
                        
                        // Still need to reload user spaces in case they changed
                        self.reload_user_spaces().await;
                        
                        return Ok(());
                    }
                }
                Ok(FetchBundleResult::Updated { bundle, etag }) => {
                    info!(
                        "Loaded {} servers from Registry API (v{}, updated {})",
                        bundle.servers.len(),
                        bundle.version,
                        bundle.updated_at
                    );

                    // Save bundle to disk for offline use
                    if let Err(e) = self.save_bundle_to_disk(&bundle).await {
                        warn!("Failed to cache bundle to disk: {}", e);
                    }

                    // Save ETag to memory and disk
                    if let Some(ref e) = etag {
                        let mut etag_lock = self.cached_etag.write().await;
                        *etag_lock = Some(e.clone());
                        self.save_etag(e).await;
                    }

                    Some(bundle)
                }
                Err(e) => {
                    warn!("Failed to fetch from Registry API: {}. Trying disk cache...", e);
                    
                    // Try loading from disk cache
                    if let Some(cached_bundle) = self.load_bundle_from_disk().await {
                        info!("Using cached bundle from disk (offline mode)");
                        offline_mode = true;
                        Some(cached_bundle)
                    } else {
                        warn!("No disk cache available. Running offline with no registry servers.");
                        offline_mode = true;
                        None
                    }
                }
            }
        } else {
            // No API client configured, try disk cache
            if let Some(cached_bundle) = self.load_bundle_from_disk().await {
                info!("No API client configured. Using cached bundle from disk.");
                offline_mode = true;
                Some(cached_bundle)
            } else {
                None
            }
        };

        // 2. Process bundle if available
        let got_bundle = bundle_result.is_some();
        let base_servers = if let Some(bundle) = bundle_result {
            // Update UI config
            {
                let mut ui_lock = self.ui_config.write().await;
                *ui_lock = bundle.ui.clone();
            }

            // Update home config
            {
                let mut home_lock = self.home_config.write().await;
                *home_lock = bundle.home.clone();
            }
            
            // Mark source as Registry
            let registry_url = self.registry_client
                .as_ref()
                .map(|c| c.base_url().to_string())
                .unwrap_or_else(|| "cached".to_string());
            
            bundle.servers.into_iter().map(|mut s| {
                s.source = ServerSource::Registry {
                    url: registry_url.clone(),
                    name: "McpMux Registry".to_string(),
                };
                s
            }).collect::<Vec<_>>()
        } else {
            vec![]
        };

        // Update offline status
        {
            let mut offline_lock = self.is_offline.write().await;
            *offline_lock = offline_mode;
        }

        for server in base_servers {
            merged_servers.insert(server.id.clone(), server);
        }

        // 3. Load User Spaces (highest priority - overrides everything)
        match self.load_user_spaces().await {
            Ok(user_servers) => {
                info!("Loaded {} user-configured servers", user_servers.len());
                for server in user_servers {
                    if merged_servers.contains_key(&server.id) {
                        info!("User configuration overriding server: {}", server.id);
                    }
                    merged_servers.insert(server.id.clone(), server);
                }
            }
            Err(e) => error!("Failed to load user spaces: {}", e),
        }

        // 4. Update Cache
        let mut lock = self.servers.write().await;
        *lock = merged_servers;
        
        // 5. Update refresh timestamp ONLY if we got a bundle
        // This ensures we retry on next request if both API and disk cache failed
        if got_bundle {
            let mut last_refresh = self.last_refresh.write().await;
            *last_refresh = Some(Instant::now());
        } else {
            info!("No bundle available, will retry on next request");
        }
        
        Ok(())
    }

    /// Refresh if cache is stale
    pub async fn refresh_if_needed(&self) -> anyhow::Result<()> {
        if self.should_refresh().await {
            self.refresh().await?;
        }
        Ok(())
    }

    /// Reload only user spaces (called when bundle is unchanged via 304)
    async fn reload_user_spaces(&self) {
        // Get current servers (registry servers from cache)
        let mut servers = self.servers.read().await.clone();
        
        // Remove existing user space servers (they might have changed)
        servers.retain(|_, s| !matches!(s.source, ServerSource::UserSpace { .. }));
        
        // Load fresh user spaces
        match self.load_user_spaces().await {
            Ok(user_servers) => {
                info!("Reloaded {} user-configured servers", user_servers.len());
                for server in user_servers {
                    servers.insert(server.id.clone(), server);
                }
            }
            Err(e) => error!("Failed to reload user spaces: {}", e),
        }
        
        // Update cache
        let mut lock = self.servers.write().await;
        *lock = servers;
    }

    async fn load_user_spaces(&self) -> anyhow::Result<Vec<ServerDefinition>> {
        let mut results = Vec::new();
        
        if !self.spaces_dir.exists() {
            return Ok(results);
        }

        let mut entries = tokio::fs::read_dir(&self.spaces_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
                
                match self.load_single_user_file(&path, &file_name).await {
                    Ok(servers) => results.extend(servers),
                    Err(e) => warn!("Failed to parse user config {}: {}", path.display(), e),
                }
            }
        }

        Ok(results)
    }

    async fn load_single_user_file(&self, path: &PathBuf, space_id: &str) -> anyhow::Result<Vec<ServerDefinition>> {
        let content = tokio::fs::read_to_string(path).await?;
        let config: UserSpaceConfig = serde_json::from_str(&content)?;
        Ok(config.to_server_definitions(space_id, path.clone()))
    }

    // ============================================
    // Query Methods (all operate on local cache)
    // ============================================

    /// Get all servers (merged view).
    pub async fn list(&self) -> Vec<ServerDefinition> {
        self.servers.read().await.values().cloned().collect()
    }

    /// Get a specific server by ID.
    pub async fn get(&self, id: &str) -> Option<ServerDefinition> {
        self.servers.read().await.get(id).cloned()
    }

    /// Get featured server IDs from home config
    pub async fn featured_ids(&self) -> Vec<String> {
        let home = self.home_config.read().await;
        home.as_ref()
            .map(|h| h.featured_server_ids.clone())
            .unwrap_or_default()
    }

    /// Get featured servers
    pub async fn featured(&self) -> Vec<ServerDefinition> {
        let servers = self.servers.read().await;
        let featured_ids = self.featured_ids().await;
        
        featured_ids
            .iter()
            .filter_map(|id| servers.get(id))
            .cloned()
            .collect()
    }

    /// Search servers (searches in-memory cache)
    pub async fn search(&self, query: &str) -> Vec<ServerDefinition> {
        let query_lower = query.to_lowercase();
        
        self.servers
            .read()
            .await
            .values()
            .filter(|server| {
                server.name.to_lowercase().contains(&query_lower)
                    || server.description.as_ref().is_some_and(|d| d.to_lowercase().contains(&query_lower))
                    || server.alias.as_ref().is_some_and(|a| a.to_lowercase().contains(&query_lower))
                    || server.categories.iter().any(|c| c.to_lowercase().contains(&query_lower))
            })
            .cloned()
            .collect()
    }

    // ============================================
    // UI Configuration (API-driven)
    // ============================================

    /// Get the UI configuration from the bundle
    pub async fn ui_config(&self) -> UiConfig {
        self.ui_config.read().await.clone()
    }

    /// Get the home configuration from the bundle
    pub async fn home_config(&self) -> Option<HomeConfig> {
        self.home_config.read().await.clone()
    }
}
