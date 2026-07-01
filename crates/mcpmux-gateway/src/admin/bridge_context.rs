//! Shared dependency context for admin command bridges.

use std::path::PathBuf;
use std::sync::Arc;

use mcpmux_core::{
    AppSettingsRepository, ApplicationServices, FeatureSetRepository, GatewayPortService,
    MachineRepository, ServerDiscoveryService, ServerFeatureRepository, ServerLogManager,
    SpaceBaseDirRepository, SpaceBuiltinConfigRepository, SpaceService,
    WorkspaceAppearanceRepository, WorkspaceBindingRepository,
};
use mcpmux_storage::InboundClientRepository;

use super::runtime::GatewayRuntime;
use super::write_runtime::GatewayWriteRuntime;

/// Git/build metadata compiled into the desktop binary at build time.
#[derive(Clone, Debug, Default)]
pub struct BackendBuildStamp {
    pub git_sha: String,
    pub git_branch: String,
    pub commit_time: String,
    pub build_time: String,
}

/// Shared dependency graph used by admin bridge functions.
///
/// This mirrors the desktop `AppState` dependency surface so handlers can stay
/// thin and bridge modules can be reused across transports.
#[derive(Clone)]
pub struct AdminBridgeCtx {
    pub services: Arc<ApplicationServices>,
    pub spaces_dir: PathBuf,
    pub data_dir: PathBuf,
    pub gateway_port_service: Arc<GatewayPortService>,
    pub server_discovery: Arc<ServerDiscoveryService>,
    pub settings_repository: Arc<dyn AppSettingsRepository>,
    pub workspace_binding_repository: Arc<dyn WorkspaceBindingRepository>,
    pub machine_repository: Arc<dyn MachineRepository>,
    pub inbound_client_repository: Arc<InboundClientRepository>,
    pub workspace_appearance_repository: Arc<dyn WorkspaceAppearanceRepository>,
    pub server_feature_repository: Arc<dyn ServerFeatureRepository>,
    pub server_log_manager: Arc<ServerLogManager>,
    pub space_service: Arc<SpaceService>,
    pub gateway_runtime: Arc<dyn GatewayRuntime>,
    /// Gateway-dependent write operations (start/stop, server connections, OAuth grants).
    pub gateway_writes: Arc<dyn GatewayWriteRuntime>,
    pub feature_set_repository: Arc<dyn FeatureSetRepository>,
    pub space_base_dir_repository: Arc<dyn SpaceBaseDirRepository>,
    pub space_builtin_config_repository: Arc<dyn SpaceBuiltinConfigRepository>,
    /// Optional OS auto-launch value injected by desktop runtime.
    pub auto_launch_enabled: Option<bool>,
    /// Desktop app version (`CARGO_PKG_VERSION` from the app crate).
    pub app_version: String,
    /// Desktop bundle version when available (macOS app bundle).
    pub bundle_version: Option<String>,
    /// Git/build metadata compiled into the desktop binary (`MCPMUX_BUILD_*`).
    pub backend_build: BackendBuildStamp,
}
