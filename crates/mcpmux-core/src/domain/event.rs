//! Domain Events - Unified event system for McpMux
//!
//! All domain changes are represented as events in this module.
//! Events are emitted by Application Services and consumed by:
//! - UIBridge (Desktop frontend)
//! - MCPNotifier (External MCP clients)
//! - AuditLogger (Logging/Cloud sync)
//!
//! # Design Principles
//!
//! - **Single Source of Truth**: One enum for all domain events
//! - **Smart Consumers**: Consumers decide which events they care about
//! - **Immutable**: Events are facts that happened, never mutated
//! - **Serializable**: All events can be serialized for transport/storage

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerFeature;

// ============================================================================
// CACHED FEATURES (moved from gateway to core for event payloads)
// ============================================================================

/// Discovered features from an MCP server connection
///
/// This is the payload included in connection events to provide
/// immediate access to discovered capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveredCapabilities {
    pub tools: Vec<ServerFeature>,
    pub prompts: Vec<ServerFeature>,
    pub resources: Vec<ServerFeature>,
}

impl DiscoveredCapabilities {
    /// Create empty capabilities
    pub fn new() -> Self {
        Self::default()
    }

    /// Total number of features
    pub fn total_count(&self) -> usize {
        self.tools.len() + self.prompts.len() + self.resources.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.total_count() == 0
    }

    /// Get all features as a single vector
    pub fn all_features(&self) -> Vec<ServerFeature> {
        let mut all = Vec::with_capacity(self.total_count());
        all.extend(self.tools.iter().cloned());
        all.extend(self.prompts.iter().cloned());
        all.extend(self.resources.iter().cloned());
        all
    }
}

// ============================================================================
// CONNECTION STATUS
// ============================================================================

/// Server connection status
///
/// Unified status enum for both entity persistence and events.
/// Values match database storage for backward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    /// Successfully connected and responding
    Connected,
    /// Not connected (idle state) - this is the default
    #[default]
    Disconnected,
    /// Connection failed with error
    Error,
    /// OAuth authentication required before connecting
    OAuthRequired,
    /// Attempting to connect
    Connecting,
    /// Refreshing features/connection
    Refreshing,
    /// In OAuth authentication flow (waiting for user)
    Authenticating,
}

impl ConnectionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Error => "error",
            Self::OAuthRequired => "oauth_required",
            Self::Connecting => "connecting",
            Self::Refreshing => "refreshing",
            Self::Authenticating => "authenticating",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "connected" => Self::Connected,
            "error" => Self::Error,
            "oauth_required" => Self::OAuthRequired,
            "connecting" => Self::Connecting,
            "refreshing" => Self::Refreshing,
            "authenticating" => Self::Authenticating,
            _ => Self::Disconnected,
        }
    }

    /// Check if the server is currently connected
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected | Self::Refreshing)
    }

    /// Check if this is a terminal state (not transitioning)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Connected | Self::Disconnected | Self::Error | Self::OAuthRequired
        )
    }

    /// Check if this status indicates an error condition
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }

    /// Check if authentication is needed
    pub fn needs_auth(&self) -> bool {
        matches!(self, Self::OAuthRequired | Self::Authenticating)
    }
}

// ============================================================================
// DOMAIN EVENT ENUM
// ============================================================================

