//! Shared desktop UI event emission (Tauri + admin SSE fan-in).

use std::sync::Arc;

use mcpmux_gateway::admin::ui_events::AdminUiEventBus;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::AdminServerState;

/// Tauri / SSE channel for inbound OAuth consent deep links.
pub const OAUTH_CONSENT_REQUEST_CHANNEL: &str = "oauth-consent-request";

/// Tauri / SSE channel for OAuth client grant / settings changes.
pub const OAUTH_CLIENT_CHANGED_CHANNEL: &str = "oauth-client-changed";

/// Resolve the admin UI event bus from managed Tauri state, if available.
fn resolve_ui_event_bus<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<AdminUiEventBus>> {
    app.try_state::<Arc<tokio::sync::RwLock<AdminServerState>>>()
        .and_then(|state| {
            state
                .try_read()
                .ok()
                .map(|guard| guard.ui_event_bus.clone())
        })
}

/// Emit a UI channel event to the Tauri webview and admin SSE subscribers.
pub fn emit_ui_channel<R: Runtime>(
    app: &AppHandle<R>,
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

/// Emit a UI channel event, resolving the admin SSE bus from app state.
pub fn emit_ui_channel_from_app<R: Runtime>(app: &AppHandle<R>, channel: &str, payload: Value) {
    let ui_event_bus = resolve_ui_event_bus(app);
    emit_ui_channel(app, ui_event_bus.as_deref(), channel, payload);
}
