//! Dependency Injection Container
//!
//! Provides a clean DI pattern for Gateway dependencies.
//! Makes testing easier and dependencies explicit.

use std::path::PathBuf;
use std::sync::Arc;

use crate::services::ClientMetadataService;
use mcpmux_core::{
    AppSettingsRepository, CimdMetadataFetcher, CredentialRepository, FeatureSetRepository,
    InstalledServerRepository, OutboundOAuthRepository, ServerDiscoveryService,
    ServerFeatureRepository, ServerLogManager, SpaceRepository,
};
use mcpmux_storage::{Database, InboundClientRepository};
use tokio::sync::Mutex;

/// Dependency container for Gateway
///
/// Follows Dependency Injection pattern - all dependencies are injected,
/// making the Gateway testable and decoupled from concrete implementations.
#[derive(Clone)]
pub struct GatewayDependencies {
    // Repositories (Data Layer)
    pub installed_server_repo: Arc<dyn InstalledServerRepository>,
    pub credential_repo: Arc<dyn CredentialRepository>,
    pub backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
    pub feature_repo: Arc<dyn ServerFeatureRepository>,
    pub feature_set_repo: Arc<dyn FeatureSetRepository>,
    pub space_repo: Arc<dyn SpaceRepository>,
    pub inbound_client_repo: Arc<InboundClientRepository>,

    // Services (Business Layer)
    pub server_discovery: Arc<ServerDiscoveryService>,
    pub log_manager: Arc<ServerLogManager>,
    pub cimd_fetcher: Arc<CimdMetadataFetcher>,
    pub client_metadata_service: Arc<ClientMetadataService>,

    // Database (for Gateway state persistence)
    pub database: Arc<Mutex<Database>>,

    // JWT signing secret (optional, for token issuance)
    pub jwt_secret: Option<zeroize::Zeroizing<[u8; mcpmux_storage::JWT_SECRET_SIZE]>>,
    /// Base directory for transport state (optional)
    pub state_dir: Option<PathBuf>,
    /// App settings repository (for OAuth port persistence)
    pub settings_repo: Option<Arc<dyn AppSettingsRepository>>,
}

impl GatewayDependencies {
    /// Create a new dependency container
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        installed_server_repo: Arc<dyn InstalledServerRepository>,
        credential_repo: Arc<dyn CredentialRepository>,
        backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
        feature_repo: Arc<dyn ServerFeatureRepository>,
        feature_set_repo: Arc<dyn FeatureSetRepository>,
        space_repo: Arc<dyn SpaceRepository>,
        inbound_client_repo: Arc<InboundClientRepository>,
        server_discovery: Arc<ServerDiscoveryService>,
        log_manager: Arc<ServerLogManager>,
        cimd_fetcher: Arc<CimdMetadataFetcher>,
        client_metadata_service: Arc<ClientMetadataService>,
        database: Arc<Mutex<Database>>,
        jwt_secret: Option<zeroize::Zeroizing<[u8; mcpmux_storage::JWT_SECRET_SIZE]>>,
        state_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            installed_server_repo,
            credential_repo,
            backend_oauth_repo,
            feature_repo,
            feature_set_repo,
            space_repo,
            inbound_client_repo,
            server_discovery,
            log_manager,
            cimd_fetcher,
            client_metadata_service,
            database,
            jwt_secret,
            state_dir,
            settings_repo: None, // Use builder for this
        }
    }
}

/// Builder for GatewayDependencies
pub struct DependenciesBuilder {
    installed_server_repo: Option<Arc<dyn InstalledServerRepository>>,
    credential_repo: Option<Arc<dyn CredentialRepository>>,
    backend_oauth_repo: Option<Arc<dyn OutboundOAuthRepository>>,
    feature_repo: Option<Arc<dyn ServerFeatureRepository>>,
    feature_set_repo: Option<Arc<dyn FeatureSetRepository>>,
    space_repo: Option<Arc<dyn SpaceRepository>>,
    inbound_client_repo: Option<Arc<InboundClientRepository>>,
    server_discovery: Option<Arc<ServerDiscoveryService>>,
    log_manager: Option<Arc<ServerLogManager>>,
    cimd_fetcher: Option<Arc<CimdMetadataFetcher>>,
    client_metadata_service: Option<Arc<ClientMetadataService>>,
    database: Option<Arc<Mutex<Database>>>,
    jwt_secret: Option<zeroize::Zeroizing<[u8; mcpmux_storage::JWT_SECRET_SIZE]>>,
    state_dir: Option<PathBuf>,
    settings_repo: Option<Arc<dyn AppSettingsRepository>>,
}

