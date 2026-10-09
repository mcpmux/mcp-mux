//! Shared bootstrap: data dir, lock, key, database, repositories, services,
//! and the gateway dependencies that both the desktop and the daemon use.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tracing::{info, warn};
use zeroize::Zeroizing;

use mcpmux_core::{
    AppSettingsRepository, AppSettingsService, CredentialRepository, EventBus,
    FeatureSetRepository, GatewayPortService, InboundMcpClientRepository,
    InstalledServerRepository, LogConfig as CoreLogConfig, OutboundOAuthRepository,
    ServerDiscoveryService, ServerFeatureRepository, ServerLogManager, SharedEventBus,
    SpaceBaseDirRepository, SpaceBuiltinConfigRepository, SpaceRepository, SpaceService,
    WorkspaceBindingRepository,
};
use mcpmux_gateway::{DependenciesBuilder, GatewayConfig, GatewayDependencies, GatewayServer};
use mcpmux_storage::{
    create_jwt_secret_provider, Database, FieldEncryptor, InboundClientRepository,
    JwtSecretProvider, KeychainJwtSecretProvider, SqliteAppSettingsRepository,
    SqliteCredentialRepository, SqliteFeatureSetRepository, SqliteInboundMcpClientRepository,
    SqliteInstalledServerRepository, SqliteOutboundOAuthRepository, SqliteServerFeatureRepository,
    SqliteSpaceBaseDirRepository, SqliteSpaceBuiltinConfigRepository, SqliteSpaceRepository,
    SqliteWorkspaceBindingRepository, DATABASE_FILE, JWT_SECRET_SIZE, KEY_SIZE,
};

#[cfg(windows)]
use mcpmux_storage::{create_key_provider, KeychainKeyProvider, MasterKeyProvider};

use crate::error::RuntimeError;
use crate::lock::DataDirLock;
use crate::master_key::KeySource;
#[cfg(not(windows))]
use crate::master_key::OsKeychain;
use crate::paths::resolve_data_dir;

/// Default registry API URL. Matches the desktop's hard-coded default and
/// the env-var fallback in `mcpmux_core::service::registry_api_client`.
pub const DEFAULT_REGISTRY_URL: &str = "https://api.mcpmux.com";

/// Per-file size cap for the server-log manager (10 MiB).
pub const DEFAULT_SERVER_LOG_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Number of rotated log files to retain.
pub const DEFAULT_SERVER_LOG_MAX_FILES: usize = 30;

/// Selection policy for the master-key provider. Matches the desktop's
/// current behaviour for `Auto`; explicit `Keychain` / `File` land in
/// Phase 2 (and already work at the storage layer — this enum just exposes
/// them through the runtime).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum KeyProviderPolicy {
    /// Platform default. DPAPI on Windows, OS keychain with file fallback
    /// on macOS / Linux.
    #[default]
    Auto,
    /// Require the OS keychain. Fails clearly when unavailable (e.g. on
    /// a headless host with no Secret Service).
    Keychain,
    /// Always use the file-backed provider. Skips OS keychain probing.
    File,
}

/// Configuration for [`RuntimeBuilder`]. All fields have sensible defaults
/// that match the desktop's pre-runtime behaviour.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Persistent data directory. `MCPMUX_DATA_DIR` env var overrides.
    pub data_dir: PathBuf,
    /// Optional override for the server-log directory. When `None`,
    /// `<data_dir>/logs` is used.
    pub log_dir: Option<PathBuf>,
    /// Registry API URL used by `ServerDiscoveryService`. `MCPMUX_REGISTRY_URL`
    /// env var overrides the default at construction time.
    pub registry_url: String,
    /// Which master-key provider to use.
    pub key_provider_policy: KeyProviderPolicy,
    /// Capacity for the shared event bus broadcast channel.
    pub event_bus_capacity: usize,
    /// Per-file size cap for the server-log manager.
    pub server_log_max_file_size: u64,
    /// Number of rotated log files to retain.
    pub server_log_max_files: usize,
    /// Whether the server-log manager gzips rotated files.
    pub server_log_compress: bool,
    /// Whether to load the JWT signing secret at bootstrap. When `false`,
    /// `Runtime::jwt_secret` is `None` and token signing stays disabled.
    pub load_jwt_secret: bool,
    /// How long to keep retrying while another process holds the data-dir
    /// lock. Zero (the default) fails immediately.
    pub lock_wait: Duration,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            data_dir: crate::paths::default_data_dir(),
            log_dir: None,
            registry_url: std::env::var("MCPMUX_REGISTRY_URL")
                .unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string()),
            key_provider_policy: KeyProviderPolicy::Auto,
            event_bus_capacity: 256,
            server_log_max_file_size: DEFAULT_SERVER_LOG_MAX_FILE_SIZE,
            server_log_max_files: DEFAULT_SERVER_LOG_MAX_FILES,
            server_log_compress: true,
            load_jwt_secret: true,
            lock_wait: Duration::ZERO,
        }
    }
}

