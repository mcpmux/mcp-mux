//! Event Emitter Service
//!
//! SRP: Provides a simple interface for emitting domain events.
//! This service can be used by external components (like desktop commands)
//! to trigger notifications without directly accessing the event channel.

use mcpmux_core::DomainEvent;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Service for emitting domain events
///
/// SRP: Single responsibility - emit events to the domain event bus
#[derive(Clone)]
pub struct EventEmitter {
    event_tx: broadcast::Sender<DomainEvent>,
}

impl EventEmitter {
    /// Create a new event emitter
    pub fn new(event_tx: broadcast::Sender<DomainEvent>) -> Self {
        Self { event_tx }
    }

    /// Emit a tools list changed notification for a space
    pub fn emit_tools_changed(&self, server_id: impl Into<String>, space_id: Uuid) {
        let server_id = server_id.into();
        tracing::info!(
            server_id = %server_id,
            space_id = %space_id,
            "[EventEmitter] 🔔 Emitting tools/list_changed"
        );
        let result = self.event_tx.send(DomainEvent::ToolsChanged {
            server_id: server_id.clone(),
            space_id,
        });
        if let Err(e) = result {
            tracing::warn!(
                "[EventEmitter] Failed to emit tools_changed: {} (no subscribers)",
                e
            );
        }
    }

    /// Emit a prompts list changed notification for a space
    pub fn emit_prompts_changed(&self, server_id: impl Into<String>, space_id: Uuid) {
        let server_id = server_id.into();
        tracing::info!(
            server_id = %server_id,
            space_id = %space_id,
            "[EventEmitter] 🔔 Emitting prompts/list_changed"
        );
        let result = self.event_tx.send(DomainEvent::PromptsChanged {
            server_id: server_id.clone(),
            space_id,
        });
        if let Err(e) = result {
            tracing::warn!(
                "[EventEmitter] Failed to emit prompts_changed: {} (no subscribers)",
                e
            );
        }
    }

    /// Emit a resources list changed notification for a space
    pub fn emit_resources_changed(&self, server_id: impl Into<String>, space_id: Uuid) {
        let server_id = server_id.into();
        tracing::info!(
            server_id = %server_id,
            space_id = %space_id,
            "[EventEmitter] 🔔 Emitting resources/list_changed"
        );
        let result = self.event_tx.send(DomainEvent::ResourcesChanged {
            server_id: server_id.clone(),
            space_id,
        });
        if let Err(e) = result {
            tracing::warn!(
                "[EventEmitter] Failed to emit resources_changed: {} (no subscribers)",
                e
            );
        }
    }

    /// Emit all list changed notifications for a space
    ///
    /// This is useful when grants change - we don't know which specific
    /// features changed, so we notify about all of them.
    pub fn emit_all_changed_for_space(&self, space_id: Uuid) {
        tracing::info!(
            space_id = %space_id,
            "[EventEmitter] 🔔 Emitting ALL list_changed notifications (tools/prompts/resources)"
        );
        // Use "*" as a wildcard server_id to indicate "all servers"
        self.emit_tools_changed("*", space_id);
        self.emit_prompts_changed("*", space_id);
        self.emit_resources_changed("*", space_id);
    }
}

// Implement NotificationEmitter trait for dependency inversion (SOLID)
impl super::NotificationEmitter for EventEmitter {
    fn emit_tools_changed(&self, server_id: &str, space_id: Uuid) {
        Self::emit_tools_changed(self, server_id, space_id);
    }

    fn emit_prompts_changed(&self, server_id: &str, space_id: Uuid) {
        Self::emit_prompts_changed(self, server_id, space_id);
    }

    fn emit_resources_changed(&self, server_id: &str, space_id: Uuid) {
        Self::emit_resources_changed(self, server_id, space_id);
    }

    fn emit_all_changed_for_space(&self, space_id: Uuid) {
        Self::emit_all_changed_for_space(self, space_id);
    }
}