/// Unified domain events for the entire McpMux system
///
/// ALL domain state changes are represented in this enum.
/// Application services emit these events after successful operations.
/// Consumers (UIBridge, MCPNotifier, AuditLogger) subscribe and react.
///
/// # Event Categories
///
/// - **Space Events**: Space creation, deletion, activation
/// - **Server Lifecycle**: Install, uninstall, enable, disable
/// - **Server Connection**: Connect, disconnect, auth flow
/// - **Feature Sets**: Create, update, delete, modify members
/// - **Client & Grants**: Register, update, grant/revoke permissions
/// - **Gateway**: Start, stop
/// - **MCP Capabilities**: Tools/prompts/resources changed
///
/// # Serialization
///
/// Events serialize with a `type` field containing the snake_case variant name:
/// ```json
/// { "type": "space_created", "space_id": "...", "name": "..." }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
    // ════════════════════════════════════════════════════════════════════════
    // SPACE MANAGEMENT
    // ════════════════════════════════════════════════════════════════════════
    /// A new space was created
    SpaceCreated {
        space_id: Uuid,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
    },

    /// A space was updated (name, icon, description)
    SpaceUpdated { space_id: Uuid, name: String },

    /// A space was deleted
    SpaceDeleted { space_id: Uuid },

    // ════════════════════════════════════════════════════════════════════════
    // SERVER LIFECYCLE (Configuration)
    // ════════════════════════════════════════════════════════════════════════
    /// A server was installed from registry into a space
    ServerInstalled {
        space_id: Uuid,
        server_id: String,
        server_name: String,
    },

    /// A server was uninstalled from a space
    ServerUninstalled { space_id: Uuid, server_id: String },

    /// Server configuration was updated (inputs, env, etc.)
    ServerConfigUpdated { space_id: Uuid, server_id: String },

    /// Server was enabled (will auto-connect)
    ServerEnabled { space_id: Uuid, server_id: String },

    /// Server was disabled (will disconnect)
    ServerDisabled { space_id: Uuid, server_id: String },

    /// Version probe completed for a server (fired regardless of whether an update is available).
    ServerVersionChecked { space_id: Uuid, server_id: String },

    /// Notify-mode probe found a newer package version than the installed/pinned one.
    ServerUpdateAvailable {
        space_id: Uuid,
        server_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        current_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        latest_version: Option<String>,
    },

    // ════════════════════════════════════════════════════════════════════════
    // SERVER CONNECTION STATE (Runtime)
    // ════════════════════════════════════════════════════════════════════════
    /// Server connection status changed
    ServerStatusChanged {
        space_id: Uuid,
        server_id: String,
        status: ConnectionStatus,
        /// Monotonic flow_id for race condition prevention
        flow_id: u64,
        /// Whether this server has ever connected successfully
        has_connected_before: bool,
        /// Error or status message
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        /// Discovered features (only when status is Connected)
        #[serde(skip_serializing_if = "Option::is_none")]
        features: Option<DiscoveredCapabilities>,
    },

    /// OAuth authentication progress (countdown timer)
    ServerAuthProgress {
        space_id: Uuid,
        server_id: String,
        /// Seconds remaining in auth timeout
        remaining_seconds: u64,
        /// Unique ID for this auth flow (to detect stale updates)
        flow_id: u64,
    },

    /// Server features were refreshed (periodic or manual)
    ServerFeaturesRefreshed {
        space_id: Uuid,
        server_id: String,
        features: DiscoveredCapabilities,
        /// Feature names that were added
        added: Vec<String>,
        /// Feature names that were removed
        removed: Vec<String>,
    },

    // ════════════════════════════════════════════════════════════════════════
    // FEATURE SETS
    // ════════════════════════════════════════════════════════════════════════
    /// A new feature set was created
    FeatureSetCreated {
        space_id: Uuid,
        feature_set_id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        feature_set_type: Option<String>,
    },

    /// A feature set was updated (name, description, icon)
    FeatureSetUpdated {
        space_id: Uuid,
        feature_set_id: String,
        name: String,
    },

    /// A feature set was deleted
    FeatureSetDeleted {
        space_id: Uuid,
        feature_set_id: String,
    },

    /// Feature set members changed (features/sets added or removed)
    FeatureSetMembersChanged {
        space_id: Uuid,
        feature_set_id: String,
        /// Number of members added
        added_count: usize,
        /// Number of members removed
        removed_count: usize,
    },

    // ════════════════════════════════════════════════════════════════════════
    // CLIENT & GRANTS
    // ════════════════════════════════════════════════════════════════════════
    /// A client's per-space FeatureSet grants were added, removed, or
    /// replaced wholesale. MCPNotifier listens and pushes
    /// `notifications/{tools,prompts,resources}/list_changed` to every
    /// peer registered under this `client_id` so they re-fetch under the
    /// new permission set.
    ///
    /// Used only by the rootless-client fallback path (the resolver consults
    /// `client_grants` when a session has no roots and the client did not
    /// declare the MCP `roots` capability). Roots-capable sessions ignore
    /// these grants and continue to route via `WorkspaceBinding`.
    ClientGrantChanged { client_id: String, space_id: Uuid },

    /// An MCP client was registered (Cursor, VS Code, etc.)
    ClientRegistered {
        client_id: String,
        client_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        registration_type: Option<String>,
    },

    /// A previously approved client reconnected (silent approval)
    ClientReconnected {
        client_id: String,
        client_name: String,
    },

    /// A client's settings were updated
    ClientUpdated { client_id: String },

    /// A client was deleted
    ClientDeleted { client_id: String },

    /// A client was issued an access token
    ClientTokenIssued { client_id: String },

    // ════════════════════════════════════════════════════════════════════════
    // GATEWAY
    // ════════════════════════════════════════════════════════════════════════
    /// Gateway server started
    GatewayStarted { url: String, port: u16 },

    /// Gateway server stopped
    GatewayStopped,

    // ════════════════════════════════════════════════════════════════════════
    // MCP CAPABILITY CHANGES (pass-through from backend servers)
    // ════════════════════════════════════════════════════════════════════════
    /// Backend server notified that its tools changed
    ToolsChanged { space_id: Uuid, server_id: String },

    /// Backend server notified that its prompts changed
    PromptsChanged { space_id: Uuid, server_id: String },

    /// Backend server notified that its resources changed
    ResourcesChanged { space_id: Uuid, server_id: String },

    // ════════════════════════════════════════════════════════════════════════
    // WORKSPACE BINDINGS (root → FeatureSet resolution)
    // ════════════════════════════════════════════════════════════════════════
    /// A workspace binding was created, updated, or deleted.
    ///
    /// Emitted by the WorkspaceBinding application service. MCPNotifier
    /// listens for this and broadcasts `notifications/tools/list_changed`
    /// (plus prompts + resources) to every peer in the affected space so
    /// clients re-fetch their tool list under the new routing decision.
    WorkspaceBindingChanged {
        space_id: Uuid,
        workspace_root: String,
    },

    /// A client session resolved via `source=Default` because no binding
    /// matched any of its reported roots. The desktop UI uses this to
    /// prompt the user once per new (space, root) pair to pick a FeatureSet
    /// (or explicitly commit to the default and stop re-prompting).
    ///
    /// NOT fired for rootless sessions — nothing to bind.
    WorkspaceNeedsBinding {
        client_id: String,
        session_id: String,
        space_id: Uuid,
        workspace_root: String,
        /// The folder is scoped to `space_id` by a Space base directory, so the
        /// mapping popup locks its Space field to it (the user only picks the
        /// FeatureSet). `false` for an ordinary unmapped folder, where the user
        /// may bind it to any Space.
        space_locked: bool,
    },

    /// The live set of reported session roots changed (a client connected
    /// and surfaced new folders, or an existing client's roots moved). The
    /// desktop Workspaces tab listens for this and re-fetches the detected
    /// roots list so unbound folders stay visible to the user.
    ///
    /// Payload-less on purpose — the consumer always re-queries; embedding
    /// the roots here would be redundant and race with disconnect cleanup.
    SessionRootsChanged,

    // ════════════════════════════════════════════════════════════════════════
    // META-TOOL AUDIT TRAIL
    // ════════════════════════════════════════════════════════════════════════
    /// A built-in `mcpmux_*` self-management tool was called by an MCP client.
    ///
    /// Emitted by the gateway for every meta-tool invocation (read + write)
    /// so the desktop's Connection Log can show an audit row. For writes,
    /// `decision` records what the user chose in the approval dialog.
    MetaToolInvoked {
        client_id: String,
        session_id: Option<String>,
        tool_name: String,
        /// `"allow_once" | "always_for_this_session_and_client" | "deny" | "timeout" | "read"`
        decision: String,
        /// FeatureSet that became active as a result of the write, when known.
        resolved_feature_set_id: Option<String>,
        /// Redacted summary of the payload the LLM supplied (no secrets).
        summary: String,
    },

    // ════════════════════════════════════════════════════════════════════════
    // BUILT-IN SERVERS
    // ════════════════════════════════════════════════════════════════════════
    /// A Space's built-in-server config changed — a built-in server (e.g. Tool
    /// Optimization / `mcpmux_*`) or one of its tools was enabled/disabled for
    /// this Space. Every session resolving to this Space may now see a
    /// different tool list, so the notifier re-pushes `tools/list_changed` to
    /// that Space's peers.
    BuiltinServerConfigChanged { space_id: Uuid },

    // ════════════════════════════════════════════════════════════════════════
    // WORKSPACE APPEARANCES
    // ════════════════════════════════════════════════════════════════════════
    /// A workspace appearance (icon, theme) was upserted or deleted.
    ///
    /// The desktop Workspaces tab and space switcher listen for this to
    /// refresh their icon display without a full reload.
    WorkspaceAppearanceChanged { workspace_root: String },
}