/// Fluent builder for [`Runtime`]. Most callers only need `with_data_dir`.
#[derive(Debug, Default)]
pub struct RuntimeBuilder {
    config: RuntimeConfig,
}

impl RuntimeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(mut self, config: RuntimeConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_data_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.data_dir = dir.into();
        self
    }

    pub fn with_log_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.log_dir = Some(dir.into());
        self
    }

    pub fn with_registry_url(mut self, url: impl Into<String>) -> Self {
        self.config.registry_url = url.into();
        self
    }

    pub fn with_key_provider_policy(mut self, policy: KeyProviderPolicy) -> Self {
        self.config.key_provider_policy = policy;
        self
    }

    pub fn with_event_bus_capacity(mut self, capacity: usize) -> Self {
        self.config.event_bus_capacity = capacity;
        self
    }

    pub fn with_jwt_secret_loading(mut self, load: bool) -> Self {
        self.config.load_jwt_secret = load;
        self
    }

    /// Keep retrying for up to `wait` while another process holds the data
    /// directory lock (see [`DataDirLock::acquire_with_wait`]).
    pub fn with_lock_wait(mut self, wait: Duration) -> Self {
        self.config.lock_wait = wait;
        self
    }

    /// Run the bootstrap. Returns an `Arc<Runtime>` so both the desktop
    /// (which stores it in Tauri's state container) and the daemon (which
    /// awaits its own lifetime) can share one instance.
    pub async fn build(self) -> Result<Arc<Runtime>, RuntimeError> {
        let config = self.config;
        let data_dir = resolve_data_dir(Some(&config.data_dir))?;

        info!(path = %data_dir.display(), "[runtime] initialising");

        let lock = DataDirLock::acquire_with_wait(&data_dir, config.lock_wait).await?;

        // Master key. Failures here are always operator-visible: a missing
        // keychain on a headless host is exactly the kind of problem the
        // user needs to know about, not a silent fallback to a plaintext
        // secret (which the roadmap forbids — see Phase 2 acceptance).
        let (master_key, key_source) = master_key(&data_dir, config.key_provider_policy)
            .map_err(|e| RuntimeError::KeyProvider(e.to_string()))?;
        let encryptor = Arc::new(
            FieldEncryptor::new(&master_key)
                .map_err(|e| RuntimeError::KeyProvider(e.to_string()))?,
        );
        drop(master_key);

        // Database + migrations. Database::open runs all 22 SQL migrations
        // inside a transaction; the runtime does not introduce new schema.
        let db_path = data_dir.join(DATABASE_FILE);
        let database = Database::open(&db_path)
            .map_err(|e| RuntimeError::KeyProvider(format!("database open: {}", e)))?;
        let database = Arc::new(Mutex::new(database));

        let repositories = Repositories::new(database.clone(), encryptor.clone());

        // Server settings saved before they were encrypted at rest.
        match SqliteInstalledServerRepository::new(database.clone(), encryptor.clone())
            .encrypt_plaintext_rows()
            .await
        {
            Ok(0) => {}
            Ok(n) => info!(
                rows = n,
                "[runtime] encrypted server settings stored as plaintext"
            ),
            Err(e) => {
                tracing::warn!(error = %e, "[runtime] could not encrypt plaintext server settings")
            }
        }

        let app_settings_service =
            Arc::new(AppSettingsService::new(repositories.app_settings.clone()));
        let gateway_port_service =
            Arc::new(GatewayPortService::new(repositories.app_settings.clone()));

        let spaces_dir = data_dir.join("spaces");
        std::fs::create_dir_all(&spaces_dir).map_err(|source| RuntimeError::DataDirCreate {
            path: spaces_dir.clone(),
            source,
        })?;

        let server_discovery = Arc::new(
            ServerDiscoveryService::new(data_dir.clone(), spaces_dir.clone())
                .with_registry_api(config.registry_url.clone())
                .with_settings_service(app_settings_service.clone()),
        );

        let logs_dir = match &config.log_dir {
            Some(d) => d.clone(),
            None => data_dir.join("logs"),
        };
        std::fs::create_dir_all(&logs_dir).map_err(|source| RuntimeError::DataDirCreate {
            path: logs_dir.clone(),
            source,
        })?;

        let log_config = CoreLogConfig {
            base_dir: logs_dir.clone(),
            max_file_size: config.server_log_max_file_size,
            max_files: config.server_log_max_files,
            compress: config.server_log_compress,
        };
        let server_log_manager = Arc::new(ServerLogManager::new(log_config));

        let space_service = SpaceService::with_feature_set_repository(
            repositories.space.clone(),
            repositories.feature_set.clone(),
        );

        let jwt_secret = if config.load_jwt_secret {
            match load_jwt_secret(&data_dir, config.key_provider_policy, key_source) {
                Ok(secret) => {
                    info!("[runtime] JWT signing secret loaded");
                    Some(secret)
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "[runtime] JWT secret unavailable — token signing will be disabled"
                    );
                    None
                }
            }
        } else {
            None
        };

        let event_bus: SharedEventBus =
            Arc::new(EventBus::with_capacity(config.event_bus_capacity));

        let keys_dir = data_dir.join("keys");
        // key providers create this directory themselves on first use; no
        // need to mkdir here. The directory is exposed via Runtime::keys_dir
        // for diagnostics + Phase 2 permission checks.

        Ok(Arc::new(Runtime {
            config,
            data_dir,
            spaces_dir,
            keys_dir,
            logs_dir,
            db_path,
            lock,
            database,
            encryptor,
            repositories,
            app_settings_service,
            gateway_port_service,
            space_service,
            server_discovery,
            server_log_manager,
            jwt_secret,
            event_bus,
        }))
    }
}

