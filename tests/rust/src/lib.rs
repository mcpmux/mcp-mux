//! Shared test utilities and fixtures for McpMux integration tests.

pub use mcpmux_core::domain::{
    Client, Credential, FeatureSet, FeatureSetType, InstalledServer, Space,
};
pub use mcpmux_core::{
    ConnectionStatus, DiscoveredCapabilities, DomainEvent, FeatureType, ServerFeature,
};

/// Mock repository implementations
pub mod mocks;
pub use mocks::MockRepositories;

/// Service test helpers
pub mod services;
pub use services::ServerManagerTestHarness;

/// Event testing utilities
pub mod events {
    use mcpmux_core::DomainEvent;
    use std::time::Duration;
    use tokio::sync::broadcast;

    /// Create a test event channel with sufficient capacity
    pub fn test_event_channel() -> (
        broadcast::Sender<DomainEvent>,
        broadcast::Receiver<DomainEvent>,
    ) {
        broadcast::channel(100)
    }

    /// Collect events from a receiver with a timeout
    pub async fn collect_events(
        mut rx: broadcast::Receiver<DomainEvent>,
        timeout: Duration,
    ) -> Vec<DomainEvent> {
        let mut events = Vec::new();
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(event)) => events.push(event),
                Ok(Err(_)) => break, // Channel closed or lagged
                Err(_) => break,     // Timeout
            }
        }

        events
    }

    /// Wait for a specific event type
    pub async fn wait_for_event<F>(
        mut rx: broadcast::Receiver<DomainEvent>,
        timeout: Duration,
        predicate: F,
    ) -> Option<DomainEvent>
    where
        F: Fn(&DomainEvent) -> bool,
    {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }

            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(event)) if predicate(&event) => return Some(event),
                Ok(Ok(_)) => continue,     // Not the event we want
                Ok(Err(_)) => return None, // Channel closed
                Err(_) => return None,     // Timeout
            }
        }
    }

    /// Assert that a ServerStatusChanged event was emitted with expected status
    pub fn assert_status_changed(
        events: &[DomainEvent],
        expected_server_id: &str,
        expected_status: mcpmux_core::ConnectionStatus,
    ) -> bool {
        events.iter().any(|e| {
            if let DomainEvent::ServerStatusChanged {
                server_id, status, ..
            } = e
            {
                server_id == expected_server_id && *status == expected_status
            } else {
                false
            }
        })
    }
}

/// Server feature fixtures for testing
pub mod features {
    use mcpmux_core::ServerFeature;

    /// Create a test tool feature
    pub fn test_tool(space_id: &str, server_id: &str, name: &str) -> ServerFeature {
        ServerFeature::tool(space_id, server_id, name)
            .with_description(format!("Test tool: {}", name))
    }

    /// Create a test prompt feature
    pub fn test_prompt(space_id: &str, server_id: &str, name: &str) -> ServerFeature {
        ServerFeature::prompt(space_id, server_id, name)
            .with_description(format!("Test prompt: {}", name))
    }

    /// Create a test resource feature
    pub fn test_resource(space_id: &str, server_id: &str, uri: &str) -> ServerFeature {
        ServerFeature::resource(space_id, server_id, uri)
            .with_description(format!("Test resource: {}", uri))
    }

    /// Create a set of test features for a server
    pub fn test_feature_set(space_id: &str, server_id: &str) -> Vec<ServerFeature> {
        vec![
            test_tool(space_id, server_id, "read_file"),
            test_tool(space_id, server_id, "write_file"),
            test_prompt(space_id, server_id, "summarize"),
            test_resource(space_id, server_id, "file:///test"),
        ]
    }
}

/// Test fixture utilities
pub mod fixtures {
    use super::*;
    use uuid::Uuid;

    /// Create a test space with default values
    pub fn test_space(name: &str) -> Space {
        Space::new(name)
            .with_icon("🧪")
            .with_description(format!("Test space: {}", name))
    }

    /// Create a default test space
    pub fn default_test_space() -> Space {
        Space::new("Default Test Space")
            .with_icon("🏠")
            .set_default()
    }

    /// Create a test installed server
    pub fn test_installed_server(space_id: &str, server_id: &str) -> InstalledServer {
        InstalledServer::new(space_id, server_id).with_enabled(true)
    }

    /// Create a test feature set
    pub fn test_feature_set(name: &str, space_id: &str) -> FeatureSet {
        FeatureSet::new_custom(name, space_id)
            .with_icon("🔧")
            .with_description(format!("Test feature set: {}", name))
    }

    /// Create an "all features" feature set
    pub fn all_features_set(space_id: &str) -> FeatureSet {
        FeatureSet::new_all(space_id)
    }

    /// Create a "default" feature set
    pub fn default_feature_set(space_id: &str) -> FeatureSet {
        FeatureSet::new_default(space_id)
    }

    /// Create a server-all feature set
    pub fn server_all_feature_set(
        space_id: &str,
        server_id: &str,
        server_name: &str,
    ) -> FeatureSet {
        FeatureSet::new_server_all(space_id, server_id, server_name)
    }

    /// Generate a random UUID string
    pub fn random_id() -> String {
        Uuid::new_v4().to_string()
    }
}

/// Database test helpers
pub mod db {
    use mcpmux_storage::Database;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    /// Database file name
    const DB_FILE: &str = "mcpmux.db";

    /// Create a temporary database for testing
    pub struct TestDatabase {
        pub db: Database,
        _temp_dir: TempDir,
        db_path: PathBuf,
    }

    impl TestDatabase {
        /// Create a new test database in a temporary directory
        pub fn new() -> Self {
            let temp_dir = TempDir::new().expect("Failed to create temp dir");
            let db_path = temp_dir.path().join(DB_FILE);
            let db = Database::open(&db_path).expect("Failed to open test database");
            Self {
                db,
                db_path,
                _temp_dir: temp_dir,
            }
        }

        /// Create an in-memory database for fast tests
        pub fn in_memory() -> Self {
            let temp_dir = TempDir::new().expect("Failed to create temp dir");
            let db = Database::open_in_memory().expect("Failed to open in-memory database");
            Self {
                db,
                db_path: PathBuf::new(),
                _temp_dir: temp_dir,
            }
        }

        /// Get the database directory path
        pub fn path(&self) -> &Path {
            self._temp_dir.path()
        }

        /// Get the full database file path
        pub fn db_path(&self) -> &Path {
            &self.db_path
        }
    }

    impl Default for TestDatabase {
        fn default() -> Self {
            Self::new()
        }
    }
}

/// Async test helpers
pub mod async_helpers {
    use std::time::Duration;
    use tokio::time::timeout;

    /// Run an async operation with a timeout
    pub async fn with_timeout<F, T>(duration: Duration, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        timeout(duration, f).await.expect("Operation timed out")
    }

    /// Default test timeout (5 seconds)
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
}
