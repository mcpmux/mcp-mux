//! App Settings Service
//!
//! High-level service for managing application settings with typed access.
//! Provides convenient methods for common settings while using the repository
//! for persistence.

use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::AppSettingsRepository;

// =============================================================================
// Setting Keys (centralized constants)
// =============================================================================

/// Setting key constants for type-safe access.
pub mod keys {
    /// Gateway settings namespace
    pub mod gateway {
        /// Gateway server port (u16)
        pub const PORT: &str = "gateway.port";
        /// Auto-start gateway on app launch (bool)
        pub const AUTO_START: &str = "gateway.auto_start";
        /// Web admin server enabled (bool)
        pub const ADMIN_ENABLED: &str = "gateway.admin_enabled";
        /// Web admin server port (u16)
        pub const ADMIN_PORT: &str = "gateway.admin_port";
        /// Trust Cloudflare Access JWT (bool)
        pub const ADMIN_TRUST_CF_ACCESS: &str = "gateway.admin_trust_cf_access";
        /// Cloudflare team domain for JWT issuer verification
        pub const ADMIN_CF_TEAM_DOMAIN: &str = "gateway.admin_cf_team_domain";
        /// UUID of the [`Machine`](crate::domain::Machine) this install identifies as.
        pub const LOCAL_MACHINE_ID: &str = "gateway.local_machine_id";
        /// Public HTTPS URL for remote MCP clients (string, e.g. https://mcp.example.com).
        /// Shares the desktop app's `gateway.public_base_url` key so the admin API and
        /// the desktop Settings page read/write the same setting.
        pub const PUBLIC_URL: &str = "gateway.public_base_url";
    }

    /// OAuth callback settings namespace
    pub mod oauth {
        /// Preferred OAuth callback port (u16)
        pub const CALLBACK_PORT: &str = "oauth.callback_port";
    }

    /// UI settings namespace
    pub mod ui {
        /// UI theme ("light", "dark", "system")
        pub const THEME: &str = "ui.theme";
        /// Window state JSON (position, size, maximized)
        pub const WINDOW_STATE: &str = "ui.window_state";
    }

    /// Logs settings namespace
    pub mod logs {
        /// Number of days to retain log files (u32, 0 = keep forever)
        pub const RETENTION_DAYS: &str = "logs.retention_days";
    }

    /// Registry settings namespace
    pub mod registry {
        /// Cached ETag from last bundle fetch
        pub const BUNDLE_ETAG: &str = "registry.bundle_etag";
    }

    /// Browser/app viewer profiles mapped to machine catalog rows.
    pub mod viewer {
        /// JSON map of viewer_device_id → machine UUID string.
        pub const DEVICES: &str = "viewer.devices";
    }
}

// =============================================================================
// AppSettingsService
// =============================================================================

/// Service for managing application settings with typed access.
///
/// Wraps the repository with convenient typed methods and default values.
///
/// # Example
/// ```ignore
/// let service = AppSettingsService::new(repo);
///
/// // Typed access with defaults
/// let port = service.get_gateway_port().await;
/// let auto_start = service.get_gateway_auto_start().await;
///
/// // Set values
/// service.set_gateway_port(45818).await?;
/// ```
pub struct AppSettingsService {
    repository: Arc<dyn AppSettingsRepository>,
}

impl AppSettingsService {
    /// Create a new settings service with the given repository.
    pub fn new(repository: Arc<dyn AppSettingsRepository>) -> Self {
        Self { repository }
    }

    // =========================================================================
    // Generic typed access
    // =========================================================================

