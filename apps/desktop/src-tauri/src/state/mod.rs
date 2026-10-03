//! Application state shared between Tauri commands.
//!
//! Thin wrapper around [`mcpmux_runtime::Runtime`]. The runtime owns the
//! data directory lock, SQLite database, encryption, repositories,
//! discovery, log manager, event bus, and gateway bootstrap; this struct
//! re-exposes those fields under the names the existing commands expect
//! and holds the Tauri-only state (file-watcher handles, server-app
//! service, etc.) that does not belong in the runtime.
//!
//! [`AppState`] derefs to [`Runtime`] so every existing
//! `app_state.<field>` access — `data_dir`, `encryptor`,
//! `gateway_port_service`, `space_service`, `server_discovery`,
//! `server_log_manager` — keeps working. Fields whose names on the
//! runtime (`repositories.app_settings`, `repositories.installed_server`,
//! …) differ from the legacy desktop names
//! (`settings_repository`, `installed_server_repository`, …) are
//! aliased below as Arc clones; nothing in the command layer needs to
//! change in this PR.

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mcpmux_core::{
    AppSettingsRepository, CredentialRepository, FeatureSetRepository, InboundMcpClientRepository,
    InstalledServerRepository, ServerFeatureRepository, SpaceBaseDirRepository,
    SpaceBuiltinConfigRepository, WorkspaceBindingRepository,
};
use mcpmux_runtime::Runtime;
use mcpmux_storage::{Database, SqliteServerFeatureRepository};

/// Global application state accessible from commands.
pub struct AppState {
    /// The shared runtime. Held by `Arc` so the lock + database + repos
    /// outlive the bootstrap function and any future CLI can read them.
    runtime: Arc<Runtime>,

    /// Aliases for fields renamed by the runtime extraction. See the
    /// module-level docs for the full mapping.
    pub settings_repository: Arc<dyn AppSettingsRepository>,
    pub installed_server_repository: Arc<dyn InstalledServerRepository>,
    pub credential_repository: Arc<dyn CredentialRepository>,
    pub feature_set_repository: Arc<dyn FeatureSetRepository>,
    pub client_repository: Arc<dyn InboundMcpClientRepository>,
    pub workspace_binding_repository: Arc<dyn WorkspaceBindingRepository>,
    pub space_base_dir_repository: Arc<dyn SpaceBaseDirRepository>,
    pub space_builtin_config_repository: Arc<dyn SpaceBuiltinConfigRepository>,
    pub server_feature_repository: Arc<SqliteServerFeatureRepository>,
    pub server_feature_repository_core: Arc<dyn ServerFeatureRepository>,
}

impl AppState {
    /// Build the application state for the given data directory.
    ///
    /// Delegates to [`mcpmux_runtime::RuntimeBuilder`] for everything
    /// environment-neutral: lock acquisition, key provider, SQLite +
    /// migrations, repository graph, JWT secret, event bus.
    ///
    /// The setup closure runs synchronously; Tauri's `async_runtime`
    /// provides the Tokio context so the runtime's `build()` can use
    /// `block_in_place` internally.
    pub fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        let runtime = tauri::async_runtime::block_on(async {
            mcpmux_runtime::RuntimeBuilder::new()
                .with_data_dir(data_dir)
                // After an in-place update the OS relaunches the new build
                // before the old process has exited and released the lock;
                // wait it out the same way gateway auto-start waits for the
                // port instead of failing setup.
                .with_lock_wait(mcpmux_core::service::AUTOSTART_PORT_WAIT)
                .build()
                .await
        })?;

        Ok(Self {
            settings_repository: runtime.repositories.app_settings.clone(),
            installed_server_repository: runtime.repositories.installed_server.clone(),
            credential_repository: runtime.repositories.credential.clone(),
            feature_set_repository: runtime.repositories.feature_set.clone(),
            client_repository: runtime.repositories.client.clone(),
            workspace_binding_repository: runtime.repositories.workspace_binding.clone(),
            space_base_dir_repository: runtime.repositories.space_base_dir.clone(),
            space_builtin_config_repository: runtime.repositories.space_builtin_config.clone(),
            server_feature_repository: runtime.repositories.server_feature.clone(),
            server_feature_repository_core: runtime.repositories.server_feature_core.clone(),
            runtime,
        })
    }

    /// Shared database handle. Used by the gateway bootstrap and the
    /// desktop's OAuth flow; aliases the runtime's `database` field for
    /// callers that read it as a method.
    pub fn database(&self) -> Arc<tokio::sync::Mutex<Database>> {
        self.runtime.database.clone()
    }

    /// Data directory root. Kept as a method (returning `&Path`) so
    /// callers that did `app_state.data_dir().to_path_buf()` keep working.
    pub fn data_dir(&self) -> &Path {
        &self.runtime.data_dir
    }

    /// Spaces configuration directory. Same rationale as [`Self::data_dir`].
    #[allow(dead_code)]
    pub fn spaces_dir(&self) -> &Path {
        &self.runtime.spaces_dir
    }

    /// Get the path to a specific space's config file.
    ///
    /// Fails when `space_id` is not a valid UUID — the id arrives over
    /// IPC, so this is the path-traversal guard for every space-config
    /// command.
    pub fn space_config_path(&self, space_id: &str) -> Result<PathBuf, String> {
        mcpmux_core::get_space_config_path(&self.runtime.spaces_dir, space_id)
            .map_err(|e| format!("Invalid space id '{space_id}': {e}"))
    }

    /// Access the underlying runtime for the few callers that need it
    /// (gateway construction in `commands/gateway.rs`, event-bridge
    /// subscribers in Phase 2).
    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }
}

impl Deref for AppState {
    type Target = Runtime;

    fn deref(&self) -> &Runtime {
        &self.runtime
    }
}
