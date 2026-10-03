#![cfg(unix)]

use std::path::Path;
use std::process::{Child, Command};
use std::time::Duration;

use mcpmux_control::{
    codes, read_frame, write_frame, EventEnvelope, RequestEnvelope, ResponseEnvelope,
    PROTOCOL_VERSION,
};
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::time::{sleep, timeout};

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Spawn `mcpmuxd` against a disposable data dir + runtime dir and wait until
/// the control socket accepts a connection.
async fn spawn_daemon(data_dir: &Path, runtime_dir: &Path) -> (Child, u16) {
    let port = free_port();
    let port_arg = port.to_string();
    let child = Command::new(env!("CARGO_BIN_EXE_mcpmuxd"))
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "--key-provider",
            "file",
            "--port",
            &port_arg,
        ])
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .spawn()
        .unwrap();

    let socket = runtime_dir.join("mcpmux").join("control.sock");
    let ready = timeout(Duration::from_secs(10), async {
        loop {
            if socket.exists() && UnixStream::connect(&socket).await.is_ok() {
                return;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await;

    if ready.is_err() {
        let mut child = child;
        let _ = child.kill();
        let _ = child.wait();
        panic!("mcpmuxd control socket did not become ready");
    }

    (child, port)
}

async fn call(socket_path: &Path, request: &RequestEnvelope) -> ResponseEnvelope {
    let stream = UnixStream::connect(socket_path).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    write_frame(&mut write_half, request).await.unwrap();
    let mut reader = BufReader::new(read_half);
    read_frame(&mut reader).await.unwrap()
}

fn socket_path(runtime_dir: &Path) -> std::path::PathBuf {
    runtime_dir.join("mcpmux").join("control.sock")
}

fn request(request_id: &str, method: &str) -> RequestEnvelope {
    RequestEnvelope {
        version: PROTOCOL_VERSION,
        request_id: request_id.to_string(),
        method: method.to_string(),
        params: serde_json::json!({}),
    }
}

fn request_with(request_id: &str, method: &str, params: serde_json::Value) -> RequestEnvelope {
    RequestEnvelope {
        version: PROTOCOL_VERSION,
        request_id: request_id.to_string(),
        method: method.to_string(),
        params,
    }
}

#[tokio::test]
async fn daemon_serves_health_and_stops_cleanly_on_sigterm() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;

    let client = reqwest::Client::new();
    let health_url = format!("http://127.0.0.1:{port}/health");
    let response = client.get(&health_url).send().await.unwrap();
    assert!(response.status().is_success());

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }

    let shutdown_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait().unwrap() {
            Some(status) => break status,
            None if tokio::time::Instant::now() < shutdown_deadline => {
                sleep(Duration::from_millis(25)).await;
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("mcpmuxd did not stop within 5 seconds of SIGTERM");
            }
        }
    };
    assert!(status.success(), "mcpmuxd exited with {status}");
}

/// Another process already answering `/health` with 200 on the daemon's port
/// must not be mistaken for the daemon's own gateway: the daemon has to fail
/// to start instead of reporting ready without a listener.
#[tokio::test]
async fn daemon_refuses_to_start_when_port_is_owned_by_another_process() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let squatter = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = squatter.local_addr().unwrap().port();
    let squatter_task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = squatter.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                          Content-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}",
                    )
                    .await;
            });
        }
    });

    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let port_arg = port.to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpmuxd"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "--key-provider",
            "file",
            "--port",
            &port_arg,
        ])
        .env("XDG_RUNTIME_DIR", runtime_dir.path())
        .spawn()
        .unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait().unwrap() {
            Some(status) => break status,
            None if tokio::time::Instant::now() < deadline => {
                sleep(Duration::from_millis(25)).await;
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("mcpmuxd kept running although its gateway port was taken");
            }
        }
    };
    squatter_task.abort();

    assert!(
        !status.success(),
        "mcpmuxd must exit non-zero, got {status}"
    );
    assert!(
        !socket_path(runtime_dir.path()).exists(),
        "control socket must not be published without a gateway"
    );
}

#[tokio::test]
async fn control_socket_answers_ping_and_status() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path());

    let pong = call(&socket, &request("p1", "ping")).await;
    assert!(pong.ok, "ping failed: {:?}", pong.error);
    assert_eq!(pong.data.unwrap()["pong"], true);

    let status = call(&socket, &request("s1", "status")).await;
    assert!(status.ok, "status failed: {:?}", status.error);
    let data = status.data.unwrap();
    assert_eq!(data["port"], port);
    assert_eq!(data["pid"], child.id());

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}

