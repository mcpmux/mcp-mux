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
use crate::service::app_settings_service::{keys, AppSettingsService};
use crate::service::registry_api_client::{
    FetchBundleResult, HomeConfig, RegistryApiClient, RegistryBundle, UiConfig,
};

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

    /// The disk cache, if it may be used: when bundles must be signed, only
    /// one saved from a bundle whose signature was checked.
    async fn load_usable_cache(&self, must_verify: bool) -> Option<RegistryBundle> {
        let bundle = self.load_bundle_from_disk().await?;
        if must_verify {
            let trusted = bundle.signed_by.as_deref().is_some_and(|key| {
                self.registry_client
                    .as_ref()
                    .is_some_and(|client| client.trusts_key(key))
            });
            if !trusted {
                warn!(
                    "Ignoring the cached registry bundle: its signature was never checked \
                     against a key that is trusted now"
                );
                return None;
            }
        }
        Some(bundle)
    }

    /// Whether `bundle` was published before the verified bundle in the disk
    /// cache (by `updated_at`, RFC 3339). Unparseable dates don't count.
    async fn is_older_than_cache(&self, bundle: &RegistryBundle) -> bool {
        let Some(cached) = self.load_usable_cache(true).await else {
            return false;
        };
        let parse = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok();
        matches!(
            (parse(&bundle.updated_at), parse(&cached.updated_at)),
            (Some(new), Some(old)) if new < old
        )
    }

    /// Load bundle from disk cache
    async fn load_bundle_from_disk(&self) -> Option<RegistryBundle> {
        let path = self.bundle_cache_path();

        if !path.exists() {
            return None;
        }

        match tokio::fs::read_to_string(&path).await {
            Ok(content) => match serde_json::from_str::<RegistryBundle>(&content) {
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
            },
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

    /// Remove the ETag from persistent storage
    async fn clear_etag(&self) {
        if let Some(ref settings) = self.settings_service {
            if let Err(e) = settings.delete(keys::registry::BUNDLE_ETAG).await {
                warn!("Failed to clear ETag in settings: {}", e);
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

        // Once bundles must be signed, only a cache saved from a verified
        // bundle may be revalidated with its ETag or used offline.
        let must_verify = self
            .registry_client
            .as_ref()
            .is_some_and(|client| client.verifies_bundles());

        // Get current ETag (from memory, or load from settings on first run)
        // IMPORTANT: Only use ETag if cache file exists, otherwise force fresh fetch
        let cache_file_exists = if must_verify {
            self.load_usable_cache(true).await.is_some()
        } else {
            self.bundle_cache_path().exists()
        };
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
                        if let Some(cached_bundle) = self.load_usable_cache(must_verify).await {
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
                Ok(FetchBundleResult::Updated { bundle, .. })
                    if must_verify && self.is_older_than_cache(&bundle).await =>
                {
                    // A signed bundle older than the verified one we have:
                    // replaying it must not roll the catalog back.
                    warn!(
                        "Registry sent an older bundle (updated {}) than the verified cache; \
                         keeping the cache",
                        bundle.updated_at
                    );
                    self.load_usable_cache(true).await
                }
                Ok(FetchBundleResult::Updated { bundle, etag }) => {
                    let bundle = *bundle; // Unbox the bundle
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

                    // Save ETag to memory and disk. If servers were skipped,
                    // forget it instead: the disk cache lacks them, and a later
                    // version that can read them must not get a 304 for this
                    // bundle and keep serving the cache without them.
                    if bundle.skipped_servers > 0 {
                        warn!(
                            "Skipped {} registry server(s) this version can't read; \
                             not caching the bundle ETag",
                            bundle.skipped_servers
                        );
                        *self.cached_etag.write().await = None;
                        self.clear_etag().await;
                    } else if let Some(ref e) = etag {
                        let mut etag_lock = self.cached_etag.write().await;
                        *etag_lock = Some(e.clone());
                        self.save_etag(e).await;
                    }

                    Some(bundle)
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch from Registry API: {}. Trying disk cache...",
                        e
                    );

                    // Try loading from disk cache
                    if let Some(cached_bundle) = self.load_usable_cache(must_verify).await {
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
            let registry_url = self
                .registry_client
                .as_ref()
                .map(|c| c.base_url().to_string())
                .unwrap_or_else(|| "cached".to_string());

            bundle
                .servers
                .into_iter()
                .map(|mut s| {
                    s.source = ServerSource::Registry {
                        url: registry_url.clone(),
                        name: "McpMux Registry".to_string(),
                    };
                    s
                })
                .collect::<Vec<_>>()
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
                let file_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                match self.load_single_user_file(&path, &file_name).await {
                    Ok(servers) => results.extend(servers),
                    Err(e) => warn!("Failed to parse user config {}: {}", path.display(), e),
                }
            }
        }

        Ok(results)
    }

    async fn load_single_user_file(
        &self,
        path: &PathBuf,
        space_id: &str,
    ) -> anyhow::Result<Vec<ServerDefinition>> {
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
                    || server
                        .description
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&query_lower))
                    || server
                        .alias
                        .as_ref()
                        .is_some_and(|a| a.to_lowercase().contains(&query_lower))
                    || server
                        .categories
                        .iter()
                        .any(|c| c.to_lowercase().contains(&query_lower))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::registry_api_client::test_registry::{
        bundle_data, key_pair, signed, MockRegistry,
    };

    async fn ids(service: &ServerDiscoveryService) -> Vec<String> {
        service.list().await.into_iter().map(|s| s.id).collect()
    }

    /// Once bundles must be signed, a cache saved before that is neither
    /// revalidated with its ETag nor used offline; a cache of a verified
    /// bundle is.
    #[tokio::test]
    async fn only_a_verified_cache_is_trusted_once_bundles_must_be_signed() {
        let dir = tempfile::tempdir().unwrap();
        let (pair, public) = key_pair();
        let registry = MockRegistry::start().await;
        let service = || {
            let mut service =
                ServerDiscoveryService::new(dir.path().join("data"), dir.path().join("spaces"));
            service.registry_client =
                Some(RegistryApiClient::new(registry.url.clone()).with_bundle_keys(vec![public]));
            service
        };

        // A cache and ETag saved before bundles were signed.
        let first = service();
        let legacy: RegistryBundle = serde_json::from_str(&bundle_data("legacy")).unwrap();
        first.save_bundle_to_disk(&legacy).await.unwrap();
        *first.cached_etag.write().await = Some("\"legacy\"".into());

        // The registry is down: the unchecked cache is not used.
        registry.serve(|s| s.down = true);
        first.refresh().await.unwrap();
        assert!(ids(&first).await.is_empty());
        assert!(first.is_offline().await);

        // Back up: the old ETag is not sent, and the signed bundle replaces
        // the cache.
        let (body, signature) = signed(&pair, &bundle_data("signed"));
        registry.serve(|s| {
            s.down = false;
            s.body = body;
            s.signature = Some(signature);
            s.etag = Some("\"v2\"".into());
        });
        first.refresh().await.unwrap();
        assert_eq!(ids(&first).await, ["signed"]);
        assert!(first
            .load_bundle_from_disk()
            .await
            .unwrap()
            .signed_by
            .is_some());

        // The verified cache is revalidated with its ETag (304) after a
        // restart...
        let second = service();
        *second.cached_etag.write().await = Some("\"v2\"".into());
        second.refresh().await.unwrap();
        assert_eq!(ids(&second).await, ["signed"]);

        // ...and used when the registry is down.
        registry.serve(|s| s.down = true);
        let third = service();
        third.refresh().await.unwrap();
        assert_eq!(ids(&third).await, ["signed"]);
        assert!(third.is_offline().await);

        assert_eq!(
            registry.if_none_match(),
            [None, None, Some("\"v2\"".to_string()), None]
        );
    }

    /// A service trusting `keys`, on the cache in `dir`.
    fn service_with(
        dir: &std::path::Path,
        url: &str,
        keys: Vec<[u8; 32]>,
    ) -> ServerDiscoveryService {
        let mut service = ServerDiscoveryService::new(dir.join("data"), dir.join("spaces"));
        service.registry_client =
            Some(RegistryApiClient::new(url.to_string()).with_bundle_keys(keys));
        service
    }

    /// `bundle_data(id)` published at `updated_at`.
    fn dated(id: &str, updated_at: &str) -> String {
        bundle_data(id).replace("2026-10-01T00:00:00Z", updated_at)
    }

    /// An older bundle with a valid signature (a replay) doesn't replace a
    /// newer verified one.
    #[tokio::test]
    async fn an_older_signed_bundle_does_not_roll_the_catalog_back() {
        let dir = tempfile::tempdir().unwrap();
        let (pair, public) = key_pair();
        let registry = MockRegistry::start().await;
        let service = service_with(dir.path(), &registry.url, vec![public]);

        let (body, signature) = signed(&pair, &dated("current", "2026-10-05T00:00:00Z"));
        registry.serve(|s| {
            s.body = body;
            s.signature = Some(signature);
        });
        service.refresh().await.unwrap();
        assert_eq!(ids(&service).await, ["current"]);

        let (body, signature) = signed(&pair, &dated("replayed", "2026-09-01T00:00:00Z"));
        registry.serve(|s| {
            s.body = body;
            s.signature = Some(signature);
        });
        service.refresh().await.unwrap();
        assert_eq!(ids(&service).await, ["current"]);
        assert_eq!(
            service.load_bundle_from_disk().await.unwrap().servers[0].id,
            "current"
        );
    }

    /// A cache verified with a key that is no longer trusted isn't used.
    #[tokio::test]
    async fn a_cache_signed_with_a_retired_key_is_not_used() {
        let dir = tempfile::tempdir().unwrap();
        let (old_pair, old_public) = key_pair();
        let (_, new_public) = key_pair();
        let registry = MockRegistry::start().await;

        let (body, signature) = signed(&old_pair, &bundle_data("old-key"));
        registry.serve(|s| {
            s.body = body;
            s.signature = Some(signature);
        });
        service_with(dir.path(), &registry.url, vec![old_public])
            .refresh()
            .await
            .unwrap();

        // The old key is retired and the registry is down.
        registry.serve(|s| s.down = true);
        let service = service_with(dir.path(), &registry.url, vec![new_public]);
        service.refresh().await.unwrap();
        assert!(ids(&service).await.is_empty());
    }

    /// With a verified cache, an unsigned or wrongly signed bundle from a
    /// reachable registry is refused and the cache is used.
    #[tokio::test]
    async fn a_bad_bundle_from_a_reachable_registry_falls_back_to_the_verified_cache() {
        let dir = tempfile::tempdir().unwrap();
        let (pair, public) = key_pair();
        let (other, _) = key_pair();
        let registry = MockRegistry::start().await;
        let service = service_with(dir.path(), &registry.url, vec![public]);

        let (body, signature) = signed(&pair, &bundle_data("verified"));
        registry.serve(|s| {
            s.body = body;
            s.signature = Some(signature);
        });
        service.refresh().await.unwrap();

        let (unsigned_body, _) = signed(&pair, &bundle_data("unsigned"));
        let (bad_body, bad_signature) = signed(&other, &bundle_data("wrong-key"));
        for (body, signature) in [(unsigned_body, None), (bad_body, Some(bad_signature))] {
            registry.serve(|s| {
                s.body = body;
                s.signature = signature;
                s.etag = None;
            });
            service.refresh().await.unwrap();
            assert_eq!(ids(&service).await, ["verified"]);
        }
    }
}