// ============================================================================
// DOMAIN EVENT IMPLEMENTATION
// ============================================================================

impl DomainEvent {
    /// Get the event type name as a string
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::SpaceCreated { .. } => "space_created",
            Self::SpaceUpdated { .. } => "space_updated",
            Self::SpaceDeleted { .. } => "space_deleted",
            Self::ServerInstalled { .. } => "server_installed",
            Self::ServerUninstalled { .. } => "server_uninstalled",
            Self::ServerConfigUpdated { .. } => "server_config_updated",
            Self::ServerEnabled { .. } => "server_enabled",
            Self::ServerDisabled { .. } => "server_disabled",
            Self::ServerVersionChecked { .. } => "server_version_checked",
            Self::ServerUpdateAvailable { .. } => "server_update_available",
            Self::ServerStatusChanged { .. } => "server_status_changed",
            Self::ServerAuthProgress { .. } => "server_auth_progress",
            Self::ServerFeaturesRefreshed { .. } => "server_features_refreshed",
            Self::FeatureSetCreated { .. } => "feature_set_created",
            Self::FeatureSetUpdated { .. } => "feature_set_updated",
            Self::FeatureSetDeleted { .. } => "feature_set_deleted",
            Self::FeatureSetMembersChanged { .. } => "feature_set_members_changed",
            Self::ClientGrantChanged { .. } => "client_grant_changed",
            Self::ClientRegistered { .. } => "client_registered",
            Self::ClientReconnected { .. } => "client_reconnected",
            Self::ClientUpdated { .. } => "client_updated",
            Self::ClientDeleted { .. } => "client_deleted",
            Self::ClientTokenIssued { .. } => "client_token_issued",
            Self::GatewayStarted { .. } => "gateway_started",
            Self::GatewayStopped => "gateway_stopped",
            Self::ToolsChanged { .. } => "tools_changed",
            Self::PromptsChanged { .. } => "prompts_changed",
            Self::ResourcesChanged { .. } => "resources_changed",
            Self::WorkspaceBindingChanged { .. } => "workspace_binding_changed",
            Self::WorkspaceNeedsBinding { .. } => "workspace_needs_binding",
            Self::SessionRootsChanged => "session_roots_changed",
            Self::MetaToolInvoked { .. } => "meta_tool_invoked",
            Self::BuiltinServerConfigChanged { .. } => "builtin_server_config_changed",
            Self::WorkspaceAppearanceChanged { .. } => "workspace_appearance_changed",
        }
    }

    /// Check if this event affects MCP client capabilities
    ///
    /// Used by MCPNotifier to decide whether to send `list_changed` notifications.
    /// Returns true for events that can change what tools/prompts/resources
    /// a client has access to.
    pub fn affects_mcp_capabilities(&self) -> bool {
        match self {
            // Connection status changes that affect available features
            Self::ServerStatusChanged { status, .. } => {
                // Connected/Disconnected/Refreshing all affect what's available
                status.is_connected() || matches!(status, ConnectionStatus::Disconnected)
            }
            // Feature refresh directly affects capabilities
            Self::ServerFeaturesRefreshed { .. } => true,
            // Feature set member changes affect granted capabilities
            Self::FeatureSetMembersChanged { .. } => true,
            // Per-client grant changes affect what rootless sessions see
            Self::ClientGrantChanged { .. } => true,
            // Backend server notifications
            Self::ToolsChanged { .. }
            | Self::PromptsChanged { .. }
            | Self::ResourcesChanged { .. } => true,
            // Binding changes reshuffle every peer's resolution in the space
            Self::WorkspaceBindingChanged { .. } => true,
            // A Space's built-in-server config changes the tool list every
            // session resolving to that Space sees.
            Self::BuiltinServerConfigChanged { .. } => true,
            // Deleting a Space cascade-removes its bindings; deleting a
            // FeatureSet strips its tools from every binding referencing it.
            // Both leave live sessions holding stale tool lists unless we push
            // list_changed.
            Self::SpaceDeleted { .. } => true,
            Self::FeatureSetDeleted { .. } => true,
            // WorkspaceNeedsBinding is a UI prompt — doesn't itself change what
            // tools a client sees, just invites the user to configure.
            // All other events don't affect MCP capabilities
            _ => false,
        }
    }

    /// Get the space_id if this event is space-scoped
    pub fn space_id(&self) -> Option<Uuid> {
        match self {
            Self::SpaceCreated { space_id, .. }
            | Self::SpaceUpdated { space_id, .. }
            | Self::SpaceDeleted { space_id }
            | Self::ServerInstalled { space_id, .. }
            | Self::ServerUninstalled { space_id, .. }
            | Self::ServerConfigUpdated { space_id, .. }
            | Self::ServerEnabled { space_id, .. }
            | Self::ServerDisabled { space_id, .. }
            | Self::ServerVersionChecked { space_id, .. }
            | Self::ServerUpdateAvailable { space_id, .. }
            | Self::ServerStatusChanged { space_id, .. }
            | Self::ServerAuthProgress { space_id, .. }
            | Self::ServerFeaturesRefreshed { space_id, .. }
            | Self::FeatureSetCreated { space_id, .. }
            | Self::FeatureSetUpdated { space_id, .. }
            | Self::FeatureSetDeleted { space_id, .. }
            | Self::FeatureSetMembersChanged { space_id, .. }
            | Self::ClientGrantChanged { space_id, .. }
            | Self::ToolsChanged { space_id, .. }
            | Self::PromptsChanged { space_id, .. }
            | Self::ResourcesChanged { space_id, .. }
            | Self::WorkspaceBindingChanged { space_id, .. }
            | Self::WorkspaceNeedsBinding { space_id, .. }
            | Self::BuiltinServerConfigChanged { space_id } => Some(*space_id),

            Self::ClientRegistered { .. }
            | Self::ClientReconnected { .. }
            | Self::ClientUpdated { .. }
            | Self::ClientDeleted { .. }
            | Self::ClientTokenIssued { .. }
            | Self::GatewayStarted { .. }
            | Self::GatewayStopped
            | Self::SessionRootsChanged
            | Self::MetaToolInvoked { .. }
            | Self::WorkspaceAppearanceChanged { .. } => None,
        }
    }

    // (grant events removed — routing is via WorkspaceBinding + Space.default FS only)

    /// Get the server_id if this event is server-scoped
    pub fn server_id(&self) -> Option<&str> {
        match self {
            Self::ServerInstalled { server_id, .. }
            | Self::ServerUninstalled { server_id, .. }
            | Self::ServerConfigUpdated { server_id, .. }
            | Self::ServerEnabled { server_id, .. }
            | Self::ServerDisabled { server_id, .. }
            | Self::ServerVersionChecked { server_id, .. }
            | Self::ServerUpdateAvailable { server_id, .. }
            | Self::ServerStatusChanged { server_id, .. }
            | Self::ServerAuthProgress { server_id, .. }
            | Self::ServerFeaturesRefreshed { server_id, .. }
            | Self::ToolsChanged { server_id, .. }
            | Self::PromptsChanged { server_id, .. }
            | Self::ResourcesChanged { server_id, .. } => Some(server_id),
            _ => None,
        }
    }

    /// Get the client_id if this event is client-scoped
    pub fn client_id(&self) -> Option<&str> {
        match self {
            Self::ClientRegistered { client_id, .. }
            | Self::ClientUpdated { client_id, .. }
            | Self::ClientDeleted { client_id, .. }
            | Self::ClientTokenIssued { client_id, .. }
            | Self::ClientGrantChanged { client_id, .. }
            | Self::WorkspaceNeedsBinding { client_id, .. } => Some(client_id),
            _ => None,
        }
    }

    /// Get the feature_set_id if this event is feature-set-scoped
    pub fn feature_set_id(&self) -> Option<&str> {
        match self {
            Self::FeatureSetCreated { feature_set_id, .. }
            | Self::FeatureSetUpdated { feature_set_id, .. }
            | Self::FeatureSetDeleted { feature_set_id, .. }
            | Self::FeatureSetMembersChanged { feature_set_id, .. } => Some(feature_set_id),
            _ => None,
        }
    }

    /// Check if this is a UI-only event (doesn't affect MCP clients)
    pub fn is_ui_only(&self) -> bool {
        !self.affects_mcp_capabilities()
    }

    /// Get timestamp metadata for this event (for audit logging)
    pub fn timestamp(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

// ============================================================================
// EVENT METADATA (for audit logging)
// ============================================================================

/// Metadata wrapper for events (used by audit logger)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainEventEnvelope {
    /// Unique event ID
    pub event_id: Uuid,
    /// When the event occurred
    pub timestamp: DateTime<Utc>,
    /// The actual event
    pub event: DomainEvent,
    /// Optional correlation ID for tracking related events
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<Uuid>,
}

