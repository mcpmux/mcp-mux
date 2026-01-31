//! Repository implementations using SQLite.

mod app_settings_repository;
mod credential_repository;
mod feature_set_repository;
mod inbound_client_repository;
mod inbound_mcp_client_repository;
mod installed_server_repository;
mod outbound_oauth_client_repository;
mod server_feature_repository;
mod space_repository;

pub use app_settings_repository::SqliteAppSettingsRepository;
pub use credential_repository::SqliteCredentialRepository;
pub use feature_set_repository::SqliteFeatureSetRepository;
pub use inbound_client_repository::{
    InboundClientRepository, InboundClient, RegistrationType,
    AuthorizationCode, TokenRecord, TokenType,
};
pub use inbound_mcp_client_repository::SqliteInboundMcpClientRepository;
pub use installed_server_repository::SqliteInstalledServerRepository;
pub use outbound_oauth_client_repository::SqliteOutboundOAuthRepository;
pub use server_feature_repository::{
    FeatureType, ServerFeature, ServerFeatureRepository, SqliteServerFeatureRepository,
};
pub use space_repository::SqliteSpaceRepository;
