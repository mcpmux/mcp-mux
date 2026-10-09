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
    spawn_daemon_with(data_dir, runtime_dir, &[]).await
}

/// [`spawn_daemon`] with extra command-line flags.
async fn spawn_daemon_with(data_dir: &Path, runtime_dir: &Path, extra: &[&str]) -> (Child, u16) {
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
        .args(extra)
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .spawn()
        .unwrap();

    let socket = socket_path(runtime_dir, data_dir);
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

/// The control socket a daemon with `XDG_RUNTIME_DIR=runtime_dir` and
/// `--data-dir data_dir` binds.
fn socket_path(runtime_dir: &Path, data_dir: &Path) -> std::path::PathBuf {
    mcpmux_runtime::control_dir_under(Some(runtime_dir.to_path_buf()), data_dir)
        .join("control.sock")
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
        !socket_path(runtime_dir.path(), data_dir.path()).exists(),
        "control socket must not be published without a gateway"
    );
}

/// Two daemons with different data dirs (and ports) under one
/// `XDG_RUNTIME_DIR` must each get their own control socket.
#[tokio::test]
async fn daemons_with_different_data_dirs_get_separate_sockets() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let data_a = tempfile::tempdir().unwrap();
    let data_b = tempfile::tempdir().unwrap();
    let (mut a, _) = spawn_daemon(data_a.path(), runtime_dir.path()).await;
    let (mut b, _) = spawn_daemon(data_b.path(), runtime_dir.path()).await;

    for (child, data_dir) in [(&a, data_a.path()), (&b, data_b.path())] {
        let status = call(
            &socket_path(runtime_dir.path(), data_dir),
            &request("s1", "status"),
        )
        .await;
        assert!(status.ok, "status failed: {:?}", status.error);
        assert_eq!(status.data.unwrap()["pid"], child.id());
    }

    for child in [&mut a, &mut b] {
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
        }
        let _ = child.wait();
    }
}

#[tokio::test]
async fn control_socket_answers_ping_and_status() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path(), data_dir.path());

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

/// `--auth-disabled` must only affect the run it was passed to: a later
/// start without the flag enforces auth again.
#[tokio::test]
async fn auth_disabled_flag_is_not_persisted_across_restarts() {
    async fn auth_disabled_reported(data_dir: &Path, runtime_dir: &Path, extra: &[&str]) -> bool {
        let (mut child, _port) = spawn_daemon_with(data_dir, runtime_dir, extra).await;
        let status = call(
            &socket_path(runtime_dir, data_dir),
            &request("s1", "status"),
        )
        .await;
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
        }
        let _ = child.wait();
        assert!(status.ok, "status failed: {:?}", status.error);
        status.data.unwrap()["auth_disabled"].as_bool().unwrap()
    }

    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    assert!(
        auth_disabled_reported(data_dir.path(), runtime_dir.path(), &["--auth-disabled"]).await
    );
    assert!(!auth_disabled_reported(data_dir.path(), runtime_dir.path(), &[]).await);
}

/// A dry run must report what the real import would add, update and remove;
/// an import the sync rejects must not replace the Space file.
#[tokio::test]
async fn config_import_dry_run_matches_real_import_and_rejects_cleanly() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path(), data_dir.path());

    let import = |id: &str, body: &str| {
        let path = data_dir.path().join(format!("{id}.json"));
        std::fs::write(&path, body).unwrap();
        path.to_string_lossy().to_string()
    };
    let installed_ids = || async {
        let list = call(
            &socket,
            &request_with("l", "servers.list", serde_json::json!({})),
        )
        .await;
        let mut ids: Vec<String> = list
            .data
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["server_id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    };

    let first = import(
        "first",
        r#"{"mcpServers":{"alpha":{"command":"echo"},"beta":{"command":"echo"}}}"#,
    );
    let response = call(
        &socket,
        &request_with("i1", "config.import", serde_json::json!({"file": first})),
    )
    .await;
    assert!(response.ok, "import failed: {:?}", response.error);
    assert_eq!(installed_ids().await, vec!["alpha", "beta"]);

    // Dry run of a document holding only beta and gamma.
    let second = import(
        "second",
        r#"{"mcpServers":{"beta":{"command":"echo"},"gamma":{"command":"echo"}}}"#,
    );
    let dry = call(
        &socket,
        &request_with(
            "i2",
            "config.import",
            serde_json::json!({"file": second, "dry_run": true}),
        ),
    )
    .await;
    assert!(dry.ok, "dry run failed: {:?}", dry.error);
    let plan = dry.data.unwrap();
    assert_eq!(plan["added"], serde_json::json!(["gamma"]));
    assert_eq!(plan["updated"], serde_json::json!(["beta"]));
    assert_eq!(plan["removed"], serde_json::json!(["alpha"]));
    assert_eq!(
        installed_ids().await,
        vec!["alpha", "beta"],
        "dry run changed state"
    );

    // Two keys normalizing to one id: rejected, nothing replaced.
    let spaces_dir = data_dir.path().join("spaces");
    let space_file = std::fs::read_dir(&spaces_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "json"))
        .expect("space file written by the first import");
    let before = std::fs::read_to_string(&space_file).unwrap();
    let colliding = import(
        "colliding",
        r#"{"mcpServers":{"My Server":{"command":"echo"},"my_server":{"command":"echo"}}}"#,
    );
    let rejected = call(
        &socket,
        &request_with(
            "i3",
            "config.import",
            serde_json::json!({"file": colliding}),
        ),
    )
    .await;
    assert!(!rejected.ok, "colliding ids must be rejected");
    assert_eq!(std::fs::read_to_string(&space_file).unwrap(), before);
    assert_eq!(installed_ids().await, vec!["alpha", "beta"]);

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}

