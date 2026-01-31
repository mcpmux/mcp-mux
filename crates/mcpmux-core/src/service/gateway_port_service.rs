//! Gateway Port Service
//!
//! Manages port allocation and persistence for the MCP gateway server.
//! Uses AppSettingsRepository for persistence.

use std::net::TcpListener;
use std::sync::Arc;
use tracing::{info, warn};

use crate::AppSettingsRepository;
use super::app_settings_service::keys;

/// Default port for the MCP gateway server.
///
/// Uses a high port number (45818) to avoid conflicts with common services.
/// 45818 is in the dynamic/private port range.
pub const DEFAULT_GATEWAY_PORT: u16 = 45818;

/// Result of port resolution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortResolution {
    /// Use a specific port (persisted or default)
    Fixed(u16),
    /// Need to dynamically allocate a port
    Dynamic,
}

impl PortResolution {
    pub fn port(&self) -> Option<u16> {
        match self {
            PortResolution::Fixed(port) => Some(*port),
            PortResolution::Dynamic => None,
        }
    }

    pub fn is_dynamic(&self) -> bool {
        matches!(self, PortResolution::Dynamic)
    }
}

/// Errors that can occur during port allocation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortAllocationError {
    /// The requested port is already in use
    PortInUse(u16),
    /// Failed to bind to any port
    BindFailed(String),
    /// Failed to get local address after binding
    AddressFailed(String),
    /// Failed to persist port setting
    PersistFailed(String),
}

impl std::fmt::Display for PortAllocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortAllocationError::PortInUse(port) => {
                write!(f, "Port {} is already in use", port)
            }
            PortAllocationError::BindFailed(e) => {
                write!(f, "Failed to bind to port: {}", e)
            }
            PortAllocationError::AddressFailed(e) => {
                write!(f, "Failed to get port address: {}", e)
            }
            PortAllocationError::PersistFailed(e) => {
                write!(f, "Failed to persist port: {}", e)
            }
        }
    }
}

impl std::error::Error for PortAllocationError {}

/// Check if a port is available for binding.
pub fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Allocate a dynamic port by letting the OS assign one.
pub fn allocate_dynamic_port() -> Result<u16, PortAllocationError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| PortAllocationError::BindFailed(e.to_string()))?;

    let port = listener
        .local_addr()
        .map_err(|e| PortAllocationError::AddressFailed(e.to_string()))?
        .port();

    drop(listener);
    info!("[PortService] Allocated dynamic port {}", port);
    Ok(port)
}

/// Service for managing gateway port allocation and persistence.
///
/// Uses AppSettingsRepository for storing the port in SQLite.
pub struct GatewayPortService {
    settings: Arc<dyn AppSettingsRepository>,
}

impl GatewayPortService {
    /// Create a new gateway port service.
    pub fn new(settings: Arc<dyn AppSettingsRepository>) -> Self {
        Self { settings }
    }

    /// Load the persisted gateway port from settings.
    pub async fn load_persisted_port(&self) -> Option<u16> {
        match self.settings.get(keys::gateway::PORT).await {
            Ok(Some(value)) => value.parse().ok(),
            _ => None,
        }
    }

    /// Save the gateway port to settings.
    pub async fn save_port(&self, port: u16) -> Result<(), PortAllocationError> {
        self.settings
            .set(keys::gateway::PORT, &port.to_string())
            .await
            .map_err(|e| PortAllocationError::PersistFailed(e.to_string()))
    }

    /// Resolve which port to use based on the fallback strategy.
    ///
    /// Strategy:
    /// 1. Try the persisted port (if any and available)
    /// 2. Try the default port (45818) if available
    /// 3. Return Dynamic to indicate OS should assign a port
    pub async fn resolve(&self) -> PortResolution {
        // 1. Try persisted port first
        if let Some(persisted) = self.load_persisted_port().await {
            if is_port_available(persisted) {
                info!("[PortService] Using persisted port {}", persisted);
                return PortResolution::Fixed(persisted);
            }
            info!("[PortService] Persisted port {} unavailable", persisted);
        }

        // 2. Try default port
        if is_port_available(DEFAULT_GATEWAY_PORT) {
            info!("[PortService] Using default port {}", DEFAULT_GATEWAY_PORT);
            return PortResolution::Fixed(DEFAULT_GATEWAY_PORT);
        }
        info!("[PortService] Default port {} unavailable", DEFAULT_GATEWAY_PORT);

        // 3. Need dynamic port assignment
        info!("[PortService] Will use dynamic port allocation");
        PortResolution::Dynamic
    }

