#![cfg(windows)]

use std::path::Path;
use std::process::{Child, Command};
use std::time::Duration;

use mcpmux_control::{
    read_frame, write_frame, RequestEnvelope, ResponseEnvelope, PROTOCOL_VERSION,
};
use tokio::io::BufReader;
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::time::{sleep, timeout};

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Spawn `mcpmuxd` against a disposable data dir and wait until
/// the control Named Pipe accepts a connection.
async fn spawn_daemon(data_dir: &Path) -> (Child, u16) {
    let port = free_port();
    let port_arg = port.to_string();
    let child = Command::new(env!("CARGO_BIN_EXE_mcpmuxd"))
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "--key-provider",
            "auto",
            "--port",
            &port_arg,
        ])
        .spawn()
        .unwrap();

    let pipe_path = mcpmux_runtime::control_socket_path(data_dir);
    let pipe_str = pipe_path.to_str().unwrap();

    let ready = timeout(Duration::from_secs(15), async {
        loop {
            if ClientOptions::new().open(pipe_str).is_ok() {
                return;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await;

    if ready.is_err() {
        let mut child = child;
        let _ = child.kill();
        let _ = child.wait();
        panic!("mcpmuxd control pipe did not become ready at {pipe_str}");
    }

    (child, port)
}

async fn call(pipe_str: &str, request: &RequestEnvelope) -> ResponseEnvelope {
    let client = ClientOptions::new().open(pipe_str).unwrap();
    let (read_half, mut write_half) = tokio::io::split(client);
    write_frame(&mut write_half, request).await.unwrap();
    let mut reader = BufReader::new(read_half);
    read_frame(&mut reader).await.unwrap()
}

fn request(request_id: &str, method: &str) -> RequestEnvelope {
    RequestEnvelope {
        version: PROTOCOL_VERSION,
        request_id: request_id.to_string(),
        method: method.to_string(),
        params: serde_json::json!({}),
    }
}

#[tokio::test]
async fn daemon_serves_named_pipe_ping_and_status() {
    let data_dir = tempfile::tempdir().unwrap();
    let (mut child, port) = spawn_daemon(data_dir.path()).await;
    let pipe_path = mcpmux_runtime::control_socket_path(data_dir.path());
    let pipe_str = pipe_path.to_str().unwrap();

    let pong = call(pipe_str, &request("p1", "ping")).await;
    assert!(pong.ok, "ping failed: {:?}", pong.error);
    assert_eq!(pong.data.unwrap()["pong"], true);

    let status = call(pipe_str, &request("s1", "status")).await;
    assert!(status.ok, "status failed: {:?}", status.error);
    let data = status.data.unwrap();
    assert_eq!(data["port"], port);
    assert_eq!(data["pid"], child.id());

    let _ = child.kill();
    let _ = child.wait();
}

#[tokio::test]
async fn daemon_serves_health_endpoint() {
    let data_dir = tempfile::tempdir().unwrap();
    let (mut child, port) = spawn_daemon(data_dir.path()).await;

    let client = reqwest::Client::new();
    let health_url = format!("http://127.0.0.1:{port}/health");
    let response = client.get(&health_url).send().await.unwrap();
    assert!(response.status().is_success());

    let _ = child.kill();
    let _ = child.wait();
}

#[tokio::test]
async fn daemon_refuses_to_start_when_pipe_is_already_bound() {
    let data_dir = tempfile::tempdir().unwrap();
    let (mut child, _port) = spawn_daemon(data_dir.path()).await;

    let port2 = free_port();
    let port_arg = port2.to_string();
    let second = Command::new(env!("CARGO_BIN_EXE_mcpmuxd"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "--key-provider",
            "auto",
            "--port",
            &port_arg,
        ])
        .output()
        .unwrap();

    assert!(
        !second.status.success(),
        "second daemon on same data_dir must fail"
    );

    let _ = child.kill();
    let _ = child.wait();
}