    /// Get a setting value parsed as the specified type.
    ///
    /// Returns `None` if the key doesn't exist or parsing fails.
    pub async fn get_typed<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        match self.repository.get(key).await {
            Ok(Some(value)) => {
                // Try parsing as JSON first (for complex types)
                if let Ok(parsed) = serde_json::from_str(&value) {
                    return Some(parsed);
                }
                // For simple types, try wrapping in quotes for JSON parsing
                if let Ok(parsed) = serde_json::from_str(&format!("\"{}\"", value)) {
                    return Some(parsed);
                }
                warn!("[Settings] Failed to parse '{}' value: {}", key, value);
                None
            }
            Ok(None) => None,
            Err(e) => {
                warn!("[Settings] Failed to get '{}': {}", key, e);
                None
            }
        }
    }

    /// Get a string setting value.
    pub async fn get_string(&self, key: &str) -> Option<String> {
        match self.repository.get(key).await {
            Ok(value) => value,
            Err(e) => {
                warn!("[Settings] Failed to get '{}': {}", key, e);
                None
            }
        }
    }

    /// Get a setting value with a default if not set.
    pub async fn get_or_default<T: DeserializeOwned>(&self, key: &str, default: T) -> T {
        self.get_typed(key).await.unwrap_or(default)
    }

    /// Set a setting value, serializing it appropriately.
    pub async fn set_typed<T: Serialize>(&self, key: &str, value: &T) -> anyhow::Result<()> {
        let serialized = serde_json::to_string(value)?;
        // Remove quotes for simple string values to keep storage clean
        let clean_value = serialized.trim_matches('"');
        self.repository.set(key, clean_value).await
    }

    /// Set a raw string value.
    pub async fn set_string(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.repository.set(key, value).await
    }

    /// Delete a setting.
    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.repository.delete(key).await
    }

    // =========================================================================
    // Gateway settings
    // =========================================================================

    /// Default gateway port (45818 - high port to avoid conflicts)
    pub const DEFAULT_GATEWAY_PORT: u16 = 45818;

    /// Get the configured gateway port.
    ///
    /// Returns `None` if not set (caller should use default or dynamic allocation).
    pub async fn get_gateway_port(&self) -> Option<u16> {
        self.get_typed(keys::gateway::PORT).await
    }

    /// Set the gateway port.
    pub async fn set_gateway_port(&self, port: u16) -> anyhow::Result<()> {
        info!("[Settings] Setting gateway port to {}", port);
        self.repository
            .set(keys::gateway::PORT, &port.to_string())
            .await
    }

    /// Clear the gateway port (revert to default/dynamic).
    pub async fn clear_gateway_port(&self) -> anyhow::Result<()> {
        info!("[Settings] Clearing gateway port setting");
        self.repository.delete(keys::gateway::PORT).await
    }

    /// Get whether gateway should auto-start (default: true).
    pub async fn get_gateway_auto_start(&self) -> bool {
        self.get_string(keys::gateway::AUTO_START)
            .await
            .map(|v| v == "true")
            .unwrap_or(true)
    }

    /// Set gateway auto-start preference.
    pub async fn set_gateway_auto_start(&self, auto_start: bool) -> anyhow::Result<()> {
        info!("[Settings] Setting gateway auto_start to {}", auto_start);
        self.repository
            .set(
                keys::gateway::AUTO_START,
                if auto_start { "true" } else { "false" },
            )
            .await
    }

    // =========================================================================
    // Admin server settings
    // =========================================================================

    /// Default admin server port.
    pub const DEFAULT_ADMIN_PORT: u16 = 45819;

    /// Get whether the web admin server is enabled (default: false).
    pub async fn get_admin_enabled(&self) -> bool {
        self.get_string(keys::gateway::ADMIN_ENABLED)
            .await
            .map(|v| v == "true")
            .unwrap_or(false)
    }

    /// Set whether the web admin server is enabled.
    pub async fn set_admin_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        info!("[Settings] Setting admin_enabled to {}", enabled);
        self.repository
            .set(
                keys::gateway::ADMIN_ENABLED,
                if enabled { "true" } else { "false" },
            )
            .await
    }

    /// Get the configured admin server port (defaults to `DEFAULT_ADMIN_PORT`).
    pub async fn get_admin_port(&self) -> u16 {
        self.get_typed(keys::gateway::ADMIN_PORT)
            .await
            .unwrap_or(Self::DEFAULT_ADMIN_PORT)
    }

    /// Set the admin server port.
    pub async fn set_admin_port(&self, port: u16) -> anyhow::Result<()> {
        info!("[Settings] Setting admin port to {}", port);
        self.repository
            .set(keys::gateway::ADMIN_PORT, &port.to_string())
            .await
    }

    /// Get whether to trust Cloudflare Access JWTs (default: false).
    pub async fn get_admin_trust_cf_access(&self) -> bool {
        self.get_string(keys::gateway::ADMIN_TRUST_CF_ACCESS)
            .await
            .map(|v| v == "true")
            .unwrap_or(false)
    }

    /// Set whether to trust Cloudflare Access JWTs.
    pub async fn set_admin_trust_cf_access(&self, trust: bool) -> anyhow::Result<()> {
        info!("[Settings] Setting admin_trust_cf_access to {}", trust);
        self.repository
            .set(
                keys::gateway::ADMIN_TRUST_CF_ACCESS,
                if trust { "true" } else { "false" },
            )
            .await
    }

    /// Get the Cloudflare team domain for admin JWT verification.
    pub async fn get_admin_cf_team_domain(&self) -> Option<String> {
        self.get_string(keys::gateway::ADMIN_CF_TEAM_DOMAIN).await
    }

    /// Set the Cloudflare team domain (empty or None clears).
    pub async fn set_admin_cf_team_domain(&self, domain: Option<&str>) -> anyhow::Result<()> {
        let value = domain.unwrap_or("").trim();
        info!("[Settings] Setting admin_cf_team_domain");
        self.repository
            .set(keys::gateway::ADMIN_CF_TEAM_DOMAIN, value)
            .await
    }

    /// Public HTTPS URL advertised in OAuth metadata for tunnel clients.
    pub async fn get_gateway_public_url(&self) -> Option<String> {
        self.get_string(keys::gateway::PUBLIC_URL)
            .await
            .filter(|value| !value.is_empty())
    }

    /// Persist the public gateway URL (empty clears).
    pub async fn set_gateway_public_url(&self, url: &str) -> anyhow::Result<()> {
        info!("[Settings] Setting gateway public_url");
        self.repository
            .set(keys::gateway::PUBLIC_URL, url.trim())
            .await
    }

    /// Clear the configured public gateway URL.
    pub async fn clear_gateway_public_url(&self) -> anyhow::Result<()> {
        info!("[Settings] Clearing gateway public_url");
        self.repository.delete(keys::gateway::PUBLIC_URL).await
    }

    // =========================================================================
    // Machine identity
    // =========================================================================

    /// Get the machine id this install is registered as, if any.
    pub async fn get_local_machine_id(&self) -> Option<Uuid> {
        self.get_string(keys::gateway::LOCAL_MACHINE_ID)
            .await
            .and_then(|value| Uuid::parse_str(&value).ok())
    }

    /// Set or clear the machine id for this install.
    pub async fn set_local_machine_id(&self, id: Option<Uuid>) -> anyhow::Result<()> {
        match id {
            Some(uuid) => {
                info!("[Settings] Setting local_machine_id to {}", uuid);
                self.repository
                    .set(keys::gateway::LOCAL_MACHINE_ID, &uuid.to_string())
                    .await
            }
            None => {
                info!("[Settings] Clearing local_machine_id");
                self.repository
                    .delete(keys::gateway::LOCAL_MACHINE_ID)
                    .await
            }
        }
    }

    /// Get the machine id linked to a viewer device profile, if any.
    pub async fn get_viewer_machine_id(&self, viewer_id: &str) -> Option<Uuid> {
        let map: HashMap<String, String> = self
            .get_or_default(keys::viewer::DEVICES, HashMap::new())
            .await;
        map.get(viewer_id)
            .and_then(|value| Uuid::parse_str(value).ok())
    }

    /// Link or unlink a viewer device profile to a machine catalog row.
    pub async fn set_viewer_machine_id(
        &self,
        viewer_id: &str,
        machine_id: Option<Uuid>,
    ) -> anyhow::Result<()> {
        let mut map: HashMap<String, String> = self
            .get_or_default(keys::viewer::DEVICES, HashMap::new())
            .await;

        match machine_id {
            Some(id) => {
                map.insert(viewer_id.to_string(), id.to_string());
            }
            None => {
                map.remove(viewer_id);
            }
        }

        if map.is_empty() {
            self.repository.delete(keys::viewer::DEVICES).await
        } else {
            self.set_typed(keys::viewer::DEVICES, &map).await
        }
    }

    // =========================================================================
    // OAuth settings
    // =========================================================================

    /// Default OAuth callback port (45819 - adjacent to gateway port)
    pub const DEFAULT_OAUTH_CALLBACK_PORT: u16 = 45819;

    /// Get the preferred OAuth callback port.
    ///
    /// Returns `None` if not set (caller should use default or dynamic allocation).
    pub async fn get_oauth_callback_port(&self) -> Option<u16> {
        self.get_typed(keys::oauth::CALLBACK_PORT).await
    }

    /// Set the preferred OAuth callback port.
    pub async fn set_oauth_callback_port(&self, port: u16) -> anyhow::Result<()> {
        info!("[Settings] Setting OAuth callback port to {}", port);
        self.repository
            .set(keys::oauth::CALLBACK_PORT, &port.to_string())
            .await
    }

    // =========================================================================
    // UI settings
    // =========================================================================

    /// Get the UI theme preference ("light", "dark", or "system").
    pub async fn get_theme(&self) -> String {
        self.get_string(keys::ui::THEME)
            .await
            .unwrap_or_else(|| "system".to_string())
    }

    /// Set the UI theme preference.
    pub async fn set_theme(&self, theme: &str) -> anyhow::Result<()> {
        info!("[Settings] Setting theme to {}", theme);
        self.repository.set(keys::ui::THEME, theme).await
    }

    /// Get window state (position, size, maximized).
    pub async fn get_window_state<T: DeserializeOwned + Default>(&self) -> T {
        self.get_typed(keys::ui::WINDOW_STATE)
            .await
            .unwrap_or_default()
    }

    /// Set window state.
    pub async fn set_window_state<T: Serialize>(&self, state: &T) -> anyhow::Result<()> {
        let json = serde_json::to_string(state)?;
        self.repository.set(keys::ui::WINDOW_STATE, &json).await
    }

    // =========================================================================
    // Logs settings
    // =========================================================================

    /// Default log retention period in days (30 days)
    pub const DEFAULT_LOG_RETENTION_DAYS: u32 = 30;

    /// Get the log retention period in days (0 = keep forever).
    pub async fn get_log_retention_days(&self) -> u32 {
        self.get_typed(keys::logs::RETENTION_DAYS)
            .await
            .unwrap_or(Self::DEFAULT_LOG_RETENTION_DAYS)
    }

    /// Set the log retention period in days.
    pub async fn set_log_retention_days(&self, days: u32) -> anyhow::Result<()> {
        info!("[Settings] Setting log retention to {} days", days);
        self.repository
            .set(keys::logs::RETENTION_DAYS, &days.to_string())
            .await
    }

    // =========================================================================
    // Utility methods
    // =========================================================================

    /// List all settings (for debugging/export).
    pub async fn list_all(&self) -> anyhow::Result<Vec<(String, String)>> {
        self.repository.list().await
    }

    /// List settings by namespace prefix.
    pub async fn list_by_prefix(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
        self.repository.list_by_prefix(prefix).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    /// In-memory repository for testing
    struct InMemorySettingsRepository {
        data: RwLock<HashMap<String, String>>,
    }

    impl InMemorySettingsRepository {
        fn new() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl AppSettingsRepository for InMemorySettingsRepository {
        async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
            Ok(self.data.read().await.get(key).cloned())
        }

        async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
            self.data
                .write()
                .await
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        async fn delete(&self, key: &str) -> anyhow::Result<()> {
            self.data.write().await.remove(key);
            Ok(())
        }

        async fn list(&self) -> anyhow::Result<Vec<(String, String)>> {
            let data = self.data.read().await;
            let mut items: Vec<_> = data.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            items.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(items)
        }

        async fn list_by_prefix(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
            let data = self.data.read().await;
            let mut items: Vec<_> = data
                .iter()
                .filter(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            items.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(items)
        }
    }

    #[tokio::test]
    async fn test_gateway_port() {
        let repo = Arc::new(InMemorySettingsRepository::new());
        let service = AppSettingsService::new(repo);

        // Initially not set
        assert_eq!(service.get_gateway_port().await, None);

        // Set port
        service.set_gateway_port(45818).await.unwrap();
        assert_eq!(service.get_gateway_port().await, Some(45818));

        // Clear port
        service.clear_gateway_port().await.unwrap();
        assert_eq!(service.get_gateway_port().await, None);
    }

    #[tokio::test]
    async fn test_gateway_auto_start() {
        let repo = Arc::new(InMemorySettingsRepository::new());
        let service = AppSettingsService::new(repo);

        // Default is true
        assert!(service.get_gateway_auto_start().await);

        // Set to false
        service.set_gateway_auto_start(false).await.unwrap();
        assert!(!service.get_gateway_auto_start().await);

        // Set back to true
        service.set_gateway_auto_start(true).await.unwrap();
        assert!(service.get_gateway_auto_start().await);
    }

    #[tokio::test]
    async fn test_theme() {
        let repo = Arc::new(InMemorySettingsRepository::new());
        let service = AppSettingsService::new(repo);

        // Default is "system"
        assert_eq!(service.get_theme().await, "system");

        // Set theme
        service.set_theme("dark").await.unwrap();
        assert_eq!(service.get_theme().await, "dark");
    }

    #[tokio::test]
    async fn test_typed_json_value() {
        let repo = Arc::new(InMemorySettingsRepository::new());
        let service = AppSettingsService::new(repo);

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug, Default)]
        struct WindowState {
            x: i32,
            y: i32,
            width: u32,
            height: u32,
        }

        let state = WindowState {
            x: 100,
            y: 200,
            width: 800,
            height: 600,
        };
        service.set_window_state(&state).await.unwrap();

        let loaded: WindowState = service.get_window_state().await;
        assert_eq!(loaded, state);
    }

    #[tokio::test]
    async fn test_log_retention_days() {
        let repo = Arc::new(InMemorySettingsRepository::new());
        let service = AppSettingsService::new(repo);

        // Default is 30 days
        assert_eq!(service.get_log_retention_days().await, 30);

        // Set to 7 days
        service.set_log_retention_days(7).await.unwrap();
        assert_eq!(service.get_log_retention_days().await, 7);

        // Set to 0 (keep forever)
        service.set_log_retention_days(0).await.unwrap();
        assert_eq!(service.get_log_retention_days().await, 0);
    }
}
