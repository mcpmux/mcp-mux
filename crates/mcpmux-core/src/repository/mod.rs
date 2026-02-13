//! Repository traits for data access
//!
//! These traits define the interface for data storage without specifying
//! the implementation (SQLite, in-memory, etc.)

use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::{
    Client, Credential, CredentialType, FeatureSet, FeatureSetMember, InstalledServer, MemberMode,
    OutboundOAuthRegistration, ServerFeature, Space,
};

/// Result type for repository operations
pub type RepoResult<T> = anyhow::Result<T>;

/// Space repository trait
#[async_trait]
pub trait SpaceRepository: Send + Sync {
    /// Get all spaces
    async fn list(&self) -> RepoResult<Vec<Space>>;

    /// Get a space by ID
    async fn get(&self, id: &Uuid) -> RepoResult<Option<Space>>;

    /// Create a new space
    async fn create(&self, space: &Space) -> RepoResult<()>;

    /// Update a space
    async fn update(&self, space: &Space) -> RepoResult<()>;

    /// Delete a space
    async fn delete(&self, id: &Uuid) -> RepoResult<()>;

    /// Get the default space
    async fn get_default(&self) -> RepoResult<Option<Space>>;

    /// Set a space as default
    async fn set_default(&self, id: &Uuid) -> RepoResult<()>;
}

/// InstalledServer repository trait
#[async_trait]
pub trait InstalledServerRepository: Send + Sync {
    /// Get all installed servers
    async fn list(&self) -> RepoResult<Vec<InstalledServer>>;

    /// Get installed servers for a space
    async fn list_for_space(&self, space_id: &str) -> RepoResult<Vec<InstalledServer>>;

    /// Get all servers installed from a specific source file
    async fn list_by_source_file(
        &self,
        file_path: &std::path::Path,
    ) -> RepoResult<Vec<InstalledServer>>;

    /// Get an installed server by ID
    async fn get(&self, id: &Uuid) -> RepoResult<Option<InstalledServer>>;

    /// Get an installed server by space and registry server ID
    async fn get_by_server_id(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> RepoResult<Option<InstalledServer>>;

    /// Install a server (create)
    async fn install(&self, server: &InstalledServer) -> RepoResult<()>;

    /// Update an installed server
    async fn update(&self, server: &InstalledServer) -> RepoResult<()>;

    /// Uninstall a server (delete)
    async fn uninstall(&self, id: &Uuid) -> RepoResult<()>;

    /// Get enabled servers for a space
    async fn list_enabled(&self, space_id: &str) -> RepoResult<Vec<InstalledServer>>;

    /// Get all enabled servers across all spaces
    async fn list_enabled_all(&self) -> RepoResult<Vec<InstalledServer>>;

    /// Set enabled state
    async fn set_enabled(&self, id: &Uuid, enabled: bool) -> RepoResult<()>;

    /// Set OAuth connected status
    async fn set_oauth_connected(&self, id: &Uuid, connected: bool) -> RepoResult<()>;

    /// Update input values for a server
    async fn update_inputs(
        &self,
        id: &Uuid,
        input_values: std::collections::HashMap<String, String>,
    ) -> RepoResult<()>;

    /// Update the cached definition for an existing server (used during sync)
    async fn update_cached_definition(
        &self,
        id: &Uuid,
        server_name: Option<String>,
        cached_definition: Option<String>,
    ) -> RepoResult<()>;
}

/// ServerFeature repository trait
#[async_trait]
pub trait ServerFeatureRepository: Send + Sync {
    /// List all features for a space
    async fn list_for_space(&self, space_id: &str) -> RepoResult<Vec<ServerFeature>>;

    /// List features for a specific server in a space
    async fn list_for_server(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> RepoResult<Vec<ServerFeature>>;

    /// Get a feature by ID
    async fn get(&self, id: &Uuid) -> RepoResult<Option<ServerFeature>>;

    /// Upsert a feature (create or update)
    async fn upsert(&self, feature: &ServerFeature) -> RepoResult<()>;

    /// Bulk upsert features
    async fn upsert_many(&self, features: &[ServerFeature]) -> RepoResult<()>;

    /// Delete a feature
    async fn delete(&self, id: &Uuid) -> RepoResult<()>;

    /// Mark all features for a server as unavailable
    async fn mark_unavailable(&self, space_id: &str, server_id: &str) -> RepoResult<()>;

    /// Delete all features for a server
    async fn delete_for_server(&self, space_id: &str, server_id: &str) -> RepoResult<()>;
}

/// FeatureSet repository trait
#[async_trait]
pub trait FeatureSetRepository: Send + Sync {
    /// Get all feature sets (across all spaces)
    async fn list(&self) -> RepoResult<Vec<FeatureSet>>;

