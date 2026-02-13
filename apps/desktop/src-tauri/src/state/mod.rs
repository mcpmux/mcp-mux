//! Application state management.
//!
//! This module contains the global application state that is shared
//! between Tauri commands.

use mcpmux_core::{
    AppSettingsRepository, AppSettingsService, ClientService, CredentialRepository,
    FeatureSetRepository, GatewayPortService, InboundMcpClientRepository,
    InstalledServerRepository, LogConfig, OutboundOAuthRepository, ServerDiscoveryService,
    ServerFeatureRepository as CoreServerFeatureRepository, ServerLogManager, SpaceRepository,
    SpaceService,
};
use mcpmux_storage::{
    Database, FieldEncryptor, SqliteAppSettingsRepository, SqliteCredentialRepository,
    SqliteFeatureSetRepository, SqliteInboundMcpClientRepository, SqliteInstalledServerRepository,
    SqliteOutboundOAuthRepository, SqliteServerFeatureRepository, SqliteSpaceRepository,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

/// Global application state accessible from commands.
pub struct AppState {
    /// Base data directory for the app
    data_dir: PathBuf,
    /// Directory for space configuration files
    spaces_dir: PathBuf,
    /// App settings repository (for direct access when needed)
    pub settings_repository: Arc<dyn AppSettingsRepository>,
    /// Gateway port service (handles port resolution with settings)
    pub gateway_port_service: Arc<GatewayPortService>,
    /// Service for managing spaces
    pub space_service: SpaceService,
    /// Service for managing clients (auto-grants, etc.)
    pub client_service: ClientService,
    /// Server discovery service for loading servers from API/bundled/user spaces
    pub server_discovery: Arc<ServerDiscoveryService>,
    /// Server log manager for file-based logging
    pub server_log_manager: Arc<ServerLogManager>,
    /// Installed server repository (per-space installations)
    pub installed_server_repository: Arc<dyn InstalledServerRepository>,
    /// Credential repository (with encryption)
    pub credential_repository: Arc<dyn CredentialRepository>,
    /// Backend OAuth repository (our DCR with remote MCP servers)
    pub backend_oauth_repository: Arc<dyn OutboundOAuthRepository>,
    /// FeatureSet repository for permission bundles
    pub feature_set_repository: Arc<dyn FeatureSetRepository>,
    /// Client repository for AI clients
    pub client_repository: Arc<dyn InboundMcpClientRepository>,
    /// Server feature repository for discovered MCP features (implements core trait)
    pub server_feature_repository: Arc<SqliteServerFeatureRepository>,
    /// Server feature repository cast to core trait (for gateway services)
    pub server_feature_repository_core: Arc<dyn CoreServerFeatureRepository>,
    /// Field encryptor (used for credential repository creation)
    #[allow(dead_code)]
    pub encryptor: Arc<FieldEncryptor>,
    /// Shared database connection (kept alive for the app lifetime)
    #[allow(dead_code)]
    db: Arc<Mutex<Database>>,
}

impl AppState {
    /// Create a new application state with the given data directory.
    pub fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        // Ensure data directory exists
        std::fs::create_dir_all(&data_dir)?;

        // Get or create master key (DPAPI on Windows, OS Keychain elsewhere)
        info!("Retrieving master key...");
        let key_provider = mcpmux_storage::create_key_provider(&data_dir)?;
        let master_key = key_provider.get_or_create_key()?;
        info!("Master key retrieved successfully");

        // Create field encryptor
        let encryptor = Arc::new(FieldEncryptor::new(&master_key)?);

        // Open database
        let db_path = data_dir.join("mcpmux.db");
        info!("Opening database at {:?}", db_path);

        let db = Database::open(&db_path)?;
        let db = Arc::new(Mutex::new(db));

        // Initialize repositories
        let space_repository: Arc<dyn SpaceRepository> =
            Arc::new(SqliteSpaceRepository::new(db.clone()));

        let installed_server_repository: Arc<dyn InstalledServerRepository> = Arc::new(
            SqliteInstalledServerRepository::new(db.clone(), encryptor.clone()),
        );

        let credential_repository: Arc<dyn CredentialRepository> = Arc::new(
            SqliteCredentialRepository::new(db.clone(), encryptor.clone()),
        );

        let backend_oauth_repository: Arc<dyn OutboundOAuthRepository> =
            Arc::new(SqliteOutboundOAuthRepository::new(db.clone()));

        let feature_set_repository: Arc<dyn FeatureSetRepository> =
            Arc::new(SqliteFeatureSetRepository::new(db.clone()));

        let client_repository: Arc<dyn InboundMcpClientRepository> =
            Arc::new(SqliteInboundMcpClientRepository::new(db.clone()));

        let server_feature_repository = Arc::new(SqliteServerFeatureRepository::new(db.clone()));
        let server_feature_repository_core: Arc<dyn CoreServerFeatureRepository> =
            server_feature_repository.clone();

        // Create app settings repository and services
        let settings_repository: Arc<dyn AppSettingsRepository> =
            Arc::new(SqliteAppSettingsRepository::new(db.clone()));
        let settings_service = Arc::new(AppSettingsService::new(settings_repository.clone()));
        let gateway_port_service = Arc::new(GatewayPortService::new(settings_repository.clone()));

        // Create services
        let space_service = SpaceService::with_feature_set_repository(
            space_repository,
            feature_set_repository.clone(),
        );
        let client_service =
            ClientService::new(client_repository.clone(), feature_set_repository.clone());

        // Create server discovery service
        // Spaces directory is relative to app data_dir (single source of truth)
        let spaces_dir = data_dir.join("spaces");
        std::fs::create_dir_all(&spaces_dir)?;
        info!("Using spaces directory: {:?}", spaces_dir);

        let registry_url = std::env::var("MCPMUX_REGISTRY_URL")
            .unwrap_or_else(|_| "https://api.mcpmux.com".to_string());
        info!("Using Registry API URL: {}", registry_url);

        let server_discovery = Arc::new(
            ServerDiscoveryService::new(data_dir.clone(), spaces_dir.clone())
                .with_registry_api(registry_url)
                .with_settings_service(settings_service),
        );

        // Create server log manager
        let log_config = LogConfig {
            base_dir: data_dir.join("logs"),
            max_file_size: 10 * 1024 * 1024, // 10MB
            max_files: 30,                   // 30 files
            compress: true,
        };
        let server_log_manager = Arc::new(ServerLogManager::new(log_config));

        info!("Application state initialized successfully");

        Ok(Self {
            data_dir,
            spaces_dir,
            settings_repository,
            gateway_port_service,
            space_service,
            client_service,
            server_discovery,
            server_log_manager,
            installed_server_repository,
            credential_repository,
            backend_oauth_repository,
            feature_set_repository,
            client_repository,
            server_feature_repository,
            server_feature_repository_core,
            encryptor,
            db,
        })
    }

    /// Get the shared database connection for use by other components (e.g., gateway)
    pub fn database(&self) -> Arc<Mutex<Database>> {
        self.db.clone()
    }

    /// Get the base data directory for the app
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// Get the spaces configuration directory
    #[allow(dead_code)]
    pub fn spaces_dir(&self) -> &std::path::Path {
        &self.spaces_dir
    }

    /// Get the path to a specific space's config file
    pub fn space_config_path(&self, space_id: &str) -> PathBuf {
        mcpmux_core::get_space_config_path(&self.spaces_dir, space_id)
    }
}