/// Bundle of every stateful object the runtime owns. Exposed via
/// `Runtime::repositories` so callers can hand individual handles to
/// Tauri commands, CLI handlers, or tests.
pub struct Repositories {
    pub space: Arc<dyn SpaceRepository>,
    pub installed_server: Arc<dyn InstalledServerRepository>,
    pub credential: Arc<dyn CredentialRepository>,
    pub backend_oauth: Arc<dyn OutboundOAuthRepository>,
    pub feature_set: Arc<dyn FeatureSetRepository>,
    pub client: Arc<dyn InboundMcpClientRepository>,
    pub workspace_binding: Arc<dyn WorkspaceBindingRepository>,
    pub space_base_dir: Arc<dyn SpaceBaseDirRepository>,
    pub space_builtin_config: Arc<dyn SpaceBuiltinConfigRepository>,
    pub server_feature: Arc<SqliteServerFeatureRepository>,
    pub server_feature_core: Arc<dyn ServerFeatureRepository>,
    pub app_settings: Arc<dyn AppSettingsRepository>,
}

impl Repositories {
    fn new(db: Arc<Mutex<Database>>, encryptor: Arc<FieldEncryptor>) -> Self {
        let space: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
        let installed_server: Arc<dyn InstalledServerRepository> = Arc::new(
            SqliteInstalledServerRepository::new(db.clone(), encryptor.clone()),
        );
        let credential: Arc<dyn CredentialRepository> = Arc::new(SqliteCredentialRepository::new(
            db.clone(),
            encryptor.clone(),
        ));
        let backend_oauth: Arc<dyn OutboundOAuthRepository> = Arc::new(
            SqliteOutboundOAuthRepository::new(db.clone(), encryptor.clone()),
        );
        let feature_set: Arc<dyn FeatureSetRepository> =
            Arc::new(SqliteFeatureSetRepository::new(db.clone()));
        let client: Arc<dyn InboundMcpClientRepository> =
            Arc::new(SqliteInboundMcpClientRepository::new(db.clone()));
        let workspace_binding: Arc<dyn WorkspaceBindingRepository> =
            Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
        let space_base_dir: Arc<dyn SpaceBaseDirRepository> =
            Arc::new(SqliteSpaceBaseDirRepository::new(db.clone()));
        let space_builtin_config: Arc<dyn SpaceBuiltinConfigRepository> =
            Arc::new(SqliteSpaceBuiltinConfigRepository::new(db.clone()));
        let server_feature = Arc::new(SqliteServerFeatureRepository::new(db.clone()));
        let server_feature_core: Arc<dyn ServerFeatureRepository> = server_feature.clone();
        let app_settings: Arc<dyn AppSettingsRepository> =
            Arc::new(SqliteAppSettingsRepository::new(db.clone()));

        Self {
            space,
            installed_server,
            credential,
            backend_oauth,
            feature_set,
            client,
            workspace_binding,
            space_base_dir,
            space_builtin_config,
            server_feature,
            server_feature_core,
            app_settings,
        }
    }
}