    /// Get feature sets for a specific space
    async fn list_by_space(&self, space_id: &str) -> RepoResult<Vec<FeatureSet>>;

    /// Get a feature set by ID
    async fn get(&self, id: &str) -> RepoResult<Option<FeatureSet>>;

    /// Get a feature set by ID with its members loaded
    async fn get_with_members(&self, id: &str) -> RepoResult<Option<FeatureSet>>;

    /// Create a new feature set
    async fn create(&self, feature_set: &FeatureSet) -> RepoResult<()>;

    /// Update a feature set
    async fn update(&self, feature_set: &FeatureSet) -> RepoResult<()>;

    /// Delete a feature set (soft delete)
    async fn delete(&self, id: &str) -> RepoResult<()>;

    /// Get builtin feature sets for a space
    async fn list_builtin(&self, space_id: &str) -> RepoResult<Vec<FeatureSet>>;

    /// Get server-all featureset for a server in a space
    async fn get_server_all(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> RepoResult<Option<FeatureSet>>;

    /// Create server-all featureset if it doesn't exist
    async fn ensure_server_all(
        &self,
        space_id: &str,
        server_id: &str,
        server_name: &str,
    ) -> RepoResult<FeatureSet>;

    /// Get the "Default" featureset for a space
    async fn get_default_for_space(&self, space_id: &str) -> RepoResult<Option<FeatureSet>>;

    /// Get the "All" featureset for a space
    async fn get_all_for_space(&self, space_id: &str) -> RepoResult<Option<FeatureSet>>;

    /// Ensure builtin feature sets exist for a space (All + Default)
    async fn ensure_builtin_for_space(&self, space_id: &str) -> RepoResult<()>;

    /// Delete server-all feature set for a server (used when uninstalling)
    async fn delete_server_all(&self, space_id: &str, server_id: &str) -> RepoResult<()>;

    /// Add an individual feature as a member of a feature set
    async fn add_feature_member(
        &self,
        feature_set_id: &str,
        feature_id: &str,
        mode: MemberMode,
    ) -> RepoResult<()>;

    /// Remove an individual feature from a feature set
    async fn remove_feature_member(&self, feature_set_id: &str, feature_id: &str)
        -> RepoResult<()>;

    /// Get all individual feature members of a feature set
    async fn get_feature_members(&self, feature_set_id: &str) -> RepoResult<Vec<FeatureSetMember>>;
}

/// Inbound MCP Client repository trait
///
/// Manages MCP client entities (apps connecting TO McpMux).
/// Works with the unified `inbound_clients` table.
#[async_trait]
pub trait InboundMcpClientRepository: Send + Sync {
    /// Get all clients
    async fn list(&self) -> RepoResult<Vec<Client>>;

    /// Get a client by ID
    async fn get(&self, id: &Uuid) -> RepoResult<Option<Client>>;

    /// Get a client by access key
    async fn get_by_access_key(&self, key: &str) -> RepoResult<Option<Client>>;

    /// Create a new client
    async fn create(&self, client: &Client) -> RepoResult<()>;

    /// Update a client
    async fn update(&self, client: &Client) -> RepoResult<()>;

    /// Delete a client
    async fn delete(&self, id: &Uuid) -> RepoResult<()>;

    /// Grant a feature set to a client for a specific space
    async fn grant_feature_set(
        &self,
        client_id: &Uuid,
        space_id: &str,
        feature_set_id: &str,
    ) -> RepoResult<()>;

    /// Revoke a feature set from a client for a specific space
    async fn revoke_feature_set(
        &self,
        client_id: &Uuid,
        space_id: &str,
        feature_set_id: &str,
    ) -> RepoResult<()>;

