//! UI channel mapping and direct admin event bus for web SSE.

use mcpmux_core::DomainEvent;
use serde_json::Value;
use tokio::sync::broadcast;
use tracing::warn;

/// A UI-facing event ready for Tauri emit or SSE fan-out.
#[derive(Debug, Clone)]
pub struct UiEvent {
    /// Tauri / SSE channel name (e.g. `space-changed`).
    pub channel: String,
    /// JSON payload matching the desktop Tauri emit shape.
    pub payload: Value,
}

/// Broadcast bus for events emitted directly from Tauri commands (`app.emit`)
/// without passing through the domain EventBus or gateway domain channel.
#[derive(Clone)]
pub struct AdminUiEventBus {
    tx: broadcast::Sender<UiEvent>,
}

impl AdminUiEventBus {
    /// Create a direct UI event bus with default capacity.
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    /// Create a direct UI event bus with a custom channel capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publish a channel/payload pair to SSE subscribers.
    pub fn publish(&self, channel: impl Into<String>, payload: Value) {
        let event = UiEvent {
            channel: channel.into(),
            payload,
        };
        if self.tx.send(event).is_err() {
            warn!("[AdminUiEventBus] No SSE subscribers for direct UI event");
        }
    }

    /// Subscribe to direct UI events.
    pub fn subscribe(&self) -> broadcast::Receiver<UiEvent> {
        self.tx.subscribe()
    }
}

