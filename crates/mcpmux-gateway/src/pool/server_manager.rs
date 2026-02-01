//! ServerManager - Central orchestrator for server connection state
//!
//! Event-driven architecture:
//! - UI sends commands: enable_server, disable_server, start_auth, cancel_auth
//! - Backend emits events: server:status, server:auth_progress, server:features_updated
//! - No UI polling: all updates via events
//!
//! State machine with race condition prevention:
//! - flow_id: Monotonic counter, incremented on state-changing operations
//! - connect_lock, auth_lock, refresh_lock: Prevent concurrent operations
//! - Stale callbacks/timeouts validated against flow_id

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use mcpmux_core::{DiscoveredCapabilities, DomainEvent};
use tokio::sync::{broadcast, Mutex, OwnedMutexGuard, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

use super::{CachedFeatures, ConnectionService, FeatureService};
use crate::services::PrefixCacheService;

/// Open a URL without flashing a terminal window (Windows-specific)
#[cfg(target_os = "windows")]
fn open_url_no_flash(url: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_cmd: i32,
        ) -> isize;
    }

    let url_wide: Vec<u16> = OsStr::new(url).encode_wide().chain(Some(0)).collect();
    let open_wide: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();

    // SW_SHOWNORMAL = 1
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            open_wide.as_ptr(),
            url_wide.as_ptr(),
            ptr::null(),
            ptr::null(),
            1,
        )
    };

    // ShellExecuteW returns > 32 on success
    if result > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW failed with code: {}", result))
    }
}

/// Open a URL using the default handler (non-Windows)
#[cfg(not(target_os = "windows"))]
fn open_url_no_flash(url: &str) -> Result<(), String> {
    open::that(url).map_err(|e| format!("Failed to open URL: {}", e))
}

/// Browser debounce duration (prevent multiple browser opens on quick clicks)
const BROWSER_DEBOUNCE: Duration = Duration::from_secs(2);

/// OAuth timeout duration
/// Future feature - not yet used
#[allow(dead_code)]
const AUTH_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// Refresh interval for connected servers
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// Connection status - runtime state, never persisted to DB
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ConnectionStatus {
    /// Server is disabled or not started
    #[default]
    Disconnected,
    /// Attempting to connect (connect_lock held)
    Connecting,
    /// Server is running and ready
    Connected,
    /// Token refresh in progress (refresh_lock held)
    Refreshing,
    /// OAuth needed - waiting for user to click Connect/Reconnect
    AuthRequired,
    /// OAuth flow in progress (auth_lock held)
    Authenticating,
    /// Connection failed
    Error,
}

/// OAuth flow state during Authenticating status
pub struct AuthFlowState {
    /// Authorization URL for browser
    pub auth_url: String,
    /// When the flow started
    pub started_at: Instant,
    /// When browser was last opened (for debounce)
    pub browser_opened_at: Instant,
    /// Timeout task handle
    pub timeout_handle: Option<JoinHandle<()>>,
    /// Listener task handle (legacy - no longer used with deep links)
    pub listener_handle: Option<JoinHandle<()>>,
}

/// Per-server runtime state (in-memory only)
pub struct ServerState {
    /// Current connection status
    pub status: ConnectionStatus,
    /// Monotonic counter for race condition prevention
    pub flow_id: u64,
    /// Whether user has successfully connected before (for Connect vs Reconnect button)
    pub has_connected_before: bool,
    /// Cached features (tools, prompts, resources)
    pub features: Option<CachedFeatures>,
    /// Error message if status is Error
    pub error: Option<String>,
    /// OAuth flow state if Authenticating
    pub auth: Option<AuthFlowState>,

    // Operation locks (prevent concurrent operations)
    /// Lock for connect operations
    connect_lock: Option<OwnedMutexGuard<()>>,
    /// Lock for auth operations  
    auth_lock: Option<OwnedMutexGuard<()>>,
    /// Lock for refresh operations
    refresh_lock: Option<OwnedMutexGuard<()>>,

    // Handles for cancellation
    /// Connect task handle
    connect_handle: Option<JoinHandle<()>>,

    // Shared mutexes (for try_lock)
    connect_mutex: Arc<Mutex<()>>,
    auth_mutex: Arc<Mutex<()>>,
    refresh_mutex: Arc<Mutex<()>>,
}

impl Default for ServerState {
    fn default() -> Self {
        Self {
            status: ConnectionStatus::Disconnected,
            flow_id: 0,
            has_connected_before: false,
            features: None,
            error: None,
            auth: None,
            connect_lock: None,
            auth_lock: None,
            refresh_lock: None,
            connect_handle: None,
            connect_mutex: Arc::new(Mutex::new(())),
            auth_mutex: Arc::new(Mutex::new(())),
            refresh_mutex: Arc::new(Mutex::new(())),
        }
    }
}

// All events now use unified GatewayEvent system

/// Composite key for server state: space_id + server_id
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ServerKey {
    pub space_id: Uuid,
    pub server_id: String,
}

impl ServerKey {
    pub fn new(space_id: Uuid, server_id: impl Into<String>) -> Self {
        Self {
            space_id,
            server_id: server_id.into(),
        }
    }
}

