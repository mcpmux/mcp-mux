//! Notification Emitter Trait
//!
//! Abstraction for emitting MCP list_changed notifications.
//! Follows Dependency Inversion Principle - services depend on this trait,
//! not concrete implementations.

use uuid::Uuid;

/// Trait for emitting MCP notifications to connected clients
///
/// Implementations send notifications via broadcast channel, SSE, or other mechanisms.
///
/// **Object Safety**: Uses `&str` instead of `impl Into<String>` for trait object compatibility.
pub trait NotificationEmitter: Send + Sync {
    /// Emit tools/list_changed notification for a specific server in a space
    fn emit_tools_changed(&self, server_id: &str, space_id: Uuid);

    /// Emit prompts/list_changed notification for a specific server in a space
    fn emit_prompts_changed(&self, server_id: &str, space_id: Uuid);

    /// Emit resources/list_changed notification for a specific server in a space
    fn emit_resources_changed(&self, server_id: &str, space_id: Uuid);

    /// Emit all list_changed notifications (tools/prompts/resources) for a space
    ///
    /// Use "*" as server_id to indicate all servers.
    fn emit_all_changed_for_space(&self, space_id: Uuid);
}