impl Default for AdminUiEventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a `DomainEvent` to the Tauri channel name and JSON payload the React
/// hooks expect. Shared by the desktop EventBus bridge and admin SSE fan-in.
pub fn map_domain_event_to_ui(event: &DomainEvent) -> (&'static str, Value) {
    match event {
        DomainEvent::SpaceCreated {
            space_id,
            name,
            icon,
        } => (
            "space-changed",
            serde_json::json!({
                "action": "created",
                "space_id": space_id,
                "name": name,
                "icon": icon,
            }),
        ),
        DomainEvent::SpaceUpdated { space_id, name } => (
            "space-changed",
            serde_json::json!({
                "action": "updated",
                "space_id": space_id,
                "name": name,
            }),
        ),
        DomainEvent::SpaceDeleted { space_id } => (
            "space-changed",
            serde_json::json!({
                "action": "deleted",
                "space_id": space_id,
            }),
        ),
        DomainEvent::ServerInstalled {
            space_id,
            server_id,
            server_name,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "installed",
                "space_id": space_id,
                "server_id": server_id,
                "server_name": server_name,
            }),
        ),
        DomainEvent::ServerUninstalled {
            space_id,
            server_id,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "uninstalled",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ServerConfigUpdated {
            space_id,
            server_id,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "config_updated",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ServerEnabled {
            space_id,
            server_id,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "enabled",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ServerDisabled {
            space_id,
            server_id,
        } => (
            "server-changed",
            serde_json::json!({
                "action": "disabled",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ServerVersionChecked {
            space_id,
            server_id,
        } => (
            "server-version-checked",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ServerUpdateAvailable {
            space_id,
            server_id,
            current_version,
            latest_version,
        } => (
            "server-update-available",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
                "current_version": current_version,
                "latest_version": latest_version,
            }),
        ),
        DomainEvent::ServerStatusChanged {
            space_id,
            server_id,
            status,
            flow_id,
            has_connected_before,
            message,
            features,
        } => (
            "server-status-changed",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
                "status": status.as_str(),
                "flow_id": flow_id,
                "has_connected_before": has_connected_before,
                "message": message,
                "features": features.as_ref().map(|f| serde_json::json!({
                    "tools_count": f.tools.len(),
                    "prompts_count": f.prompts.len(),
                    "resources_count": f.resources.len(),
                })),
            }),
        ),
        DomainEvent::ServerAuthProgress {
            space_id,
            server_id,
            remaining_seconds,
            flow_id,
        } => (
            "server-auth-progress",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
                "remaining_seconds": remaining_seconds,
                "flow_id": flow_id,
            }),
        ),
        DomainEvent::ServerFeaturesRefreshed {
            space_id,
            server_id,
            features,
            added,
            removed,
        } => (
            "server-features-refreshed",
            serde_json::json!({
                "space_id": space_id,
                "server_id": server_id,
                "tools_count": features.tools.len(),
                "prompts_count": features.prompts.len(),
                "resources_count": features.resources.len(),
                "added": added,
                "removed": removed,
            }),
        ),
        DomainEvent::FeatureSetCreated {
            space_id,
            feature_set_id,
            name,
            feature_set_type,
        } => (
            "feature-set-changed",
            serde_json::json!({
                "action": "created",
                "space_id": space_id,
                "feature_set_id": feature_set_id,
                "name": name,
                "feature_set_type": feature_set_type,
            }),
        ),
        DomainEvent::FeatureSetUpdated {
            space_id,
            feature_set_id,
            name,
        } => (
            "feature-set-changed",
            serde_json::json!({
                "action": "updated",
                "space_id": space_id,
                "feature_set_id": feature_set_id,
                "name": name,
            }),
        ),
        DomainEvent::FeatureSetDeleted {
            space_id,
            feature_set_id,
        } => (
            "feature-set-changed",
            serde_json::json!({
                "action": "deleted",
                "space_id": space_id,
                "feature_set_id": feature_set_id,
            }),
        ),
        DomainEvent::FeatureSetMembersChanged {
            space_id,
            feature_set_id,
            added_count,
            removed_count,
        } => (
            "feature-set-changed",
            serde_json::json!({
                "action": "members_changed",
                "space_id": space_id,
                "feature_set_id": feature_set_id,
                "added_count": added_count,
                "removed_count": removed_count,
            }),
        ),
        DomainEvent::ClientRegistered {
            client_id,
            client_name,
            registration_type,
        } => (
            "client-changed",
            serde_json::json!({
                "action": "registered",
                "client_id": client_id,
                "client_name": client_name,
                "registration_type": registration_type,
            }),
        ),
        DomainEvent::ClientReconnected {
            client_id,
            client_name,
        } => (
            "client-changed",
            serde_json::json!({
                "action": "reconnected",
                "client_id": client_id,
                "client_name": client_name,
            }),
        ),
        DomainEvent::ClientUpdated { client_id } => (
            "client-changed",
            serde_json::json!({
                "action": "updated",
                "client_id": client_id,
            }),
        ),
        DomainEvent::ClientDeleted { client_id } => (
            "client-changed",
            serde_json::json!({
                "action": "deleted",
                "client_id": client_id,
            }),
        ),
        DomainEvent::ClientTokenIssued { client_id } => (
            "client-changed",
            serde_json::json!({
                "action": "token_issued",
                "client_id": client_id,
            }),
        ),
        DomainEvent::GatewayStarted { url, port } => (
            "gateway-changed",
            serde_json::json!({
                "action": "started",
                "url": url,
                "port": port,
            }),
        ),
        DomainEvent::GatewayStopped => (
            "gateway-changed",
            serde_json::json!({
                "action": "stopped",
            }),
        ),
        DomainEvent::ToolsChanged {
            space_id,
            server_id,
        } => (
            "mcp-notification",
            serde_json::json!({
                "type": "tools_changed",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::PromptsChanged {
            space_id,
            server_id,
        } => (
            "mcp-notification",
            serde_json::json!({
                "type": "prompts_changed",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::ResourcesChanged {
            space_id,
            server_id,
        } => (
            "mcp-notification",
            serde_json::json!({
                "type": "resources_changed",
                "space_id": space_id,
                "server_id": server_id,
            }),
        ),
        DomainEvent::MetaToolInvoked {
            client_id,
            session_id,
            tool_name,
            decision,
            resolved_feature_set_id,
            summary,
        } => (
            "meta-tool-invoked",
            serde_json::json!({
                "client_id": client_id,
                "session_id": session_id,
                "tool_name": tool_name,
                "decision": decision,
                "resolved_feature_set_id": resolved_feature_set_id,
                "summary": summary,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        ),
        DomainEvent::WorkspaceBindingChanged {
            space_id,
            workspace_root,
        } => (
            "workspace-binding-changed",
            serde_json::json!({
                "space_id": space_id,
                "workspace_root": workspace_root,
            }),
        ),
        DomainEvent::SessionRootsChanged => ("session-roots-changed", serde_json::json!({})),
        DomainEvent::WorkspaceNeedsBinding {
            client_id,
            session_id,
            space_id,
            workspace_root,
            space_locked,
        } => (
            "workspace-needs-binding",
            serde_json::json!({
                "client_id": client_id,
                "session_id": session_id,
                "space_id": space_id,
                "workspace_root": workspace_root,
                "space_locked": space_locked,
            }),
        ),
        DomainEvent::ClientGrantChanged {
            client_id,
            space_id,
        } => (
            "client-grant-changed",
            serde_json::json!({
                "client_id": client_id,
                "space_id": space_id,
            }),
        ),
        DomainEvent::BuiltinServerConfigChanged { space_id } => (
            "server-changed",
            serde_json::json!({
                "action": "config-changed",
                "space_id": space_id,
            }),
        ),
        DomainEvent::WorkspaceAppearanceChanged { workspace_root } => (
            "workspace-appearance-changed",
            serde_json::json!({
                "workspace_root": workspace_root,
            }),
        ),
    }
}
