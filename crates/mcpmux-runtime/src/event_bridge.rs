//! Bridge the gateway's internal `DomainEvent` broadcast channel into the
//! runtime's shared [`mcpmux_core::EventBus`].
//!
//! The gateway crate owns its own `tokio::sync::broadcast::Sender<DomainEvent>`
//! (`mcpmux_gateway::GatewayState::domain_event_tx`) that drives MCPNotifier,
//! the OAuth event handler, and (historically) the desktop bridge. The
//! runtime's [`mcpmux_core::EventBus`] is the channel that the
//! `*AppService` types emit into.
//!
//! `spawn_event_bridge` subscribes to the gateway's channel and re-emits
//! every event into the shared bus, so:
//!
//! - Application services see both their own events and gateway events.
//! - Desktop / daemon / future CLI subscribers can observe the full stream
//!   by listening to the shared bus alone (the desktop's
//!   `map_domain_event_to_ui` should be re-pointed at this bus in a
//!   follow-up PR; today it still reads the gateway channel directly).
//!
//! The subscription is established synchronously *before* the bridge task
//! is spawned, so any event emitted after the `await` returns is captured
//! — there is no startup window in which events get dropped.
//!
//! Lag is handled the same way `EventBus::recv` handles it — log a
//! warning and continue.

use std::sync::Arc;

use mcpmux_core::SharedEventBus;
use tokio::sync::RwLock;
use tracing::warn;

use mcpmux_gateway::GatewayState;

/// Subscribe to the gateway's broadcast and spawn the forwarding task.
///
/// Returns the `JoinHandle` so the caller can `abort()` it during
/// shutdown. The subscription is established before this function
/// returns, so events emitted immediately after `await`ing are captured.
pub async fn spawn_event_bridge(
    runtime_event_bus: SharedEventBus,
    gateway_state: Arc<RwLock<GatewayState>>,
) -> tokio::task::JoinHandle<()> {
    let mut gateway_rx = {
        let state = gateway_state.read().await;
        state.subscribe_domain_events()
    };

    tokio::spawn(async move {
        while let Ok(event) = gateway_rx.recv().await {
            runtime_event_bus.sender().emit(event);
        }
        warn!("[runtime] gateway event bridge: gateway channel closed, exiting");
    })
}
