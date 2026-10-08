//! PoolService shutdown tests
//!
//! Once the gateway stops, its pool must never spawn another backend: every
//! connect path is refused, and shutdown itself returns promptly.

use std::collections::HashMap;
use std::time::Duration;

use mcpmux_gateway::pool::{ConnectionContext, ConnectionResult, ResolvedTransport};
use tests::services::test_pool_service;
use uuid::Uuid;

fn stdio_ctx(space_id: Uuid, server_id: &str) -> ConnectionContext {
    ConnectionContext::new(
        space_id,
        server_id,
        ResolvedTransport::Stdio {
            // Must never run: a refused connect does not spawn anything.
            command: "mcpmux-test-command-that-does-not-exist".to_string(),
            args: vec![],
            env: HashMap::new(),
            redact: vec![],
        },
    )
}

#[tokio::test]
async fn shutdown_of_idle_pool_returns_promptly() {
    let pool = test_pool_service();
    assert!(!pool.is_shutting_down());

    tokio::time::timeout(Duration::from_secs(1), pool.shutdown())
        .await
        .expect("shutdown of an idle pool should not block");

    assert!(pool.is_shutting_down());
    assert_eq!(pool.stats().total_instances, 0);
}

#[tokio::test]
async fn connect_after_shutdown_is_refused_without_creating_an_instance() {
    let pool = test_pool_service();
    let space_id = Uuid::new_v4();
    pool.shutdown().await;

    let result = pool.connect_server(&stdio_ctx(space_id, "server-1")).await;

    match result {
        ConnectionResult::Failed { error } => {
            assert!(error.contains("stopping or stopped"), "{error}")
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(pool.get_instance(space_id, "server-1").is_none());
    assert_eq!(pool.stats().total_instances, 0);
}

#[tokio::test]
async fn reconnect_after_shutdown_is_refused() {
    let pool = test_pool_service();
    pool.shutdown().await;

    let result = pool.reconnect_instance(Uuid::new_v4(), "server-1").await;

    match result {
        ConnectionResult::Failed { error } => {
            assert!(error.contains("stopping or stopped"), "{error}")
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn shutdown_is_idempotent() {
    let pool = test_pool_service();
    pool.shutdown().await;

    tokio::time::timeout(Duration::from_secs(1), pool.shutdown())
        .await
        .expect("a second shutdown should return immediately");

    assert!(pool.is_shutting_down());
}