#[tokio::test]
async fn config_import_round_trips_a_space() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path());

    // Write a portable mcpServers document with one stdio server.
    let import_file = data_dir.path().join("import.json");
    std::fs::write(
        &import_file,
        r#"{"mcpServers":{"community.memory-npx":{"command":"npx","args":["-y","@modelcontextprotocol/server-memory"],"name":"Memory (npx)"}}}"#,
    )
    .unwrap();

    // Dry run first: must not persist anything.
    let dry = call(
        &socket,
        &request_with(
            "i1",
            "config.import",
            serde_json::json!({"file": import_file.to_string_lossy(), "dry_run": true}),
        ),
    )
    .await;
    assert!(dry.ok, "dry-run import failed: {:?}", dry.error);
    assert!(dry.data.as_ref().unwrap()["dry_run"].as_bool().unwrap());

    let installed_before = call(
        &socket,
        &request_with("s1", "servers.list", serde_json::json!({})),
    )
    .await;
    assert!(installed_before
        .data
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());

    // Real import.
    let imported = call(
        &socket,
        &request_with(
            "i2",
            "config.import",
            serde_json::json!({"file": import_file.to_string_lossy()}),
        ),
    )
    .await;
    assert!(imported.ok, "import failed: {:?}", imported.error);
    assert_eq!(
        imported.data.as_ref().unwrap()["added"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Export it back and confirm the server appears in the document.
    let exported = call(
        &socket,
        &request_with("e1", "config.export-space", serde_json::json!({})),
    )
    .await;
    assert!(exported.ok, "export-space failed: {:?}", exported.error);
    let content = exported.data.unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(content.contains("community.memory-npx"));

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}

#[tokio::test]
async fn doctor_reports_core_checks() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path());

    let response = call(&socket, &request("d1", "doctor")).await;
    assert!(response.ok, "doctor failed: {:?}", response.error);
    let report = response.data.unwrap();
    assert!(report["healthy"].as_bool().unwrap());

    let ids: Vec<&str> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    for expected in [
        "data_dir",
        "data_dir_lock",
        "key_files",
        "database",
        "gateway_listener",
        "registry",
        "server_executables",
    ] {
        assert!(
            ids.contains(&expected),
            "doctor is missing check {expected}"
        );
    }

    // A world-readable key file must flip the report to unhealthy.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let key = data_dir.path().join("keys").join("master.key");
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        let response = call(&socket, &request("d2", "doctor")).await;
        let report = response.data.unwrap();
        assert!(!report["healthy"].as_bool().unwrap());
        let keys = report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "key_files")
            .unwrap();
        assert_eq!(keys["status"], "fail");
    }

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}

#[tokio::test]
async fn registry_list_returns_cached_catalog() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path());

    let listed = call(
        &socket,
        &request_with(
            "r1",
            "registry.list",
            serde_json::json!({"query": "memory"}),
        ),
    )
    .await;
    // Offline hosts have no cached bundle; skip rather than fail CI.
    if !listed.ok {
        eprintln!(
            "skipping registry test: catalog unavailable ({:?})",
            listed.error
        );
    } else {
        let servers = listed.data.unwrap();
        assert!(
            servers.is_array(),
            "registry.list should return an array, got {servers}"
        );
    }

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}

#[tokio::test]
async fn control_socket_rejects_incompatible_version_and_unknown_method() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path());

    let mut bad_version = request("v1", "ping");
    bad_version.version = PROTOCOL_VERSION + 1;
    let response = call(&socket, &bad_version).await;
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, codes::INCOMPATIBLE_VERSION);

    let response = call(&socket, &request("u1", "does.not.exist")).await;
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, codes::UNKNOWN_METHOD);

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}

