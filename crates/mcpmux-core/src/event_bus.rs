//! Event Bus - Central event distribution system
//!
//! The event bus is the backbone of MCMux's event-driven architecture.
//! All domain events flow through this bus, enabling decoupled communication
//! between producers (application services) and consumers (UI, MCP notifier, audit log).
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     Event Bus (broadcast channel)               │
//! │                                                                  │
//! │  Producers:                    Consumers:                       │
//! │  ├─ SpaceAppService            ├─ UIEventBridge (→ Tauri/React)│
//! │  ├─ ServerAppService           ├─ MCPNotifier (→ list_changed) │
//! │  ├─ PermissionAppService       ├─ AuditLogger (→ disk/cloud)   │
//! │  └─ ClientAppService           └─ Future consumers...          │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! // Create event bus
//! let event_bus = EventBus::new();
//!
//! // Get sender for services
//! let sender = event_bus.sender();
//!
//! // Subscribe consumers
//! let ui_receiver = event_bus.subscribe();
//! let mcp_receiver = event_bus.subscribe();
//!
//! // Emit event from service
//! sender.emit(DomainEvent::SpaceCreated { ... });
//!
//! // Consumers receive asynchronously
//! while let Ok(event) = ui_receiver.recv().await { ... }
//! ```

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::DomainEvent;

/// Default channel capacity for the event bus
const DEFAULT_CAPACITY: usize = 256;

/// Event Bus - Central hub for domain event distribution
///
/// Uses a broadcast channel to allow multiple consumers to receive
/// all events. Each consumer gets its own copy of every event.
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<DomainEvent>,
}

impl EventBus {
    /// Create a new event bus with default capacity
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Create a new event bus with custom capacity
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Get a sender for emitting events
    ///
    /// The sender can be cloned and shared across threads/tasks.
    pub fn sender(&self) -> EventSender {
        EventSender::new(self.sender.clone())
    }

    /// Subscribe to receive events
    ///
    /// Each subscriber gets its own receiver that receives all events
    /// emitted after subscription.
    pub fn subscribe(&self) -> EventReceiver {
        EventReceiver::new(self.sender.subscribe())
    }

    /// Get the raw broadcast sender (for compatibility with existing code)
    pub fn raw_sender(&self) -> broadcast::Sender<DomainEvent> {
        self.sender.clone()
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Event Sender - Used by services to emit domain events
///
/// Thread-safe and cheaply cloneable. Each application service
/// should have its own sender instance.
#[derive(Clone)]
pub struct EventSender {
    sender: broadcast::Sender<DomainEvent>,
}

impl EventSender {
    fn new(sender: broadcast::Sender<DomainEvent>) -> Self {
        Self { sender }
    }

    /// Emit a domain event
    ///
    /// Returns the number of receivers that received the event.
    /// Returns 0 if there are no subscribers (not an error).
    pub fn emit(&self, event: DomainEvent) -> usize {
        let type_name = event.type_name();
        match self.sender.send(event) {
            Ok(count) => {
                debug!(
                    event_type = type_name,
                    receivers = count,
                    "[EventBus] Emitted event"
                );
                count
            }
            Err(_) => {
                // No receivers - this is okay, just means no one is listening
                debug!(
                    event_type = type_name,
                    "[EventBus] No receivers for event"
                );
                0
            }
        }
    }

    /// Emit event and log if no receivers
    pub fn emit_or_warn(&self, event: DomainEvent) {
        let type_name = event.type_name();
        if self.emit(event) == 0 {
            warn!(
                event_type = type_name,
                "[EventBus] Event emitted but no receivers listening"
            );
        }
    }

    /// Check if there are any subscribers
    pub fn has_subscribers(&self) -> bool {
        self.sender.receiver_count() > 0
    }
}

/// Event Receiver - Used by consumers to receive domain events
///
/// Each receiver gets all events emitted after subscription.
/// Use in an async loop to process events.
pub struct EventReceiver {
    receiver: broadcast::Receiver<DomainEvent>,
}

impl EventReceiver {
    fn new(receiver: broadcast::Receiver<DomainEvent>) -> Self {
        Self { receiver }
    }

    /// Receive the next event (async)
    ///
    /// Returns `None` if the channel is closed.
    /// Handles lag gracefully by logging and continuing.
    pub async fn recv(&mut self) -> Option<DomainEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(
                        skipped_events = skipped,
                        "[EventBus] Receiver lagged, skipped {} events",
                        skipped
                    );
                    // Continue to receive next available event
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("[EventBus] Channel closed");
                    return None;
                }
            }
        }
    }

    /// Try to receive an event without blocking
    pub fn try_recv(&mut self) -> Option<DomainEvent> {
        match self.receiver.try_recv() {
            Ok(event) => Some(event),
            Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                warn!(
                    skipped_events = skipped,
                    "[EventBus] Receiver lagged on try_recv"
                );
                // Try again after lag
                self.receiver.try_recv().ok()
            }
            Err(_) => None,
        }
    }
}

/// Shared event bus for application-wide use
///
/// Use this when you need a singleton event bus across the application.
pub type SharedEventBus = Arc<EventBus>;

/// Create a shared event bus
pub fn create_shared_event_bus() -> SharedEventBus {
    Arc::new(EventBus::new())
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_event_bus_basic() {
        let bus = EventBus::new();
        let sender = bus.sender();
        let mut receiver = bus.subscribe();

        // Emit event
        let space_id = Uuid::new_v4();
        sender.emit(DomainEvent::SpaceCreated {
            space_id,
            name: "Test".to_string(),
            icon: None,
        });

        // Receive event
        let event = receiver.recv().await.unwrap();
        assert_eq!(event.type_name(), "space_created");
        assert_eq!(event.space_id(), Some(space_id));
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let sender = bus.sender();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        // Emit event
        sender.emit(DomainEvent::GatewayStarted {
            url: "http://localhost:3100".to_string(),
            port: 3100,
        });

        // Both should receive
        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        assert_eq!(e1.type_name(), "gateway_started");
        assert_eq!(e2.type_name(), "gateway_started");
    }

    #[test]
    fn test_sender_clone() {
        let bus = EventBus::new();
        let sender1 = bus.sender();
        let sender2 = sender1.clone();

        // Both senders should work
        assert!(!sender1.has_subscribers());
        let _rx = bus.subscribe();
        assert!(sender2.has_subscribers());
    }

    #[test]
    fn test_no_receivers() {
        let bus = EventBus::new();
        let sender = bus.sender();

        // Should not panic, just return 0
        let count = sender.emit(DomainEvent::GatewayStopped);
        assert_eq!(count, 0);
    }
}

