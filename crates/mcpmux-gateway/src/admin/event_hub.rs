//! Admin SSE event hub — fans in EventBus, gateway domain events, and direct emits.

use std::sync::Arc;

use mcpmux_core::{ApplicationServices, DomainEvent, EventReceiver};
use parking_lot::Mutex;
use tokio::sync::{broadcast, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, info};

use super::ui_events::{map_domain_event_to_ui, AdminUiEventBus, UiEvent};

/// Merged outbound bus for admin SSE clients.
#[derive(Clone)]
pub struct AdminEventHub {
    outbound: broadcast::Sender<UiEvent>,
    ui_event_bus: Arc<AdminUiEventBus>,
    services_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    direct_ui_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    gateway_task: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl AdminEventHub {
    /// Create a new admin event hub.
    pub fn new(ui_event_bus: Arc<AdminUiEventBus>) -> Self {
        let (outbound, _) = broadcast::channel(512);
        Self {
            outbound,
            ui_event_bus,
            services_task: Arc::new(Mutex::new(None)),
            direct_ui_task: Arc::new(Mutex::new(None)),
            gateway_task: Arc::new(RwLock::new(None)),
        }
    }

    /// Direct UI event bus for Tauri `app.emit` fan-in.
    pub fn ui_event_bus(&self) -> Arc<AdminUiEventBus> {
        self.ui_event_bus.clone()
    }

    /// Subscribe to merged UI events for SSE streaming.
    pub fn subscribe(&self) -> broadcast::Receiver<UiEvent> {
        self.outbound.subscribe()
    }

    /// True when at least one browser is actively streaming SSE.
    pub fn has_sse_subscribers(&self) -> bool {
        self.outbound.receiver_count() > 0
    }

    /// Publish a mapped domain event to SSE subscribers.
    fn publish_domain(&self, event: DomainEvent) {
        let (channel, payload) = map_domain_event_to_ui(&event);
        let _ = self.outbound.send(UiEvent {
            channel: channel.to_string(),
            payload,
        });
    }

    /// Abort a fan-in task if it is still running.
    fn abort_task(slot: &mut Option<JoinHandle<()>>) {
        if let Some(handle) = slot.take() {
            handle.abort();
        }
    }

    /// Start background fan-in from ApplicationServices EventBus and direct UI bus.
    pub fn start(&self, services: Arc<ApplicationServices>) {
        {
            let mut slot = self.services_task.lock();
            Self::abort_task(&mut slot);
        }

        let hub = self.clone();
        let handle = tokio::spawn(async move {
            let mut rx: EventReceiver = services.subscribe();
            info!("[AdminEventHub] ApplicationServices EventBus fan-in started");
            while let Some(event) = rx.recv().await {
                hub.publish_domain(event);
            }
            debug!("[AdminEventHub] ApplicationServices EventBus fan-in stopped");
        });
        *self.services_task.lock() = Some(handle);

        {
            let mut slot = self.direct_ui_task.lock();
            Self::abort_task(&mut slot);
        }

        let hub = self.clone();
        let mut direct_rx = self.ui_event_bus.subscribe();
        let handle = tokio::spawn(async move {
            info!("[AdminEventHub] Direct UI event fan-in started");
            loop {
                match direct_rx.recv().await {
                    Ok(event) => {
                        let _ = hub.outbound.send(event);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            debug!("[AdminEventHub] Direct UI event fan-in stopped");
        });
        *self.direct_ui_task.lock() = Some(handle);
    }

    /// Subscribe to gateway runtime domain events when the MCP gateway starts.
    pub async fn register_gateway_events(&self, domain_event_tx: broadcast::Sender<DomainEvent>) {
        if let Some(handle) = self.gateway_task.write().await.take() {
            handle.abort();
        }

        let hub = self.clone();
        let handle = tokio::spawn(async move {
            let mut rx = domain_event_tx.subscribe();
            info!("[AdminEventHub] Gateway domain event fan-in started");
            loop {
                match rx.recv().await {
                    Ok(event) => hub.publish_domain(event),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            debug!("[AdminEventHub] Gateway domain event fan-in stopped");
        });

        *self.gateway_task.write().await = Some(handle);
    }

    /// Stop gateway domain event fan-in when the MCP gateway stops.
    pub async fn clear_gateway_events(&self) {
        if let Some(handle) = self.gateway_task.write().await.take() {
            handle.abort();
        }
    }

    /// Test-only publish of a direct channel event (integration tests / Playwright).
    #[cfg(any(test, feature = "test-utils"))]
    pub fn publish_test_event(&self, channel: &str, payload: serde_json::Value) {
        self.ui_event_bus.publish(channel, payload);
    }

    /// Count active EventBus fan-in tasks (integration tests for router rebuild idempotency).
    #[cfg(any(test, feature = "test-utils"))]
    pub fn active_fan_in_task_count(&self) -> usize {
        let services = self.services_task.lock().is_some() as usize;
        let direct = self.direct_ui_task.lock().is_some() as usize;
        services + direct
    }
}

impl Default for AdminEventHub {
    fn default() -> Self {
        Self::new(Arc::new(AdminUiEventBus::new()))
    }
}