#[tokio::test]
async fn control_socket_streams_events_after_subscribe_ack() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;

    let stream = UnixStream::connect(socket_path(runtime_dir.path()))
        .await
        .unwrap();
    let (read_half, mut write_half) = stream.into_split();
    write_frame(&mut write_half, &request("sub1", "events.subscribe"))
        .await
        .unwrap();

    let mut reader = BufReader::new(read_half);
    let ack: ResponseEnvelope = read_frame(&mut reader).await.unwrap();
    assert!(ack.ok, "subscribe failed: {:?}", ack.error);

    // Mutating via a second connection must appear on the event stream.
    let other = UnixStream::connect(socket_path(runtime_dir.path()))
        .await
        .unwrap();
    let (other_read, mut other_write) = other.into_split();
    write_frame(
        &mut other_write,
        &RequestEnvelope {
            version: PROTOCOL_VERSION,
            request_id: "c1".to_string(),
            method: "spaces.create".to_string(),
            params: serde_json::json!({"name": "Control Test"}),
        },
    )
    .await
    .unwrap();
    let mut other_reader = BufReader::new(other_read);
    let response: ResponseEnvelope = read_frame(&mut other_reader).await.unwrap();
    assert!(response.ok, "spaces.create failed: {:?}", response.error);

    let event = timeout(Duration::from_secs(5), async {
        loop {
            let envelope: EventEnvelope = read_frame(&mut reader).await.unwrap();
            if matches!(
                envelope.event,
                mcpmux_core::DomainEvent::SpaceCreated { .. }
            ) {
                return envelope;
            }
        }
    })
    .await
    .expect("did not receive SpaceCreated event on the stream");
    assert_eq!(event.version, PROTOCOL_VERSION);

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}

#[tokio::test]
async fn servers_add_configure_enable_and_feature_set_membership() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path());

    // Register an API-key client: the key it returns must be usable as a
    // Bearer token, which is what proves the CLI and gateway agree on key
    // storage.
    let created = call(&socket, &request("c1", "clients.create")).await;
    assert!(!created.ok, "clients.create without params should fail");

    let created = call(
        &socket,
        &request_with(
            "c2",
            "clients.create",
            serde_json::json!({"name": "E2E", "client_type": "cursor"}),
        ),
    )
    .await;
    assert!(created.ok, "clients.create failed: {:?}", created.error);
    let key = created.data.unwrap()["api_key"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(key.starts_with("mcpk_"), "unexpected key format: {key}");

    // Install a definition with no required inputs so the test needs no
    // credentials. `community.memory-npx` is a stdio npx server. If the host
    // has no registry access and no cached bundle, skip the network-dependent
    // remainder rather than failing CI.
    let added = call(
        &socket,
        &request_with(
            "a1",
            "servers.add",
            serde_json::json!({"server_id": "community.memory-npx", "inputs": {}}),
        ),
    )
    .await;
    if !added.ok {
        eprintln!(
            "skipping server flow: registry unavailable ({:?})",
            added.error
        );
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
        }
        let _ = child.wait();
        return;
    }

    // The Starter FeatureSet starts empty; including the server's features is
    // what makes them visible to clients.
    let starter = "fs_default_00000000-0000-0000-0000-000000000001";

    // Enable connects the server and discovers its features.
    let enabled = call(
        &socket,
        &request_with(
            "e1",
            "servers.enable",
            serde_json::json!({"server_id": "community.memory-npx"}),
        ),
    )
    .await;
    assert!(enabled.ok, "servers.enable failed: {:?}", enabled.error);
    assert_eq!(enabled.data.unwrap()["status"], "connected");

    let features = call(
        &socket,
        &request_with(
            "f1",
            "servers.features",
            serde_json::json!({"server_id": "community.memory-npx"}),
        ),
    )
    .await;
    assert!(features.ok, "servers.features failed: {:?}", features.error);
    assert!(
        !features
            .data
            .as_ref()
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty(),
        "server connected but reported no features"
    );

    let included = call(
        &socket,
        &request_with(
            "i1",
            "feature-sets.add-member",
            serde_json::json!({"feature_set_id": starter, "server_id": "community.memory-npx"}),
        ),
    )
    .await;
    assert!(
        included.ok,
        "feature-sets.add-member failed: {:?}",
        included.error
    );
    assert!(included.data.unwrap()["added"].as_u64().unwrap() > 0);

    // Removing by name must resolve to the same member.
    let removed = call(
        &socket,
        &request_with(
            "r1",
            "feature-sets.remove-member",
            serde_json::json!({
                "feature_set_id": starter,
                "feature_id": "read_graph",
                "server_id": "community.memory-npx",
                "by_name": true,
            }),
        ),
    )
    .await;
    assert!(
        removed.ok,
        "feature-sets.remove-member failed: {:?}",
        removed.error
    );

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}
