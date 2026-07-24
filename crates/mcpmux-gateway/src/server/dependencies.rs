//! Dependency Injection Container
//!
//! Provides a clean DI pattern for Gateway dependencies.
//! Makes testing easier and dependencies explicit.

use std::path::PathBuf;
use std::sync::Arc;

use crate::services::ClientMetadataService;
use mcpmux_core::{
    AppSettingsRepository, CimdMetadataFetcher, CredentialRepository, EventBus,
    FeatureSetRepository, InboundMcpClientRepository, InstalledServerRepository,
    OutboundOAuthRepository, ServerDiscoveryService, ServerFeatureRepository, ServerLogManager,
    SpaceBaseDirRepository, SpaceBuiltinConfigRepository, SpaceRepository,
    WorkspaceBindingRepository,
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
    /// Trait-based MCP client repository (for Client entity CRUD + pin setters).
    ///
    /// Used by the FeatureSet resolver v2 — separate from `inbound_client_repo`
    /// (which is the concrete OAuth-flow-focused repo).
    pub inbound_mcp_client_repo: Arc<dyn InboundMcpClientRepository>,
    /// Workspace -> FeatureSet bindings for resolver v2.
    pub workspace_binding_repo: Arc<dyn WorkspaceBindingRepository>,
    /// Per-Space base directories — scope a reported workspace root to a Space
    /// by folder prefix (longest match wins).
    pub space_base_dir_repo: Arc<dyn SpaceBaseDirRepository>,
    /// Per-Space built-in server config (Tool Optimization enablement + tool
    /// toggles), consulted when advertising the `mcpmux_*` tools per Space.
    pub builtin_config_repo: Arc<dyn SpaceBuiltinConfigRepository>,

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
    /// Application event bus (shared with desktop ApplicationServices)
    pub event_bus: Option<Arc<EventBus>>,
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
        // Resolver v2 repositories — always SQLite-backed; no-op at runtime
        // until the resolver flag flips out of shadow mode.
        let inbound_mcp_client_repo: Arc<dyn InboundMcpClientRepository> = Arc::new(
            mcpmux_storage::SqliteInboundMcpClientRepository::new(database.clone()),
        );
        let workspace_binding_repo: Arc<dyn WorkspaceBindingRepository> = Arc::new(
            mcpmux_storage::SqliteWorkspaceBindingRepository::new(database.clone()),
        );
        let space_base_dir_repo: Arc<dyn SpaceBaseDirRepository> = Arc::new(
            mcpmux_storage::SqliteSpaceBaseDirRepository::new(database.clone()),
        );
        let builtin_config_repo: Arc<dyn SpaceBuiltinConfigRepository> = Arc::new(
            mcpmux_storage::SqliteSpaceBuiltinConfigRepository::new(database.clone()),
        );

        Self {
            installed_server_repo,
            credential_repo,
            backend_oauth_repo,
            feature_repo,
            feature_set_repo,
            space_repo,
            inbound_client_repo,
            inbound_mcp_client_repo,
            workspace_binding_repo,
            space_base_dir_repo,
            builtin_config_repo,
            server_discovery,
            log_manager,
            cimd_fetcher,
            client_metadata_service,
            database,
            jwt_secret,
            state_dir,
            settings_repo: None, // Use builder for this
            event_bus: None,
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
    event_bus: Option<Arc<EventBus>>,
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
            event_bus: None,
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

    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
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

        // Resolver v2 repositories — always SQLite-backed for now.
        let inbound_mcp_client_repo: Arc<dyn InboundMcpClientRepository> = Arc::new(
            mcpmux_storage::SqliteInboundMcpClientRepository::new(database.clone()),
        );
        let workspace_binding_repo: Arc<dyn WorkspaceBindingRepository> = Arc::new(
            mcpmux_storage::SqliteWorkspaceBindingRepository::new(database.clone()),
        );
        let space_base_dir_repo: Arc<dyn SpaceBaseDirRepository> = Arc::new(
            mcpmux_storage::SqliteSpaceBaseDirRepository::new(database.clone()),
        );
        let builtin_config_repo: Arc<dyn SpaceBuiltinConfigRepository> = Arc::new(
            mcpmux_storage::SqliteSpaceBuiltinConfigRepository::new(database.clone()),
        );

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
            inbound_mcp_client_repo,
            workspace_binding_repo,
            space_base_dir_repo,
            builtin_config_repo,
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
            event_bus: self.event_bus,
        })
    }
}

impl Default for DependenciesBuilder {
    fn default() -> Self {
        Self::new()
    }
}
