//! Shared test utilities and fixtures for McpMux integration tests.

pub use mcpmux_core::domain::{InstalledServer, Space, FeatureSet, FeatureSetType};

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
        InstalledServer::new(space_id, server_id)
            .with_enabled(true)
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
    pub fn server_all_feature_set(space_id: &str, server_id: &str, server_name: &str) -> FeatureSet {
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
            let db = Database::open(&db_path)
                .expect("Failed to open test database");
            Self {
                db,
                db_path,
                _temp_dir: temp_dir,
            }
        }

        /// Create an in-memory database for fast tests
        pub fn in_memory() -> Self {
            let temp_dir = TempDir::new().expect("Failed to create temp dir");
            let db = Database::open_in_memory()
                .expect("Failed to open in-memory database");
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
        timeout(duration, f)
            .await
            .expect("Operation timed out")
    }

    /// Default test timeout (5 seconds)
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
}