/// The fully-bootstrapped runtime. Wrap in `Arc` before storing in Tauri
/// state or passing across the daemon's main task boundary.
pub struct Runtime {
    pub config: RuntimeConfig,
    pub data_dir: PathBuf,
    pub spaces_dir: PathBuf,
    pub keys_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub db_path: PathBuf,
    pub lock: DataDirLock,
    pub database: Arc<Mutex<Database>>,
    pub encryptor: Arc<FieldEncryptor>,
    pub repositories: Repositories,
    pub app_settings_service: Arc<AppSettingsService>,
    pub gateway_port_service: Arc<GatewayPortService>,
    pub space_service: SpaceService,
    pub server_discovery: Arc<ServerDiscoveryService>,
    pub server_log_manager: Arc<ServerLogManager>,
    pub jwt_secret: Option<Zeroizing<[u8; JWT_SECRET_SIZE]>>,
    pub event_bus: SharedEventBus,
}

impl Runtime {
    /// Subscribe to the shared event bus. Receives every event the gateway
    /// emits AND every event any future `*AppService` writes through the
    /// shared bus. The desktop's domain-event bridge should be re-pointed
    /// here in a follow-up; today it still reads the gateway's broadcast
    /// channel directly.
    pub fn subscribe_events(&self) -> mcpmux_core::EventReceiver {
        self.event_bus.subscribe()
    }

    /// Build a [`GatewayDependencies`] from this runtime's repositories
    /// and the loaded JWT secret. Reusable for the desktop's
    /// `start_gateway` Tauri command and the daemon's gateway boot.
    pub fn build_gateway_dependencies(&self) -> Result<GatewayDependencies, RuntimeError> {
        let inbound_client_repo = Arc::new(InboundClientRepository::new(self.database.clone()));

        let mut builder = DependenciesBuilder::new()
            .with_installed_server_repo(self.repositories.installed_server.clone())
            .with_credential_repo(self.repositories.credential.clone())
            .with_backend_oauth_repo(self.repositories.backend_oauth.clone())
            .with_feature_repo(self.repositories.server_feature_core.clone())
            .with_feature_set_repo(self.repositories.feature_set.clone())
            .with_server_discovery(self.server_discovery.clone())
            .with_log_manager(self.server_log_manager.clone())
            .with_database(self.database.clone())
            .with_state_dir(self.data_dir.clone())
            .with_settings_repo(self.repositories.app_settings.clone());

        if let Some(secret) = &self.jwt_secret {
            builder = builder.with_jwt_secret(secret.clone());
        }

        let deps = builder.build().map_err(RuntimeError::GatewayDeps)?;

        // The builder auto-creates these from the database, but we want
        // the concrete `InboundClientRepository` the runtime owns and the
        // `SpaceRepository` the gateway already shares with the rest of
        // the codebase (instead of a second handle wired only into the
        // gateway).
        Ok(GatewayDependencies {
            space_repo: self.repositories.space.clone(),
            inbound_client_repo,
            ..deps
        })
    }

    /// Build a [`GatewayServer`] ready to be spawned via `server.spawn()`.
    ///
    /// `async` because `GatewayServer::new` performs a `block_in_place`
    /// internally and must run inside a Tokio runtime. Callers without a
    /// current runtime should wrap the call in a `tokio::runtime::Runtime`
    /// or use the daemon's `#[tokio::main]`.
    pub async fn build_gateway_server(
        &self,
        config: GatewayConfig,
    ) -> Result<GatewayServer, RuntimeError> {
        let deps = self.build_gateway_dependencies()?;
        Ok(GatewayServer::new(config, deps))
    }

    /// Pre-built default [`GatewayConfig`] (loopback + default port, no
    /// public base URL, CORS on). The daemon and the desktop both start
    /// from this and override what they need.
    pub fn default_gateway_config(&self) -> GatewayConfig {
        GatewayConfig::default()
    }
}

/// The master key and where it came from (`None` on Windows, where DPAPI
/// keeps it next to the data and there is nothing to choose between).
#[cfg(not(windows))]
fn master_key(
    data_dir: &Path,
    policy: KeyProviderPolicy,
) -> anyhow::Result<(Zeroizing<[u8; KEY_SIZE]>, Option<KeySource>)> {
    let allowed: &[KeySource] = match policy {
        KeyProviderPolicy::Auto => &[KeySource::Keychain, KeySource::File],
        KeyProviderPolicy::Keychain => &[KeySource::Keychain],
        KeyProviderPolicy::File => &[KeySource::File],
    };
    // A short-lived handle: deciding needs the stored ciphertexts before the
    // repositories (which need the key) exist.
    let database = Database::open(&data_dir.join(DATABASE_FILE))?;
    let (key, source) =
        crate::master_key::resolve_master_key(data_dir, &database, allowed, &OsKeychain)?;
    Ok((key, Some(source)))
}