/// Central orchestrator for server connection state
pub struct ServerManager {
    /// Per-server state with fine-grained locking
    states: DashMap<ServerKey, RwLock<ServerState>>,
    /// Event sender for unified domain event emission (non-blocking)
    event_tx: tokio::sync::broadcast::Sender<DomainEvent>,
    /// Feature service for feature discovery
    feature_service: Arc<FeatureService>,
    /// Connection service for connect/disconnect
    connection_service: Arc<ConnectionService>,
    /// Prefix cache service for runtime prefix assignment
    prefix_cache: Arc<PrefixCacheService>,
}

impl ServerManager {
    /// Create a new ServerManager
    pub fn new(
        event_tx: tokio::sync::broadcast::Sender<DomainEvent>,
        feature_service: Arc<FeatureService>,
        connection_service: Arc<ConnectionService>,
        prefix_cache: Arc<PrefixCacheService>,
    ) -> Self {
        Self {
            states: DashMap::new(),
            event_tx,
            feature_service,
            connection_service,
            prefix_cache,
        }
    }

    /// Subscribe to gateway events (unified event system)
    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        // Non-blocking: just subscribe to cloned sender
        self.event_tx.subscribe()
    }

    /// Get current status for a server (for initial UI load)
    pub async fn get_status(
        &self,
        key: &ServerKey,
    ) -> Option<(ConnectionStatus, u64, bool, Option<String>)> {
        if let Some(entry) = self.states.get(key) {
            let state = entry.read().await;
            Some((
                state.status,
                state.flow_id,
                state.has_connected_before,
                state.error.clone(),
            ))
        } else {
            None
        }
    }

    /// Get all server statuses for a space
    pub async fn get_all_statuses(
        &self,
        space_id: Uuid,
    ) -> HashMap<String, (ConnectionStatus, u64, bool, Option<String>)> {
        let mut result = HashMap::new();
        for entry in self.states.iter() {
            if entry.key().space_id == space_id {
                let state = entry.value().read().await;
                result.insert(
                    entry.key().server_id.clone(),
                    (
                        state.status,
                        state.flow_id,
                        state.has_connected_before,
                        state.error.clone(),
                    ),
                );
            }
        }
        result
    }

    /// Count currently connected servers across all spaces
    pub async fn connected_count(&self) -> usize {
        let mut count = 0;
        for entry in self.states.iter() {
            let state = entry.value().read().await;
            if state.status == ConnectionStatus::Connected {
                count += 1;
            }
        }
        count
    }

    /// Emit a domain event (unified event system)
    fn emit(&self, event: DomainEvent) {
        // Trace Refreshing events to find the source
        if let DomainEvent::ServerStatusChanged {
            ref server_id,
            ref status,
            ..
        } = event
        {
            if matches!(status, mcpmux_core::ConnectionStatus::Refreshing) {
                warn!(
                    server_id = %server_id,
                    "[ServerManager] ⚠️ TRACE: Emitting Refreshing status (call stack: {:?})",
                    std::backtrace::Backtrace::capture()
                );
            }
        }

        // Non-blocking: send directly via cloned sender
        if let Err(e) = self.event_tx.send(event) {
            debug!("No domain event subscribers: {}", e);
        }
    }

    /// Convert local ConnectionStatus to core ConnectionStatus for events
    fn to_core_status(&self, status: ConnectionStatus) -> mcpmux_core::ConnectionStatus {
        match status {
            ConnectionStatus::Disconnected => mcpmux_core::ConnectionStatus::Disconnected,
            ConnectionStatus::Connecting => mcpmux_core::ConnectionStatus::Connecting,
            ConnectionStatus::Connected => mcpmux_core::ConnectionStatus::Connected,
            ConnectionStatus::Refreshing => mcpmux_core::ConnectionStatus::Refreshing,
            ConnectionStatus::AuthRequired => mcpmux_core::ConnectionStatus::OAuthRequired,
            ConnectionStatus::Authenticating => mcpmux_core::ConnectionStatus::Authenticating,
            ConnectionStatus::Error => mcpmux_core::ConnectionStatus::Error,
        }
    }

    /// Convert CachedFeatures to DiscoveredCapabilities for events
    fn to_discovered_capabilities(&self, features: &CachedFeatures) -> DiscoveredCapabilities {
        DiscoveredCapabilities {
            tools: features.tools.clone(),
            prompts: features.prompts.clone(),
            resources: features.resources.clone(),
        }
    }

    /// Get or create server state
    fn get_or_create_state(
        &self,
        key: ServerKey,
    ) -> dashmap::mapref::one::Ref<'_, ServerKey, RwLock<ServerState>> {
        let key_clone = key.clone();
        self.states
            .entry(key)
            .or_insert_with(|| RwLock::new(ServerState::default()));
        self.states.get(&key_clone).unwrap()
    }

    // =========================================================================
    // Atomic Operations
    // =========================================================================

    /// Enable server and attempt connection
    ///
    /// Flow:
    /// 1. Try to acquire connect_lock
    /// 2. Increment flow_id
    /// 3. Set status = Connecting
    /// 4. Emit event
    /// 5. Spawn connect task
    pub async fn enable_server(&self, key: ServerKey) -> Result<(), String> {
        let entry = self.get_or_create_state(key.clone());
        let mut state = entry.write().await;

        // Already connecting or connected?
        if matches!(
            state.status,
            ConnectionStatus::Connecting | ConnectionStatus::Connected
        ) {
            return Ok(());
        }

        // Try to acquire connect lock (non-blocking)
        let connect_guard = match state.connect_mutex.clone().try_lock_owned() {
            Ok(guard) => guard,
            Err(_) => return Err("Already connecting".to_string()),
        };

        // Atomic state change
        state.flow_id += 1;
        state.status = ConnectionStatus::Connecting;
        state.connect_lock = Some(connect_guard);
        state.error = None;
        let flow_id = state.flow_id;

        info!(
            server_id = %key.server_id,
            space_id = %key.space_id,
            flow_id = flow_id,
            "Server enabling, status = Connecting"
        );

        // Emit event
        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::Connecting),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: None,
            features: None,
        });

        // Release state lock before async work
        drop(state);
        drop(entry);

        // Assign prefix for this server (fetches alias from registry internally)
        let space_id_str = key.space_id.to_string();
        let _ = self
            .prefix_cache
            .assign_prefix_for_server(&space_id_str, &key.server_id)
            .await;

        // Note: Actual connection is handled by PoolService/ConnectionService
        // This method just sets the connecting state
        Ok(())
    }

    /// Disable server (cancels any active operations)
    pub async fn disable_server(&self, key: &ServerKey) -> Result<(), String> {
        let entry = match self.states.get(key) {
            Some(e) => e,
            None => return Ok(()), // Not tracked
        };

        let mut state = entry.write().await;

        // Increment flow_id to invalidate ALL pending operations
        state.flow_id += 1;

        // Cancel any active operations
        if state.status == ConnectionStatus::Connecting {
            if let Some(handle) = state.connect_handle.take() {
                handle.abort();
            }
            state.connect_lock = None;
        }

        if state.status == ConnectionStatus::Authenticating {
            if let Some(auth) = state.auth.take() {
                if let Some(h) = auth.timeout_handle {
                    h.abort();
                }
                if let Some(h) = auth.listener_handle {
                    h.abort();
                }
            }
            state.auth_lock = None;
        }

        // Disconnect if connected (need to call connection_service)
        let was_connected = state.status == ConnectionStatus::Connected;

        // Check features BEFORE clearing
        let had_tools = state
            .features
            .as_ref()
            .map(|f| !f.tools.is_empty())
            .unwrap_or(false);
        let had_prompts = state
            .features
            .as_ref()
            .map(|f| !f.prompts.is_empty())
            .unwrap_or(false);
        let had_resources = state
            .features
            .as_ref()
            .map(|f| !f.resources.is_empty())
            .unwrap_or(false);
        let had_features = had_tools || had_prompts || had_resources;

        // Clear features
        state.features = None;

        state.status = ConnectionStatus::Disconnected;
        state.error = None;

        // Capture what we need for after dropping lock
        let flow_id = state.flow_id;
        let has_connected_before = state.has_connected_before;
        let server_id = key.server_id.clone();
        let space_id = key.space_id;

        info!(
            server_id = %server_id,
            space_id = %space_id,
            flow_id = flow_id,
            "Server disabled"
        );

        // Drop state lock before async operations
        drop(state);
        drop(entry);

        // Release prefix (no reassignment to other servers)
        let space_id_str = space_id.to_string();
        self.prefix_cache
            .release_prefix_runtime(&space_id_str, &server_id)
            .await;

        // Clean up connection resources if was connected
        if was_connected {
            // Clear tokens and mark features unavailable
            if let Err(e) = self
                .connection_service
                .disconnect(space_id, &server_id, &self.feature_service)
                .await
            {
                warn!(
                    server_id = %server_id,
                    space_id = %space_id,
                    "Failed to disconnect: {}",
                    e
                );
            }
        }

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: server_id.clone(),
            space_id,
            status: self.to_core_status(ConnectionStatus::Disconnected),
            flow_id,
            has_connected_before,
            message: None,
            features: None,
        });

        // Emit MCP list_changed notifications if server had features
        if had_features {
            if had_tools {
                self.emit(DomainEvent::ToolsChanged {
                    server_id: server_id.clone(),
                    space_id,
                });
            }
            if had_prompts {
                self.emit(DomainEvent::PromptsChanged {
                    server_id: server_id.clone(),
                    space_id,
                });
            }
            if had_resources {
                self.emit(DomainEvent::ResourcesChanged {
                    server_id,
                    space_id,
                });
            }
        }

        Ok(())
    }

    /// Start OAuth flow (from AuthRequired state)
    ///
    /// Handles debounce for double-clicks:
    /// - If already Authenticating and < 2s since browser open: ignore
    /// - If already Authenticating and >= 2s: reopen browser
    pub async fn start_auth(&self, key: &ServerKey) -> Result<(), String> {
        let entry = match self.states.get(key) {
            Some(e) => e,
            None => return Err("Server not found".to_string()),
        };

        let mut state = entry.write().await;

        // CASE 1: Already authenticating - check debounce
        if state.status == ConnectionStatus::Authenticating {
            if let Some(ref mut auth) = state.auth {
                let elapsed = auth.browser_opened_at.elapsed();

                if elapsed < BROWSER_DEBOUNCE {
                    // Quick double-click: ignore silently
                    debug!(
                        server_id = %key.server_id,
                        elapsed_ms = elapsed.as_millis(),
                        "Ignoring double-click (debounce)"
                    );
                    return Ok(());
                }

                // Cooldown expired: reopen browser
                auth.browser_opened_at = Instant::now();
                let auth_url = auth.auth_url.clone();
                drop(state);
                drop(entry);

                info!(
                    server_id = %key.server_id,
                    "Reopening browser (user may have closed it)"
                );

                // Open browser
                self.open_browser(&auth_url);
                return Ok(());
            }
        }

        // CASE 2: Start new auth flow (from AuthRequired state)
        if state.status != ConnectionStatus::AuthRequired {
            return Err(format!("Invalid state: {:?}", state.status));
        }

        // Try to acquire auth lock
        let auth_guard = match state.auth_mutex.clone().try_lock_owned() {
            Ok(guard) => guard,
            Err(_) => return Err("Already authenticating".to_string()),
        };

        // Atomic state change
        state.flow_id += 1;
        state.status = ConnectionStatus::Authenticating;
        state.auth_lock = Some(auth_guard);
        let flow_id = state.flow_id;

        info!(
            server_id = %key.server_id,
            space_id = %key.space_id,
            flow_id = flow_id,
            "Starting OAuth flow"
        );

        // Emit event
        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::Authenticating),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: Some("Opening browser...".to_string()),
            features: None,
        });

        // Release state lock before async work
        drop(state);
        drop(entry);

        // Note: Actual OAuth flow is handled by OutboundOAuthManager
        // This method just sets the authenticating state
        Ok(())
    }

    /// Cancel OAuth flow
    pub async fn cancel_auth(&self, key: &ServerKey) -> Result<(), String> {
        let entry = match self.states.get(key) {
            Some(e) => e,
            None => return Err("Server not found".to_string()),
        };

        let mut state = entry.write().await;

        if state.status != ConnectionStatus::Authenticating {
            return Err(format!("Not authenticating: {:?}", state.status));
        }

        // Increment flow_id FIRST (invalidates pending callbacks/timeouts)
        state.flow_id += 1;

        // Abort timeout and listener
        if let Some(auth) = state.auth.take() {
            if let Some(h) = auth.timeout_handle {
                h.abort();
            }
            if let Some(h) = auth.listener_handle {
                h.abort();
            }
        }

        // Release auth lock
        state.auth_lock = None;

        state.status = ConnectionStatus::AuthRequired;

        info!(
            server_id = %key.server_id,
            space_id = %key.space_id,
            flow_id = state.flow_id,
            "OAuth cancelled by user"
        );

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::AuthRequired),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: Some("Cancelled".to_string()),
            features: None,
        });

        Ok(())
    }

    // =========================================================================
    // Internal Handlers
    // =========================================================================

    /// Handle connect completion - Reserved for event-driven architecture
    #[allow(dead_code)]
    async fn on_connect_complete(
        &self,
        key: &ServerKey,
        flow_id: u64,
        result: Result<ConnectResult, String>,
    ) {
        let entry = match self.states.get(key) {
            Some(e) => e,
            None => return,
        };

        let mut state = entry.write().await;

        // Stale check - was this connect cancelled/superseded?
        if state.flow_id != flow_id {
            debug!(
                server_id = %key.server_id,
                expected_flow_id = state.flow_id,
                actual_flow_id = flow_id,
                "Dropping stale connect result"
            );
            return;
        }

        // Release connect lock
        state.connect_lock = None;
        state.connect_handle = None;

        match result {
            Ok(ConnectResult::Connected { features }) => {
                state.status = ConnectionStatus::Connected;
                state.features = Some(features.clone());
                state.error = None;

                // Mark as connected before (for Reconnect button)
                state.has_connected_before = true;

                info!(
                    server_id = %key.server_id,
                    space_id = %key.space_id,
                    "Server connected"
                );

                self.emit(DomainEvent::ServerStatusChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                    status: self.to_core_status(ConnectionStatus::Connected),
                    flow_id: state.flow_id,
                    has_connected_before: true,
                    message: None,
                    features: Some(self.to_discovered_capabilities(&features)),
                });
            }
            Ok(ConnectResult::AuthRequired) => {
                state.status = ConnectionStatus::AuthRequired;
                state.error = None;

                info!(
                    server_id = %key.server_id,
                    space_id = %key.space_id,
                    has_connected_before = state.has_connected_before,
                    "Server requires OAuth"
                );

                self.emit(DomainEvent::ServerStatusChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                    status: self.to_core_status(ConnectionStatus::AuthRequired),
                    flow_id: state.flow_id,
                    has_connected_before: state.has_connected_before,
                    message: None,
                    features: None,
                });
            }
            Err(e) => {
                state.status = ConnectionStatus::Error;
                state.error = Some(e.clone());

                error!(
                    server_id = %key.server_id,
                    space_id = %key.space_id,
                    error = %e,
                    "Server connection failed"
                );

                self.emit(DomainEvent::ServerStatusChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                    status: self.to_core_status(ConnectionStatus::Error),
                    flow_id: state.flow_id,
                    has_connected_before: state.has_connected_before,
                    message: Some(e),
                    features: None,
                });
            }
        }
    }

    /// Handle OAuth timeout - Reserved for event-driven architecture
    #[allow(dead_code)]
    async fn on_auth_timeout(&self, key: &ServerKey, flow_id: u64) {
        let entry = match self.states.get(key) {
            Some(e) => e,
            None => return,
        };

        let mut state = entry.write().await;

        // Stale check
        if state.flow_id != flow_id {
            debug!(
                server_id = %key.server_id,
                "Ignoring stale auth timeout"
            );
            return;
        }

        if state.status != ConnectionStatus::Authenticating {
            return; // Already transitioned
        }

        // Cleanup
        if let Some(auth) = state.auth.take() {
            if let Some(h) = auth.listener_handle {
                h.abort();
            }
        }
        state.auth_lock = None;

        state.status = ConnectionStatus::AuthRequired;
        state.error = Some("Authentication timed out".to_string());

        warn!(
            server_id = %key.server_id,
            space_id = %key.space_id,
            "OAuth timed out"
        );

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::AuthRequired),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: Some("Timed out".to_string()),
            features: None,
        });
    }

    /// Handle OAuth error - Reserved for event-driven architecture
    #[allow(dead_code)]
    async fn on_auth_error(&self, key: &ServerKey, flow_id: u64, error: &str) {
        let entry = match self.states.get(key) {
            Some(e) => e,
            None => return,
        };

        let mut state = entry.write().await;

        if state.flow_id != flow_id {
            return;
        }

        // Cleanup
        if let Some(auth) = state.auth.take() {
            if let Some(h) = auth.timeout_handle {
                h.abort();
            }
            if let Some(h) = auth.listener_handle {
                h.abort();
            }
        }
        state.auth_lock = None;

        state.status = ConnectionStatus::AuthRequired;
        state.error = Some(error.to_string());

        error!(
            server_id = %key.server_id,
            space_id = %key.space_id,
            error = %error,
            "OAuth error"
        );

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::AuthRequired),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: Some(error.to_string()),
            features: None,
        });
    }

    /// Handle OAuth callback success
    pub async fn on_auth_callback(
        &self,
        key: &ServerKey,
        flow_id: u64,
        code: &str,
    ) -> Result<(), String> {
        let entry = match self.states.get(key) {
            Some(e) => e,
            None => return Err("Server not found".to_string()),
        };

        let mut state = entry.write().await;

        // Race condition checks
        if state.status != ConnectionStatus::Authenticating {
            return Err("Not authenticating".to_string());
        }
        if state.flow_id != flow_id {
            return Err("Stale callback".to_string());
        }

        // Cancel timeout (we won the race)
        if let Some(auth) = state.auth.take() {
            if let Some(h) = auth.timeout_handle {
                h.abort();
            }
            if let Some(h) = auth.listener_handle {
                h.abort();
            }
        }

        // Release auth lock
        state.auth_lock = None;

        info!(
            server_id = %key.server_id,
            space_id = %key.space_id,
            "OAuth callback received, exchanging tokens..."
        );

        // Set to Connecting while we exchange tokens and connect
        state.status = ConnectionStatus::Connecting;
        let flow_id_for_connect = state.flow_id;

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::Connecting),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: Some("Exchanging tokens...".to_string()),
            features: None,
        });

        drop(state);
        drop(entry);

        // Note: Token exchange and connection is now handled by Tauri commands
        // This callback will be triggered externally and the result passed to set_connected() or set_error()
        let _flow_id = flow_id_for_connect;
        let _code = code;

        Ok(())
    }

    // =========================================================================
    // Public State Update Methods (called by Tauri commands after actual work)
    // =========================================================================

    /// Update server state to Connecting (called before attempting connection)
    pub async fn set_connecting(&self, key: &ServerKey) {
        let entry = self.get_or_create_state(key.clone());
        let mut state = entry.write().await;

        state.flow_id += 1;
        state.status = ConnectionStatus::Connecting;
        state.error = None;

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::Connecting),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: None,
            features: None,
        });
    }

    /// Update server state to Connected with features
    pub async fn set_connected(&self, key: &ServerKey, features: CachedFeatures) {
        let entry = self.get_or_create_state(key.clone());
        let mut state = entry.write().await;

        state.status = ConnectionStatus::Connected;
        state.has_connected_before = true;
        state.features = Some(features.clone());
        state.error = None;
        state.connect_lock = None;

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::Connected),
            flow_id: state.flow_id,
            has_connected_before: true,
            message: None,
            features: Some(self.to_discovered_capabilities(&features)),
        });

        // Also emit FeaturesUpdated event for UI to refresh features
        let feature_count = features.total_count();
        if feature_count > 0 {
            info!(
                server_id = %key.server_id,
                feature_count = feature_count,
                "[ServerManager] Emitting features updated event"
            );
            // Generate added list from feature names
            let mut added = Vec::new();
            added.extend(features.tools.iter().map(|t| t.feature_name.clone()));
            added.extend(features.prompts.iter().map(|p| p.feature_name.clone()));
            added.extend(features.resources.iter().map(|r| r.feature_name.clone()));

            self.emit(DomainEvent::ServerFeaturesRefreshed {
                server_id: key.server_id.clone(),
                space_id: key.space_id,
                features: self.to_discovered_capabilities(&features),
                added,
                removed: Vec::new(),
            });

            // Emit MCP list_changed notifications for clients to re-fetch
            if !features.tools.is_empty() {
                self.emit(DomainEvent::ToolsChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                });
            }
            if !features.prompts.is_empty() {
                self.emit(DomainEvent::PromptsChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                });
            }
            if !features.resources.is_empty() {
                self.emit(DomainEvent::ResourcesChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                });
            }
        }

        info!(server_id = %key.server_id, "[ServerManager] Connected successfully");
    }

    /// Update server state to AuthRequired (OAuth needed)
    pub async fn set_auth_required(&self, key: &ServerKey, message: Option<String>) {
        let entry = self.get_or_create_state(key.clone());
        let mut state = entry.write().await;

        state.status = ConnectionStatus::AuthRequired;
        state.error = message.clone();
        state.connect_lock = None;

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::AuthRequired),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message,
            features: None,
        });

        info!(server_id = %key.server_id, "[ServerManager] Auth required");
    }

    /// Update server state to Authenticating (OAuth flow started)
    pub async fn set_authenticating(&self, key: &ServerKey, auth_url: String) {
        let entry = self.get_or_create_state(key.clone());
        let mut state = entry.write().await;

        state.flow_id += 1;
        state.status = ConnectionStatus::Authenticating;
        state.error = None;

        let now = Instant::now();
        state.auth = Some(AuthFlowState {
            auth_url: auth_url.clone(),
            started_at: now,
            browser_opened_at: now,
            timeout_handle: None,
            listener_handle: None,
        });

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::Authenticating),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: Some("Waiting for OAuth callback via deep link".to_string()),
            features: None,
        });

        info!(server_id = %key.server_id, "[ServerManager] Authenticating via deep link");
    }

    /// Update server state to Error
    pub async fn set_error(&self, key: &ServerKey, error: String) {
        let entry = self.get_or_create_state(key.clone());
        let mut state = entry.write().await;

        state.status = ConnectionStatus::Error;
        state.error = Some(error.clone());
        state.connect_lock = None;
        state.auth_lock = None;
        state.auth = None;

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::Error),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: Some(error),
            features: None,
        });

        warn!(server_id = %key.server_id, "[ServerManager] Error state");
    }

    /// Update server state to Disconnected
    pub async fn set_disconnected(&self, key: &ServerKey) {
        let entry = self.get_or_create_state(key.clone());
        let mut state = entry.write().await;

        // Check if server had features before disconnecting
        let had_features = state.features.is_some();
        let had_tools = state
            .features
            .as_ref()
            .map(|f| !f.tools.is_empty())
            .unwrap_or(false);
        let had_prompts = state
            .features
            .as_ref()
            .map(|f| !f.prompts.is_empty())
            .unwrap_or(false);
        let had_resources = state
            .features
            .as_ref()
            .map(|f| !f.resources.is_empty())
            .unwrap_or(false);

        state.status = ConnectionStatus::Disconnected;
        state.error = None;
        state.features = None;
        state.auth = None;
        state.connect_lock = None;
        state.auth_lock = None;

        self.emit(DomainEvent::ServerStatusChanged {
            server_id: key.server_id.clone(),
            space_id: key.space_id,
            status: self.to_core_status(ConnectionStatus::Disconnected),
            flow_id: state.flow_id,
            has_connected_before: state.has_connected_before,
            message: None,
            features: None,
        });

        // Emit MCP list_changed notifications if server had features
        if had_features {
            if had_tools {
                self.emit(DomainEvent::ToolsChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                });
            }
            if had_prompts {
                self.emit(DomainEvent::PromptsChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                });
            }
            if had_resources {
                self.emit(DomainEvent::ResourcesChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                });
            }
        }

        info!(server_id = %key.server_id, "[ServerManager] Disconnected");
    }

    /// Check if we should debounce browser open (within 2s of last open)
    pub async fn should_debounce_browser(&self, key: &ServerKey) -> bool {
        if let Some(entry) = self.states.get(key) {
            let state = entry.read().await;
            if let Some(auth) = &state.auth {
                return auth.browser_opened_at.elapsed() < BROWSER_DEBOUNCE;
            }
        }
        false
    }

    /// Update browser opened timestamp
    pub async fn update_browser_opened(&self, key: &ServerKey) {
        if let Some(entry) = self.states.get(key) {
            let mut state = entry.write().await;
            if let Some(auth) = &mut state.auth {
                auth.browser_opened_at = Instant::now();
            }
        }
    }

    /// Get the current auth URL if in Authenticating state
    pub async fn get_auth_url(&self, key: &ServerKey) -> Option<String> {
        if let Some(entry) = self.states.get(key) {
            let state = entry.read().await;
            if let Some(auth) = &state.auth {
                return Some(auth.auth_url.clone());
            }
        }
        None
    }

    /// Check if server is in a specific status
    pub async fn is_status(&self, key: &ServerKey, expected: ConnectionStatus) -> bool {
        if let Some(entry) = self.states.get(key) {
            let state = entry.read().await;
            return state.status == expected;
        }
        false
    }

    /// Get the feature service for feature operations
    pub fn feature_service(&self) -> Arc<FeatureService> {
        self.feature_service.clone()
    }

    /// Get the connection service for connection operations
    pub fn connection_service(&self) -> Arc<ConnectionService> {
        self.connection_service.clone()
    }

    /// Open browser with auth URL (without terminal flash on Windows)
    pub fn open_browser(&self, url: &str) {
        info!(url = %url, "[ServerManager] Opening browser for OAuth");

        // Log browser opening (if we have server context)
        // Note: This is called from various places, so we log at the call site instead

        if let Err(e) = open_url_no_flash(url) {
            error!(url = %url, error = %e, "[ServerManager] Failed to open browser");
        }
    }

    // =========================================================================
    // RefreshService - Startup + Periodic Feature Refresh
    // =========================================================================

    /// Refresh all enabled servers at startup
    ///
    /// Called once when the app starts. For each enabled server:
    /// - If has token: try to connect
    /// - If no token/auth required: set AuthRequired status
    /// - Emits status events for UI to display current state
    pub async fn startup_refresh(&self, enabled_servers: Vec<(Uuid, String)>) {
        info!(
            count = enabled_servers.len(),
            "[RefreshService] Starting refresh for all enabled servers"
        );

        // Process servers in parallel
        let tasks: Vec<_> = enabled_servers
            .into_iter()
            .map(|(space_id, server_id)| {
                let key = ServerKey::new(space_id, server_id.clone());
                async move {
                    self.refresh_single_server(&key).await;
                }
            })
            .collect();

        // Wait for all to complete
        futures::future::join_all(tasks).await;

        info!("[RefreshService] Startup refresh complete");
    }

    /// Refresh a single server (check connection, refresh features if connected)
    async fn refresh_single_server(&self, key: &ServerKey) {
        let entry = self.get_or_create_state(key.clone());
        let state = entry.write().await;

        // Skip if already in a transient state
        match state.status {
            ConnectionStatus::Connecting
            | ConnectionStatus::Authenticating
            | ConnectionStatus::Refreshing => {
                trace!(server_id = %key.server_id, status = ?state.status, "[RefreshService] Skipping - transient state");
                return;
            }
            _ => {}
        }

        // Try to acquire refresh lock
        let refresh_mutex = state.refresh_mutex.clone();
        let lock = match refresh_mutex.try_lock_owned() {
            Ok(guard) => guard,
            Err(_) => {
                trace!(server_id = %key.server_id, "[RefreshService] Refresh already in progress");
                return;
            }
        };

        let was_connected = state.status == ConnectionStatus::Connected;

        // If connected, refresh features WITHOUT changing status
        if was_connected {
            // Don't change status or emit events for feature refresh
            // Status should only change if server actually disconnects
            trace!(server_id = %key.server_id, "[RefreshService] Feature refresh starting (status unchanged)");

            // Note: Feature refresh would be done via FeatureService
            // For now, just log that we checked
            // In the future: call FeatureService.refresh_features() here

            drop(lock);

            trace!(server_id = %key.server_id, "[RefreshService] Feature refresh complete (no changes)");
        } else {
            // Not connected - nothing to refresh
            drop(lock);
            trace!(
                server_id = %key.server_id,
                status = ?state.status,
                "[RefreshService] Skipping - not connected"
            );
        }
    }

    /// Handle refresh completion
    /// Future feature - not yet used
    #[allow(dead_code)]
    async fn on_refresh_complete(&self, key: &ServerKey, result: Result<CachedFeatures, String>) {
        let Some(entry) = self.states.get(key) else {
            return;
        };
        let mut state = entry.write().await;

        // Release refresh lock
        state.refresh_lock = None;

        match result {
            Ok(new_features) => {
                // Compare with old features for diff
                let (added, removed) = if let Some(old) = &state.features {
                    compute_feature_diff(old, &new_features)
                } else {
                    (vec![], vec![])
                };

                let has_changes = !added.is_empty() || !removed.is_empty();

                state.status = ConnectionStatus::Connected;
                state.features = Some(new_features.clone());
                state.error = None;

                self.emit(DomainEvent::ServerStatusChanged {
                    server_id: key.server_id.clone(),
                    space_id: key.space_id,
                    status: self.to_core_status(ConnectionStatus::Connected),
                    flow_id: state.flow_id,
                    has_connected_before: state.has_connected_before,
                    message: None,
                    features: Some(self.to_discovered_capabilities(&new_features)),
                });

                // Emit features updated if there are changes
                if has_changes {
                    self.emit(DomainEvent::ServerFeaturesRefreshed {
                        server_id: key.server_id.clone(),
                        space_id: key.space_id,
                        features: self.to_discovered_capabilities(&new_features),
                        added: added.clone(),
                        removed: removed.clone(),
                    });

                    // Emit MCP list_changed notifications for changed feature types
                    let tools_changed = added.iter().any(|f| f.starts_with("tool:"))
                        || removed.iter().any(|f| f.starts_with("tool:"));
                    let prompts_changed = added.iter().any(|f| f.starts_with("prompt:"))
                        || removed.iter().any(|f| f.starts_with("prompt:"));
                    let resources_changed = added.iter().any(|f| f.starts_with("resource:"))
                        || removed.iter().any(|f| f.starts_with("resource:"));

                    if tools_changed {
                        self.emit(DomainEvent::ToolsChanged {
                            server_id: key.server_id.clone(),
                            space_id: key.space_id,
                        });
                    }
                    if prompts_changed {
                        self.emit(DomainEvent::PromptsChanged {
                            server_id: key.server_id.clone(),
                            space_id: key.space_id,
                        });
                    }
                    if resources_changed {
                        self.emit(DomainEvent::ResourcesChanged {
                            server_id: key.server_id.clone(),
                            space_id: key.space_id,
                        });
                    }
                }

                info!(server_id = %key.server_id, "[RefreshService] Refresh complete");
            }
            Err(e) => {
                warn!(server_id = %key.server_id, error = %e, "[RefreshService] Refresh failed");

                // Check if it's an auth error
                if e.contains("auth")
                    || e.contains("token")
                    || e.contains("401")
                    || e.contains("unauthorized")
                {
                    state.status = ConnectionStatus::AuthRequired;
                    state.error = Some(e.clone());

                    self.emit(DomainEvent::ServerStatusChanged {
                        server_id: key.server_id.clone(),
                        space_id: key.space_id,
                        status: self.to_core_status(ConnectionStatus::AuthRequired),
                        flow_id: state.flow_id,
                        has_connected_before: state.has_connected_before,
                        message: Some(format!("Token expired: {}", e)),
                        features: None,
                    });
                } else {
                    state.status = ConnectionStatus::Error;
                    state.error = Some(e.clone());

                    self.emit(DomainEvent::ServerStatusChanged {
                        server_id: key.server_id.clone(),
                        space_id: key.space_id,
                        status: self.to_core_status(ConnectionStatus::Error),
                        flow_id: state.flow_id,
                        has_connected_before: state.has_connected_before,
                        message: Some(e),
                        features: None,
                    });
                }
            }
        }
    }

    /// Actually refresh features from a connected server
    /// Future feature - not yet used
    #[allow(dead_code)]
    async fn do_refresh_features(&self, _key: &ServerKey) -> Result<CachedFeatures, String> {
        // TODO: Delegate to feature_service
        // For now, return placeholder
        Err("Not implemented".to_string())
    }

    /// Start periodic refresh loop (call this once at startup)
    ///
    /// Runs every REFRESH_INTERVAL (60s) and refreshes features for all connected servers
    pub fn start_periodic_refresh(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(REFRESH_INTERVAL);

            loop {
                interval.tick().await;

                // Collect connected servers
                let connected_keys: Vec<ServerKey> = {
                    let mut keys = Vec::new();
                    for entry in self.states.iter() {
                        let state = entry.value().read().await;
                        if state.status == ConnectionStatus::Connected {
                            keys.push(entry.key().clone());
                        }
                    }
                    keys
                };

                if connected_keys.is_empty() {
                    continue;
                }

                debug!(
                    count = connected_keys.len(),
                    "[RefreshService] Periodic refresh starting"
                );

                // Refresh each connected server
                for key in connected_keys {
                    self.refresh_single_server(&key).await;
                }
            }
        })
    }
}

