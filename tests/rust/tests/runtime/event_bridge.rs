//! Verifies that the gateway's internal `DomainEvent` broadcast is
//! bridged into the runtime's shared `EventBus`. Future control-socket
//! subscribers (Phase 3+) depend on this so a single subscription sees
//! every event the running process emits.

use std::sync::Arc;
use std::time::Duration;

use mcpmux_core::DomainEvent;
use mcpmux_gateway::GatewayConfig;
use mcpmux_runtime::{spawn_event_bridge, RuntimeBuilder};

use super::{next_test_port, Fixture};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gateway_state_events_flow_through_runtime_event_bus() {
    let fx = Fixture::new();
    let runtime = RuntimeBuilder::new()
        .with_data_dir(fx.data_dir())
        .build()
        .await
        .expect("runtime build");

    let port = next_test_port();
    let config = GatewayConfig {
        host: "127.0.0.1".to_string(),
        port,
        public_base_url: None,
        enable_cors: true,
    };

    // Build the gateway (this creates its internal broadcast channel)
    // but do not spawn — we only need the broadcast to verify the
    // bridge wiring. A live listener is exercised by `bootstrap.rs`.
    let server = runtime
        .build_gateway_server(config)
        .await
        .expect("gateway build");
    let gateway_state: Arc<tokio::sync::RwLock<mcpmux_gateway::GatewayState>> = server.state();

    let _bridge = spawn_event_bridge(runtime.event_bus.clone(), gateway_state.clone()).await;

    let mut rx = runtime.subscribe_events();

    // Emit through the gateway's broadcast — the bridge should re-emit
    // into the runtime's shared bus and reach the subscriber.
    gateway_state
        .read()
        .await
        .emit_domain_event(DomainEvent::GatewayStopped);

    let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("event arrived within timeout")
        .expect("event is not None");
    assert_eq!(event.type_name(), "gateway_stopped");
}