    /// Get all feature set IDs granted to a client for a specific space
    async fn get_grants_for_space(
        &self,
        client_id: &Uuid,
        space_id: &str,
    ) -> RepoResult<Vec<String>>;

    /// Get all grants for a client (all spaces)
    async fn get_all_grants(
        &self,
        client_id: &Uuid,
    ) -> RepoResult<std::collections::HashMap<String, Vec<String>>>;

    /// Set all grants for a client in a space (replaces existing)
    async fn set_grants_for_space(
        &self,
        client_id: &Uuid,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> RepoResult<()>;

    /// Check if client has any grants for a space
    async fn has_grants_for_space(&self, client_id: &Uuid, space_id: &str) -> RepoResult<bool>;
}

/// Credential repository trait (local-only, never synced)
///
/// Each credential is a separate row per (space, server, type).
/// This allows independent lifecycle management for access tokens vs refresh tokens.
#[async_trait]
pub trait CredentialRepository: Send + Sync {
    /// Get a specific credential by (space, server, type)
    async fn get(
        &self,
        space_id: &Uuid,
        server_id: &str,
        credential_type: &CredentialType,
    ) -> RepoResult<Option<Credential>>;

    /// Get all credentials for a (space, server) combination
    async fn get_all(&self, space_id: &Uuid, server_id: &str) -> RepoResult<Vec<Credential>>;

    /// Save a credential (upsert by space_id + server_id + credential_type)
    async fn save(&self, credential: &Credential) -> RepoResult<()>;

    /// Delete a specific credential by type
    async fn delete(
        &self,
        space_id: &Uuid,
        server_id: &str,
        credential_type: &CredentialType,
    ) -> RepoResult<()>;

    /// Delete all credentials for a (space, server) combination
    async fn delete_all(&self, space_id: &Uuid, server_id: &str) -> RepoResult<()>;

    /// Clear OAuth tokens (access + refresh) but preserve client registration (for logout)
    /// Returns true if tokens were cleared
    async fn clear_tokens(&self, space_id: &Uuid, server_id: &str) -> RepoResult<bool>;

    /// List all credentials for a space
    async fn list_for_space(&self, space_id: &Uuid) -> RepoResult<Vec<Credential>>;
}

/// Outbound OAuth Client repository (OUTBOUND)
/// Stores McpMux's OAuth client registrations WITH backend MCP servers
/// (McpMux acting as OAuth client connecting TO backends)
#[async_trait]
pub trait OutboundOAuthRepository: Send + Sync {
    /// Get registration for a (space, server) combination
    async fn get(
        &self,
        space_id: &Uuid,
        server_id: &str,
    ) -> RepoResult<Option<OutboundOAuthRegistration>>;

    /// Save or update registration
    async fn save(&self, registration: &OutboundOAuthRegistration) -> RepoResult<()>;

    /// Delete registration
    async fn delete(&self, space_id: &Uuid, server_id: &str) -> RepoResult<()>;

    /// List all registrations for a space
    async fn list_for_space(&self, space_id: &Uuid) -> RepoResult<Vec<OutboundOAuthRegistration>>;
}

/// App Settings repository trait
///
/// Key-value store for application-wide settings.
/// Replaces scattered config files with a unified SQLite-backed store.
///
/// # Key Naming Convention
/// Use dot-notation for namespacing:
/// - `gateway.port` - Gateway server port
/// - `gateway.auto_start` - Auto-start gateway on app launch
/// - `ui.theme` - UI theme preference
/// - `ui.window_state` - Window position/size (JSON)
#[async_trait]
pub trait AppSettingsRepository: Send + Sync {
    /// Get a setting value by key
    async fn get(&self, key: &str) -> RepoResult<Option<String>>;

    /// Set a setting value (insert or update)
    async fn set(&self, key: &str, value: &str) -> RepoResult<()>;

    /// Delete a setting by key
    async fn delete(&self, key: &str) -> RepoResult<()>;

    /// Get all settings (for export/debug)
    async fn list(&self) -> RepoResult<Vec<(String, String)>>;

    /// Get all settings with a given prefix (e.g., "gateway." returns all gateway settings)
    async fn list_by_prefix(&self, prefix: &str) -> RepoResult<Vec<(String, String)>>;
}