#[cfg(windows)]
fn master_key(
    data_dir: &Path,
    policy: KeyProviderPolicy,
) -> anyhow::Result<(Zeroizing<[u8; KEY_SIZE]>, Option<KeySource>)> {
    let provider: Box<dyn MasterKeyProvider> = match policy {
        KeyProviderPolicy::Auto => create_key_provider(data_dir)?,
        KeyProviderPolicy::Keychain => Box::new(KeychainKeyProvider::new()?),
        KeyProviderPolicy::File => file_key_provider(data_dir)?,
    };
    // Never create a new key while stored data needs the old one.
    let database = Database::open(&data_dir.join(DATABASE_FILE))?;
    let samples = database.encrypted_samples(crate::master_key::SAMPLE_LIMIT)?;
    let existing = if provider.key_exists() {
        Some(provider.get_or_create_key()?)
    } else {
        None
    };
    let key =
        crate::master_key::guard_single_key(existing, &samples, || provider.get_or_create_key())?;
    crate::master_key::warn_if_partly_unreadable(&key, &samples);
    Ok((key, None))
}

#[cfg(windows)]
fn file_key_provider(_data_dir: &Path) -> anyhow::Result<Box<dyn MasterKeyProvider>> {
    anyhow::bail!("the file key provider is not supported on Windows; use auto or keychain")
}

/// The JWT signing secret lives wherever the master key does, so a keychain
/// hiccup can't silently switch it to a new file-based secret either.
fn load_jwt_secret(
    data_dir: &Path,
    policy: KeyProviderPolicy,
    master_key_source: Option<KeySource>,
) -> anyhow::Result<Zeroizing<[u8; JWT_SECRET_SIZE]>> {
    let provider: Box<dyn JwtSecretProvider> = match (master_key_source, policy) {
        (Some(KeySource::Keychain), _) => Box::new(KeychainJwtSecretProvider::new()?),
        (Some(KeySource::File), _) => file_jwt_secret_provider(data_dir)?,
        (None, KeyProviderPolicy::Auto) => create_jwt_secret_provider(data_dir)?,
        (None, KeyProviderPolicy::Keychain) => Box::new(KeychainJwtSecretProvider::new()?),
        (None, KeyProviderPolicy::File) => file_jwt_secret_provider(data_dir)?,
    };
    provider
        .get_or_create_secret()
        .map_err(|e| anyhow::anyhow!("jwt secret: {}", e))
}

#[cfg(not(windows))]
fn file_jwt_secret_provider(data_dir: &Path) -> anyhow::Result<Box<dyn JwtSecretProvider>> {
    Ok(Box::new(mcpmux_storage::FileJwtSecretProvider::new(
        data_dir,
    )?))
}

#[cfg(windows)]
fn file_jwt_secret_provider(_data_dir: &Path) -> anyhow::Result<Box<dyn JwtSecretProvider>> {
    anyhow::bail!("the file JWT provider is not supported on Windows; use auto or keychain")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_url_matches_desktop() {
        // The desktop reads MCPMUX_REGISTRY_URL with this fallback; keep
        // the runtime aligned so the daemon ships the same default.
        assert_eq!(DEFAULT_REGISTRY_URL, "https://api.mcpmux.com");
    }

    #[test]
    fn runtime_config_default_has_no_log_dir_override() {
        let cfg = RuntimeConfig::default();
        assert!(cfg.log_dir.is_none());
        assert!(cfg.load_jwt_secret);
        assert_eq!(cfg.key_provider_policy, KeyProviderPolicy::Auto);
    }

    #[test]
    fn builder_with_methods_chain() {
        let cfg = RuntimeBuilder::new()
            .with_data_dir("/tmp/foo")
            .with_registry_url("http://example.test")
            .with_key_provider_policy(KeyProviderPolicy::File)
            .with_event_bus_capacity(1024)
            .config;
        assert_eq!(cfg.data_dir, PathBuf::from("/tmp/foo"));
        assert_eq!(cfg.registry_url, "http://example.test");
        assert_eq!(cfg.key_provider_policy, KeyProviderPolicy::File);
        assert_eq!(cfg.event_bus_capacity, 1024);
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn file_policy_uses_file_backed_master_and_jwt_keys() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = RuntimeBuilder::new()
            .with_data_dir(dir.path())
            .with_key_provider_policy(KeyProviderPolicy::File)
            .build()
            .await
            .unwrap();

        assert!(runtime.keys_dir.join("master.key").is_file());
        assert!(runtime.keys_dir.join("jwt.key").is_file());
    }
}