/// Member changes act on the FeatureSet's own Space, not the default one.
#[tokio::test]
async fn feature_set_members_use_the_feature_sets_own_space() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path(), data_dir.path());

    let default_space = call(&socket, &request("d", "spaces.list"))
        .await
        .data
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["is_default"] == true)
        .map(|s| s["id"].as_str().unwrap().to_string())
        .expect("default space");
    let other = call(
        &socket,
        &request_with("c1", "spaces.create", serde_json::json!({"name": "Other"})),
    )
    .await;
    assert!(other.ok, "spaces.create failed: {:?}", other.error);
    let other_space = other.data.unwrap()["id"].as_str().unwrap().to_string();
    let fs = call(
        &socket,
        &request_with(
            "c2",
            "feature-sets.create",
            serde_json::json!({"space_id": other_space, "name": "Other FS"}),
        ),
    )
    .await;
    assert!(fs.ok, "feature-sets.create failed: {:?}", fs.error);
    let fs_id = fs.data.unwrap()["id"].as_str().unwrap().to_string();

    // No --space: resolved to the feature set's Space, not the default.
    let added = call(
        &socket,
        &request_with(
            "a1",
            "feature-sets.add-member",
            serde_json::json!({"feature_set_id": fs_id, "server_id": "absent"}),
        ),
    )
    .await;
    assert!(!added.ok);
    let message = added.error.unwrap().message;
    assert!(message.contains(&other_space), "{message}");
    assert!(!message.contains(&default_space), "{message}");

    // A conflicting --space is rejected.
    let conflicting = call(
        &socket,
        &request_with(
            "a2",
            "feature-sets.add-member",
            serde_json::json!({
                "feature_set_id": fs_id,
                "server_id": "absent",
                "space_id": default_space,
            }),
        ),
    )
    .await;
    assert!(!conflicting.ok);
    assert!(conflicting
        .error
        .unwrap()
        .message
        .contains("belongs to space"));

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}