impl DomainEventEnvelope {
    /// Wrap an event with metadata
    pub fn new(event: DomainEvent) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
            correlation_id: None,
        }
    }

    /// Add correlation ID
    pub fn with_correlation_id(mut self, id: Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = DomainEvent::SpaceCreated {
            space_id: Uuid::new_v4(),
            name: "Test Space".to_string(),
            icon: Some("🚀".to_string()),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"space_created\""));
        assert!(json.contains("\"name\":\"Test Space\""));
    }

    #[test]
    fn test_affects_mcp_capabilities() {
        // Feature-set member changes affect every peer that resolves into that set
        let members = DomainEvent::FeatureSetMembersChanged {
            space_id: Uuid::new_v4(),
            feature_set_id: "fs1".to_string(),
            added_count: 1,
            removed_count: 0,
        };
        assert!(members.affects_mcp_capabilities());

        // Space creation doesn't affect capabilities
        let space = DomainEvent::SpaceCreated {
            space_id: Uuid::new_v4(),
            name: "Test".to_string(),
            icon: None,
        };
        assert!(!space.affects_mcp_capabilities());
    }

    #[test]
    fn test_space_id_extraction() {
        let event = DomainEvent::ServerInstalled {
            space_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            server_id: "test-server".to_string(),
            server_name: "Test Server".to_string(),
        };

        assert_eq!(
            event.space_id(),
            Some(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap())
        );
    }

    #[test]
    fn test_event_envelope() {
        let event = DomainEvent::GatewayStarted {
            url: "http://localhost:3100".to_string(),
            port: 3100,
        };

        let envelope = DomainEventEnvelope::new(event);
        assert!(envelope.correlation_id.is_none());

        let with_correlation = envelope.with_correlation_id(Uuid::new_v4());
        assert!(with_correlation.correlation_id.is_some());
    }

    #[test]
    fn test_connection_status() {
        assert!(ConnectionStatus::Connected.is_connected());
        assert!(ConnectionStatus::Refreshing.is_connected());
        assert!(!ConnectionStatus::Disconnected.is_connected());

        assert!(ConnectionStatus::Connected.is_terminal());
        assert!(!ConnectionStatus::Connecting.is_terminal());
    }

    #[test]
    fn test_workspace_binding_changed_affects_capabilities() {
        // Binding writes reshuffle what every peer in the space resolves to
        // — MCPNotifier must broadcast list_changed for this event.
        let e = DomainEvent::WorkspaceBindingChanged {
            space_id: Uuid::new_v4(),
            workspace_root: "/proj/foo".to_string(),
        };
        assert!(e.affects_mcp_capabilities());
        assert_eq!(e.type_name(), "workspace_binding_changed");
        assert!(e.space_id().is_some());
    }

    #[test]
    fn test_workspace_needs_binding_is_ui_only() {
        // The "hey, pick a FeatureSet" prompt is a UI event — it does not
        // itself change tool visibility and must NOT trigger list_changed.
        let e = DomainEvent::WorkspaceNeedsBinding {
            client_id: "client-1".to_string(),
            session_id: "sess-1".to_string(),
            space_id: Uuid::new_v4(),
            workspace_root: "/proj/foo".to_string(),
            space_locked: false,
        };
        assert!(!e.affects_mcp_capabilities());
        assert!(e.is_ui_only());
        assert_eq!(e.type_name(), "workspace_needs_binding");
        assert!(e.space_id().is_some());
        assert_eq!(e.client_id(), Some("client-1"));
    }

    #[test]
    fn test_workspace_events_roundtrip_through_json() {
        // The Tauri bridge serializes these to JSON for the webview; verify
        // the serde tag + fields match what the frontend expects.
        let changed = DomainEvent::WorkspaceBindingChanged {
            space_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            workspace_root: "d:\\proj".to_string(),
        };
        let json = serde_json::to_string(&changed).unwrap();
        assert!(json.contains("\"type\":\"workspace_binding_changed\""));
        assert!(json.contains("\"workspace_root\":\"d:\\\\proj\""));

        let needs = DomainEvent::WorkspaceNeedsBinding {
            client_id: "c".into(),
            session_id: "s".into(),
            space_id: Uuid::nil(),
            workspace_root: "/r".into(),
            space_locked: true,
        };
        let json = serde_json::to_string(&needs).unwrap();
        assert!(json.contains("\"type\":\"workspace_needs_binding\""));
        assert!(json.contains("\"session_id\":\"s\""));
    }
}
