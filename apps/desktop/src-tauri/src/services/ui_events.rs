//! Shared desktop UI event emission (Tauri + admin SSE fan-in).

use mcpmux_gateway::admin::ui_events::AdminUiEventBus;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

/// Emit a UI channel event to the Tauri webview and admin SSE subscribers.
pub fn emit_ui_channel(
    app: &AppHandle,
    ui_event_bus: Option<&AdminUiEventBus>,
    channel: &str,
    payload: Value,
) {
    if let Err(e) = app.emit(channel, payload.clone()) {
        tracing::warn!("[UI] Failed to emit {channel}: {e}");
    }
    if let Some(bus) = ui_event_bus {
        bus.publish(channel, payload);
    }
}