impl DependenciesBuilder {
    pub fn new() -> Self {
        Self {
            installed_server_repo: None,
            credential_repo: None,
            backend_oauth_repo: None,
            feature_repo: None,
            feature_set_repo: None,
            space_repo: None,
            inbound_client_repo: None,
            server_discovery: None,
            log_manager: None,
            cimd_fetcher: None,
            client_metadata_service: None,
            database: None,
            jwt_secret: None,
            state_dir: None,
            settings_repo: None,
        }
    }

    pub fn with_installed_server_repo(mut self, repo: Arc<dyn InstalledServerRepository>) -> Self {
        self.installed_server_repo = Some(repo);
        self
    }

    pub fn with_credential_repo(mut self, repo: Arc<dyn CredentialRepository>) -> Self {
        self.credential_repo = Some(repo);
        self
    }

    pub fn with_backend_oauth_repo(mut self, repo: Arc<dyn OutboundOAuthRepository>) -> Self {
        self.backend_oauth_repo = Some(repo);
        self
    }

    pub fn with_feature_repo(mut self, repo: Arc<dyn ServerFeatureRepository>) -> Self {
        self.feature_repo = Some(repo);
        self
    }

    pub fn with_feature_set_repo(mut self, repo: Arc<dyn FeatureSetRepository>) -> Self {
        self.feature_set_repo = Some(repo);
        self
    }

    pub fn with_server_discovery(mut self, service: Arc<ServerDiscoveryService>) -> Self {
        self.server_discovery = Some(service);
        self
    }

    pub fn with_log_manager(mut self, manager: Arc<ServerLogManager>) -> Self {
        self.log_manager = Some(manager);
        self
    }

    pub fn with_database(mut self, db: Arc<Mutex<Database>>) -> Self {
        self.database = Some(db);
        self
    }

    pub fn with_jwt_secret(
        mut self,
        secret: zeroize::Zeroizing<[u8; mcpmux_storage::JWT_SECRET_SIZE]>,
    ) -> Self {
        self.jwt_secret = Some(secret);
        self
    }

    pub fn with_state_dir(mut self, state_dir: PathBuf) -> Self {
        self.state_dir = Some(state_dir);
        self
    }

    pub fn with_settings_repo(mut self, repo: Arc<dyn AppSettingsRepository>) -> Self {
        self.settings_repo = Some(repo);
        self
    }

    pub fn build(self) -> Result<GatewayDependencies, String> {
        let database = self.database.ok_or("database is required")?;

        // Create CIMD fetcher if not provided
        let cimd_fetcher = self
            .cimd_fetcher
            .unwrap_or_else(|| Arc::new(CimdMetadataFetcher::default()));

        // Create ClientMetadataService if not provided
        let client_metadata_service = self.client_metadata_service.unwrap_or_else(|| {
            let inbound_client_repo = Arc::new(mcpmux_storage::InboundClientRepository::new(
                database.clone(),
            ));
            Arc::new(ClientMetadataService::new(
                inbound_client_repo,
                cimd_fetcher.clone(),
            ))
        });

        // Create repositories from database if not provided
        let space_repo = self.space_repo.unwrap_or_else(|| {
            Arc::new(mcpmux_storage::SqliteSpaceRepository::new(database.clone()))
        });

        let inbound_client_repo = self.inbound_client_repo.unwrap_or_else(|| {
            Arc::new(mcpmux_storage::InboundClientRepository::new(
                database.clone(),
            ))
        });

        Ok(GatewayDependencies {
            installed_server_repo: self
                .installed_server_repo
                .ok_or("installed_server_repo is required")?,
            credential_repo: self.credential_repo.ok_or("credential_repo is required")?,
            backend_oauth_repo: self
                .backend_oauth_repo
                .ok_or("backend_oauth_repo is required")?,
            feature_repo: self.feature_repo.ok_or("feature_repo is required")?,
            feature_set_repo: self
                .feature_set_repo
                .ok_or("feature_set_repo is required")?,
            space_repo,
            inbound_client_repo,
            server_discovery: self
                .server_discovery
                .ok_or("server_discovery is required")?,
            log_manager: self.log_manager.ok_or("log_manager is required")?,
            cimd_fetcher,
            client_metadata_service,
            database,
            jwt_secret: self.jwt_secret,
            state_dir: self.state_dir,
            settings_repo: self.settings_repo,
        })
    }
}

impl Default for DependenciesBuilder {
    fn default() -> Self {
        Self::new()
    }
}
