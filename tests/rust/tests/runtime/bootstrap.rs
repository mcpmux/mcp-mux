//! End-to-end runtime bootstrap test.
//!
//! Builds a [`mcpmux_runtime::Runtime`] against a fresh `TempDir`,
//! verifies the resulting state (data dir, migrations, repositories,
//! JWT secret, event bus), then constructs and spawns a `GatewayServer`,
//! probes `/health`, and shuts it down. Mirrors what `apps/daemon`
//! does on startup; if this passes, the daemon's first-run path is
//! covered.

use std::time::Duration;

use mcpmux_gateway::GatewayConfig;
use mcpmux_runtime::{wait_for_health, HealthCheckConfig, HealthStatus};
use tokio::time::sleep;

use super::{next_test_port, Fixture};

#[tokio::test]
async fn runtime_initialises_against_empty_data_dir() {
    let fx = Fixture::new();

    let runtime = super::runtime_builder()
        .with_data_dir(fx.data_dir())
        .build()
        .await
        .expect("runtime build");

    // The runtime took ownership of the data dir.
    assert!(runtime.data_dir.is_dir(), "data dir should exist");
    assert!(runtime.spaces_dir.is_dir(), "spaces dir should exist");
    assert!(runtime.logs_dir.is_dir(), "logs dir should exist");
    assert!(runtime.db_path.is_file(), "sqlite db should exist");
    assert!(
        runtime.lock.path().exists(),
        "lockfile should exist while runtime is alive"
    );

    // Migrations ran — the schema_migrations table is created on first open.
    // We assert by trying a benign query through the settings repo.
    runtime
        .repositories
        .app_settings
        .set("test.bootstrap", "ok")
        .await
        .expect("settings set");
    let read_back = runtime
        .repositories
        .app_settings
        .get("test.bootstrap")
        .await
        .expect("settings get");
    assert_eq!(read_back.as_deref(), Some("ok"));

    // The JWT secret is loaded (test-mode uses the file fallback because
    // the headless host has no Secret Service).
    assert!(
        runtime.jwt_secret.is_some(),
        "JWT secret should be loaded from the file key provider"
    );

    // Event bus is alive and accept subscribers.
    let mut rx = runtime.subscribe_events();
    runtime
        .event_bus
        .sender()
        .emit(mcpmux_core::DomainEvent::GatewayStopped);
    let event = tokio::time::timeout(Duration::from_millis(250), rx.recv())
        .await
        .expect("event received within timeout")
        .expect("event is not None");
    assert_eq!(event.type_name(), "gateway_stopped");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_boots_gateway_and_serves_health() {
    let fx = Fixture::new();
    let port = next_test_port();

    let runtime = super::runtime_builder()
        .with_data_dir(fx.data_dir())
        .build()
        .await
        .expect("runtime build");

    let config = GatewayConfig {
        host: "127.0.0.1".to_string(),
        port,
        public_base_url: None,
        enable_cors: true,
    };

    let server = runtime
        .build_gateway_server(config)
        .await
        .expect("gateway build");
    let handle = server.spawn();

    // Give axum a moment to bind. The actual probe waits with its own
    // backoff, so a slow bind is OK.
    sleep(Duration::from_millis(20)).await;

    let probe = HealthCheckConfig::new(
        format!("http://127.0.0.1:{port}/health"),
        Duration::from_secs(2),
    );
    match wait_for_health(&probe).await {
        HealthStatus::Ok { .. } => {}
        HealthStatus::Unreachable {
            attempts,
            last_error,
        } => {
            panic!("/health did not return 200 after {attempts} attempts: {last_error}");
        }
        HealthStatus::InvalidUrl(e) => panic!("invalid health probe URL: {e}"),
    }

    // Graceful shutdown — same code path as the daemon's `shutdown_gateway_handle`.
    mcpmux_runtime::shutdown_gateway_handle(handle).await;
}

#[tokio::test]
async fn runtime_drop_releases_lock_for_next_process() {
    let fx = Fixture::new();

    {
        let _runtime = super::runtime_builder()
            .with_data_dir(fx.data_dir())
            .build()
            .await
            .expect("first runtime build");

        // Second acquire against the same data dir must fail with a
        // known owner (us).
        let err = match super::runtime_builder()
            .with_data_dir(fx.data_dir())
            .build()
            .await
        {
            Ok(_) => panic!("second build should fail while first holds the lock"),
            Err(e) => e,
        };
        assert!(err.is_lock_held(), "expected lock-held, got {err}");
        assert!(
            err.to_string().contains("pid"),
            "operator-facing error should mention the owner pid, got: {err}"
        );

        // The first runtime drops here, releasing the lock.
    }

    // Once the first runtime is gone, a fresh build must succeed.
    let _runtime2 = super::runtime_builder()
        .with_data_dir(fx.data_dir())
        .build()
        .await
        .expect("third build should succeed after first runtime dropped");
}

/// With every stored secret bound, startup records it and switches the
/// encryptor to refuse unbound values; a value that isn't bound keeps it off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_refuses_unbound_values_once_everything_is_bound() {
    let fx = Fixture::new();
    {
        let runtime = super::runtime_builder()
            .with_data_dir(fx.data_dir())
            .build()
            .await
            .expect("runtime build");
        assert!(runtime.encryptor.requires_bound());
        assert!(runtime.database.lock().await.ciphertexts_bound());
    }

    // A fresh data dir with a value from an earlier version (unbound, but
    // readable: it gets bound) and one that can't be read, so can't be bound:
    // legacy reads stay on. The readable one also shows the key is right
    // (with a single key, as on Windows, nothing readable means a wrong key).
    let fx = Fixture::new();
    {
        let runtime = super::runtime_builder()
            .with_data_dir(fx.data_dir())
            .build()
            .await
            .expect("runtime build");
        let legacy = runtime.encryptor.encrypt("token").unwrap();
        runtime
            .database
            .lock()
            .await
            .connection()
            .execute(
                "INSERT INTO credentials (id, space_id, server_id, credential_type, credential_value, created_at, updated_at)
                 VALUES ('c0', (SELECT id FROM spaces LIMIT 1), 'srv', 'access_token', ?1, 'now', 'now'),
                        ('c1', (SELECT id FROM spaces LIMIT 1), 'srv', 'api_key', 'garbage', 'now', 'now')",
                [legacy],
            )
            .unwrap();
        // Undo the marker the first start recorded on this empty database.
        runtime
            .database
            .lock()
            .await
            .connection()
            .execute(
                "DELETE FROM app_settings WHERE key = 'storage.ciphertexts_bound'",
                [],
            )
            .unwrap();
    }
    let runtime = super::runtime_builder()
        .with_data_dir(fx.data_dir())
        .build()
        .await
        .expect("runtime build");
    assert!(!runtime.encryptor.requires_bound());
    assert!(!runtime.database.lock().await.ciphertexts_bound());
    let readable: String = runtime
        .database
        .lock()
        .await
        .connection()
        .query_row(
            "SELECT credential_value FROM credentials WHERE id = 'c0'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        mcpmux_storage::FieldEncryptor::is_bound(&readable),
        "the readable value was bound at startup"
    );
}