/// `servers.configure` merges into the stored maps: keys the file does not
/// mention survive, and `null` removes a key.
#[tokio::test]
async fn servers_configure_merges_instead_of_replacing() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path(), data_dir.path());

    // A user-config server: installed without touching the registry.
    let file = data_dir.path().join("servers.json");
    std::fs::write(&file, r#"{"mcpServers":{"alpha":{"command":"echo"}}}"#).unwrap();
    let imported = call(
        &socket,
        &request_with(
            "i1",
            "config.import",
            serde_json::json!({"file": file.to_string_lossy()}),
        ),
    )
    .await;
    assert!(imported.ok, "import failed: {:?}", imported.error);

    for (id, params) in [
        (
            "c1",
            serde_json::json!({"server_id": "alpha",
                "inputs": {"API_KEY": "k", "ORG_ID": "o"}, "env": {"A": "1", "B": "2"}}),
        ),
        (
            "c2",
            serde_json::json!({"server_id": "alpha",
                "inputs": {"ORG_ID": "o2"}, "env": {"B": null, "C": "3"}}),
        ),
    ] {
        let r = call(&socket, &request_with(id, "servers.configure", params)).await;
        assert!(r.ok, "configure failed: {:?}", r.error);
    }

    let inspect = call(
        &socket,
        &request_with(
            "n1",
            "servers.inspect",
            serde_json::json!({"server_id": "alpha"}),
        ),
    )
    .await;
    assert!(inspect.ok, "inspect failed: {:?}", inspect.error);
    let data = inspect.data.unwrap();
    let server = &data.as_array().unwrap()[0];
    let keys = |field: &str| {
        let mut keys: Vec<String> = server[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k.as_str().unwrap().to_string())
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(keys("configured_inputs"), vec!["API_KEY", "ORG_ID"]);
    assert_eq!(keys("env_overrides"), vec!["A", "C"]);

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}

/// Deleting a client also removes the `<client_id> → Starter` id binding
/// that `clients.create` added.
#[tokio::test]
async fn clients_delete_removes_the_auto_created_binding() {
    let data_dir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path(), runtime_dir.path()).await;
    let socket = socket_path(runtime_dir.path(), data_dir.path());

    let roots = || async {
        let list = call(&socket, &request("w", "workspaces.list")).await;
        assert!(list.ok, "workspaces.list failed: {:?}", list.error);
        list.data
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["workspace_root"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };

    let created = call(
        &socket,
        &request_with(
            "c1",
            "clients.create",
            serde_json::json!({"name": "Temp", "client_type": "cursor"}),
        ),
    )
    .await;
    assert!(created.ok, "clients.create failed: {:?}", created.error);
    let client_id = created.data.unwrap()["id"].as_str().unwrap().to_string();
    assert!(
        roots().await.contains(&client_id),
        "auto-map binding missing"
    );

    let deleted = call(
        &socket,
        &request_with("d1", "clients.delete", serde_json::json!({"id": client_id})),
    )
    .await;
    assert!(deleted.ok, "clients.delete failed: {:?}", deleted.error);
    assert!(!roots().await.contains(&client_id), "binding left behind");

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
    let socket = socket_path(runtime_dir.path(), data_dir.path());

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
    let socket = socket_path(runtime_dir.path(), data_dir.path());

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
    let socket = socket_path(runtime_dir.path(), data_dir.path());

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
    let socket = socket_path(runtime_dir.path(), data_dir.path());

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

    let stream = UnixStream::connect(socket_path(runtime_dir.path(), data_dir.path()))
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
    let other = UnixStream::connect(socket_path(runtime_dir.path(), data_dir.path()))
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

    // Mutations emit on the gateway's channel and reach the stream through
    // the event bridge; that must not deliver the same event twice.
    let duplicate = timeout(Duration::from_millis(500), async {
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
    .await;
    assert!(duplicate.is_err(), "SpaceCreated was delivered twice");

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
    let socket = socket_path(runtime_dir.path(), data_dir.path());

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
    if added.ok {
        // Installing again with a definition that isn't the current one (the
        // registry changed after it was shown) is refused.
        let stale = call(
            &socket,
            &request_with(
                "a0",
                "servers.add",
                serde_json::json!({"server_id": "community.memory-npx", "inputs": {},
                    "expected_transport": {"type": "stdio", "command": "something-else",
                        "args": [], "env": {}, "metadata": {"inputs": []}}}),
            ),
        )
        .await;
        assert!(!stale.ok);
        assert!(
            stale
                .error
                .as_ref()
                .unwrap()
                .message
                .contains("changed since it was shown"),
            "{:?}",
            stale.error
        );
    }
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

    // The Starter FeatureSet starts in auto mode (every server's features).
    // Adding members explicitly still works and switches it to a manual
    // selection that keeps what it already granted.
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
    let connected = |id: &'static str| {
        let socket = socket.clone();
        async move {
            let status = call(&socket, &request(id, "status")).await;
            status.data.unwrap()["connected_servers"].as_u64().unwrap()
        }
    };
    assert_eq!(connected("st1").await, 1);

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

    // Removing the server must also drop it from the connected count.
    let uninstalled = call(
        &socket,
        &request_with(
            "x1",
            "servers.remove",
            serde_json::json!({"server_id": "community.memory-npx"}),
        ),
    )
    .await;
    assert!(
        uninstalled.ok,
        "servers.remove failed: {:?}",
        uninstalled.error
    );
    assert_eq!(connected("st2").await, 0);

    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let _ = child.wait();
}