/// Result of a connection attempt
#[derive(Debug)]
pub enum ConnectResult {
    /// Successfully connected
    Connected { features: CachedFeatures },
    /// OAuth required
    AuthRequired,
}

/// Compute diff between old and new features - Reserved for feature change notifications
#[allow(dead_code)]
fn compute_feature_diff(old: &CachedFeatures, new: &CachedFeatures) -> (Vec<String>, Vec<String>) {
    use std::collections::HashSet;

    let old_names: HashSet<&str> = old
        .tools
        .iter()
        .map(|t| t.feature_name.as_str())
        .chain(old.prompts.iter().map(|p| p.feature_name.as_str()))
        .chain(old.resources.iter().map(|r| r.feature_name.as_str()))
        .collect();

    let new_names: HashSet<&str> = new
        .tools
        .iter()
        .map(|t| t.feature_name.as_str())
        .chain(new.prompts.iter().map(|p| p.feature_name.as_str()))
        .chain(new.resources.iter().map(|r| r.feature_name.as_str()))
        .collect();

    let added: Vec<String> = new_names
        .difference(&old_names)
        .map(|s: &&str| s.to_string())
        .collect();
    let removed: Vec<String> = old_names
        .difference(&new_names)
        .map(|s: &&str| s.to_string())
        .collect();

    (added, removed)
}