    /// Resolve and allocate a port, persisting it for future use.
    ///
    /// This is the main entry point for getting a usable gateway port.
    pub async fn resolve_and_allocate(&self) -> Result<u16, PortAllocationError> {
        match self.resolve().await {
            PortResolution::Fixed(port) => {
                // Ensure port is persisted (for first-run with default port)
                if self.load_persisted_port().await.is_none() {
                    if let Err(e) = self.save_port(port).await {
                        warn!("[PortService] Failed to persist port {}: {}", port, e);
                    }
                }
                Ok(port)
            }
            PortResolution::Dynamic => {
                let port = allocate_dynamic_port()?;
                
                // Persist for next startup
                if let Err(e) = self.save_port(port).await {
                    warn!("[PortService] Failed to persist dynamic port {}: {}", port, e);
                }
                
                Ok(port)
            }
        }
    }

    /// Resolve a port with an optional explicit override.
    ///
    /// If `explicit_port` is provided, validates it's available.
    /// Otherwise, uses the standard resolution strategy.
    pub async fn resolve_with_override(&self, explicit_port: Option<u16>) -> Result<u16, PortAllocationError> {
        if let Some(port) = explicit_port {
            if is_port_available(port) {
                Ok(port)
            } else {
                Err(PortAllocationError::PortInUse(port))
            }
        } else {
            self.resolve_and_allocate().await
        }
    }

    /// Get whether gateway should auto-start.
    pub async fn get_auto_start(&self) -> bool {
        match self.settings.get(keys::gateway::AUTO_START).await {
            Ok(Some(value)) => value == "true",
            _ => true, // Default to true
        }
    }

    /// Set gateway auto-start preference.
    pub async fn set_auto_start(&self, auto_start: bool) -> Result<(), PortAllocationError> {
        self.settings
            .set(keys::gateway::AUTO_START, if auto_start { "true" } else { "false" })
            .await
            .map_err(|e| PortAllocationError::PersistFailed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    struct InMemorySettings {
        data: RwLock<HashMap<String, String>>,
    }

    impl InMemorySettings {
        fn new() -> Self {
            Self { data: RwLock::new(HashMap::new()) }
        }
    }

    #[async_trait]
    impl AppSettingsRepository for InMemorySettings {
        async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
            Ok(self.data.read().await.get(key).cloned())
        }
        async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
            self.data.write().await.insert(key.to_string(), value.to_string());
            Ok(())
        }
        async fn delete(&self, key: &str) -> anyhow::Result<()> {
            self.data.write().await.remove(key);
            Ok(())
        }
        async fn list(&self) -> anyhow::Result<Vec<(String, String)>> {
            Ok(self.data.read().await.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        }
        async fn list_by_prefix(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
            Ok(self.data.read().await.iter()
                .filter(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect())
        }
    }

    #[test]
    fn test_default_gateway_port() {
        assert_eq!(DEFAULT_GATEWAY_PORT, 45818);
    }

    #[test]
    fn test_port_resolution_enum() {
        let fixed = PortResolution::Fixed(45818);
        assert_eq!(fixed.port(), Some(45818));
        assert!(!fixed.is_dynamic());

        let dynamic = PortResolution::Dynamic;
        assert_eq!(dynamic.port(), None);
        assert!(dynamic.is_dynamic());
    }

    #[test]
    fn test_is_port_available() {
        // Dynamic port should be available after allocation
        let port = allocate_dynamic_port().unwrap();
        assert!(is_port_available(port));
    }

    #[tokio::test]
    async fn test_service_persistence() {
        let settings = Arc::new(InMemorySettings::new());
        let service = GatewayPortService::new(settings);

        // Should be None initially
        assert!(service.load_persisted_port().await.is_none());

        // Save and load
        service.save_port(12345).await.unwrap();
        assert_eq!(service.load_persisted_port().await, Some(12345));
    }

    #[tokio::test]
    async fn test_auto_start() {
        let settings = Arc::new(InMemorySettings::new());
        let service = GatewayPortService::new(settings);

        // Default is true
        assert!(service.get_auto_start().await);

        // Set to false
        service.set_auto_start(false).await.unwrap();
        assert!(!service.get_auto_start().await);
    }

    #[test]
    fn test_port_allocation_error_display() {
        let err = PortAllocationError::PortInUse(3000);
        assert!(err.to_string().contains("3000"));
    }
}
