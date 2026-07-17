//! Repository traits for data access
//!
//! These traits define the interface for data storage without specifying
//! the implementation (SQLite, in-memory, etc.)

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::{
    path_is_within, Client, Credential, CredentialType, FeatureSet, FeatureSetMember,
    InstalledServer, Machine, MemberMode, OutboundOAuthRegistration, ServerFeature, Space,
    SpaceBaseDir, WorkspaceAppearance, WorkspaceBinding,
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

/// Per-Space base-directory repository.
///
/// Manages the folders a Space claims. A reported workspace root at or under a
/// base dir is scoped to that Space (longest-prefix wins when base dirs nest).
#[async_trait]
pub trait SpaceBaseDirRepository: Send + Sync {
    /// Every base dir across all Spaces.
    async fn list_all(&self) -> RepoResult<Vec<SpaceBaseDir>>;

    /// Base dirs for one Space.
    async fn list_by_space(&self, space_id: &Uuid) -> RepoResult<Vec<SpaceBaseDir>>;

    /// Add a base dir to a Space. `path` MUST already be normalized via
    /// [`crate::domain::normalize_workspace_root`]. Returns an error if the
    /// path is already claimed by any Space (one owner per path).
    async fn add(&self, space_id: &Uuid, path: &str) -> RepoResult<SpaceBaseDir>;

    /// Remove a base dir by its row id.
    async fn remove(&self, id: &str) -> RepoResult<()>;

    /// The Space whose base dir is the longest prefix of `root` (already
    /// normalized), or `None` when no base dir contains it. Most-specific
    /// (longest) base dir wins when several nest.
    async fn find_space_for_root(&self, root: &str) -> RepoResult<Option<Uuid>>;
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

    /// Pass `None` to clear the override (UI falls back to `server_name` /
    /// `cached_definition.name` / `server_id` tail).
    async fn set_display_name_override(&self, id: &Uuid, value: Option<String>) -> RepoResult<()>;

    /// Persist notify-mode version probe results for one installation.
    async fn update_version_cache(
        &self,
        id: &Uuid,
        latest_available_version: Option<String>,
        current_version: Option<String>,
        version_checked_at: DateTime<Utc>,
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

    /// Get the auto-seeded "Starter" FeatureSet for a Space, if it
    /// exists. Routing-irrelevant under resolver v3 — UI helpers use it
    /// to suggest a default selection in the binding/grant pickers.
    async fn get_starter_for_space(&self, space_id: &str) -> RepoResult<Option<FeatureSet>>;

    /// Ensure the auto-seeded Starter FeatureSet exists for a Space.
    ///
    /// Called during Space creation and any time a defensive re-seed is
    /// needed (Workspace inspector references the Starter as a "preview"
    /// for unbound roots and would crash with `None`).
    async fn ensure_builtin_for_space(&self, space_id: &str) -> RepoResult<()>;

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
///
/// Only identity is persisted here — routing is resolved per-session
/// via WorkspaceBinding and each Space's Default feature set.
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
}

/// Machine repository trait — catalog of hosts that report workspace roots.
#[async_trait]
pub trait MachineRepository: Send + Sync {
    /// List all registered machines.
    async fn list(&self) -> RepoResult<Vec<Machine>>;

    /// Get a machine by id.
    async fn get(&self, id: &Uuid) -> RepoResult<Option<Machine>>;

    /// Insert a new machine.
    async fn create(&self, machine: &Machine) -> RepoResult<()>;

    /// Update an existing machine.
    async fn update(&self, machine: &Machine) -> RepoResult<()>;

    /// Delete a machine by id.
    async fn delete(&self, id: &Uuid) -> RepoResult<()>;
}

/// Workspace binding repository trait
///
/// Bindings map normalized filesystem paths to FeatureSets on a per-Space basis.
/// Matching is EXACT (no ancestor/prefix inheritance — see `find_exact_for_roots`);
/// callers are expected to pass already-normalized paths (see
/// [`crate::domain::normalize_workspace_root`]).
#[async_trait]
pub trait WorkspaceBindingRepository: Send + Sync {
    /// List every binding across all Spaces.
    async fn list(&self) -> RepoResult<Vec<WorkspaceBinding>>;

    /// List bindings for a specific Space.
    async fn list_for_space(&self, space_id: &Uuid) -> RepoResult<Vec<WorkspaceBinding>>;

    /// Fetch a binding by id.
    async fn get(&self, id: &Uuid) -> RepoResult<Option<WorkspaceBinding>>;

    /// Insert a new binding. Fails on `(space_id, workspace_root)` conflict.
    async fn create(&self, binding: &WorkspaceBinding) -> RepoResult<()>;

    /// Update an existing binding (e.g., point to a different FS).
    async fn update(&self, binding: &WorkspaceBinding) -> RepoResult<()>;

    /// Delete a binding by id.
    async fn delete(&self, id: &Uuid) -> RepoResult<()>;

    /// Resolve which binding applies for a set of candidate workspace roots.
    ///
    /// EXACT match only — a folder resolves to a binding whose `workspace_root`
    /// equals one of the candidates. There is NO ancestor/prefix inheritance:
    /// `d:\a\b` does not pick up a binding on `d:\a`. Every candidate MUST
    /// already be normalized. Returns the first matching binding (each binding
    /// carries its own target Space).
    async fn find_exact_for_roots(
        &self,
        candidate_roots: &[String],
    ) -> RepoResult<Option<WorkspaceBinding>>;

    /// Exact match for a machine-scoped binding on `workspace_root`.
    ///
    /// Matches bindings where `machine_id` equals the given value AND either:
    /// - `client_id` is `None` (canonical machine binding), or
    /// - `client_id` equals the given `client_id` (client+machine scoped binding).
    ///
    /// A client+machine scoped binding wins over a machine-only canonical binding
    /// because it is more specific. Pass `client_id: None` to match canonical
    /// machine bindings only.
    async fn find_exact_for_machine(
        &self,
        machine_id: &Uuid,
        workspace_root: &str,
        client_id: Option<&str>,
    ) -> RepoResult<Option<WorkspaceBinding>>;

    /// Exact match for a global canonical binding (`machine_id` and `client_id`
    /// both unset) on `workspace_root`.
    async fn find_exact_global(&self, workspace_root: &str)
        -> RepoResult<Option<WorkspaceBinding>>;

    /// Lookup an id-type binding keyed by OAuth/API `client_id`.
    ///
    /// When `machine_id` is `Some`, matches a machine-scoped id binding on that
    /// host. When `None`, matches a global id binding (`machine_id IS NULL`).
    /// Path-type rows are never returned.
    async fn find_by_id_key(
        &self,
        client_id: &str,
        machine_id: Option<&Uuid>,
    ) -> RepoResult<Option<WorkspaceBinding>>;

    /// Resolve which binding applies for a set of candidate workspace roots by
    /// longest-prefix containment.
    ///
    /// Returns the binding whose `workspace_root` is the longest prefix of (or
    /// equals) any candidate. Every candidate MUST already be normalized. When
    /// `client_id` is `Some`, the client's own scoped bindings are considered
    /// alongside global (`client_id` unset) bindings — a scoped binding wins a
    /// same-path tie so a client override shadows the global default. When
    /// `client_id` is `None`, only global bindings are considered.
    ///
    /// Default impl scans `list_for_space` and matches in Rust on path-segment
    /// boundaries — used by `mcpmux_bind_current_workspace` dedup.
    async fn find_longest_prefix_match(
        &self,
        space_id: &Uuid,
        client_id: Option<&str>,
        candidate_roots: &[String],
    ) -> RepoResult<Option<WorkspaceBinding>> {
        let bindings = self.list_for_space(space_id).await?;
        let mut best: Option<WorkspaceBinding> = None;
        for binding in bindings {
            let applies = match client_id {
                Some(cid) => {
                    binding.client_id.is_none() || binding.client_id.as_deref() == Some(cid)
                }
                None => binding.client_id.is_none(),
            };
            if !applies {
                continue;
            }
            let contains_candidate = candidate_roots
                .iter()
                .any(|root| path_is_within(root, &binding.workspace_root));
            if !contains_candidate {
                continue;
            }
            let better = match &best {
                Some(current) if binding.workspace_root.len() == current.workspace_root.len() => {
                    binding.client_id.is_some() && current.client_id.is_none()
                }
                Some(current) => binding.workspace_root.len() > current.workspace_root.len(),
                None => true,
            };
            if better {
                best = Some(binding);
            }
        }
        Ok(best)
    }
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

/// Per-Space configuration for built-in MCP servers.
///
/// Built-in servers (e.g. "Tool Optimization", the `mcpmux_*` tools) and their
/// individual tools are enabled/disabled **per Space**. Absence of a stored
/// row means "default": the server uses its descriptor's `default_enabled`, and
/// every tool is on. Only deviations from the default are persisted.
#[async_trait]
pub trait SpaceBuiltinConfigRepository: Send + Sync {
    /// The stored enable override for a built-in server in a Space, or `None`
    /// when the Space has never overridden it (callers fall back to the
    /// descriptor's `default_enabled`).
    async fn server_enabled_override(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> RepoResult<Option<bool>>;

    /// Tool names explicitly disabled for `(space, server)`. Any tool not in
    /// this list is enabled.
    async fn disabled_tools(&self, space_id: &str, server_id: &str) -> RepoResult<Vec<String>>;

    /// Enable/disable a built-in server for a Space.
    async fn set_server_enabled(
        &self,
        space_id: &str,
        server_id: &str,
        enabled: bool,
    ) -> RepoResult<()>;

    /// Enable/disable an individual tool of a built-in server for a Space.
    async fn set_tool_enabled(
        &self,
        space_id: &str,
        server_id: &str,
        tool_name: &str,
        enabled: bool,
    ) -> RepoResult<()>;
}

/// A persisted embedding keyed by content hash + model version.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingRecord {
    pub content_hash: String,
    pub model_version: String,
    pub vector: Vec<f32>,
}

/// Embedding repository trait — caches tool embedding vectors for semantic search.
#[async_trait]
pub trait EmbeddingRepository: Send + Sync {
    /// Load vectors for a set of content hashes and a model version.
    async fn get_many(
        &self,
        content_hashes: &[String],
        model_version: &str,
    ) -> RepoResult<Vec<EmbeddingRecord>>;

    /// Insert or replace vectors by `(content_hash, model_version)`.
    async fn upsert_many(&self, records: &[EmbeddingRecord]) -> RepoResult<()>;
}

/// Workspace appearance repository trait — icon overrides keyed by normalized
/// workspace root (covers unmapped roots that have no binding).
#[async_trait]
pub trait WorkspaceAppearanceRepository: Send + Sync {
    /// List all stored workspace appearance overrides.
    async fn list(&self) -> RepoResult<Vec<WorkspaceAppearance>>;

    /// Get a stored appearance by normalized workspace root.
    async fn get(&self, workspace_root: &str) -> RepoResult<Option<WorkspaceAppearance>>;

    /// Insert or update an appearance for a normalized workspace root.
    async fn upsert(&self, appearance: &WorkspaceAppearance) -> RepoResult<()>;

    /// Delete a stored appearance by normalized workspace root.
    async fn delete(&self, workspace_root: &str) -> RepoResult<()>;
}
