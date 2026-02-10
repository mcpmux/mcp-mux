//! STDIO transport tests
//!
//! Tests for cross-platform child process spawning behavior.
//! Verifies that platform-specific flags (CREATE_NO_WINDOW on Windows,
//! process_group on Unix) are applied correctly and don't break
//! child process communication.

use mcpmux_gateway::pool::transport::configure_child_process_platform;
use std::process::Stdio;
use tokio::process::Command;

/// Verify that `configure_child_process_platform` can be applied to a Command
/// without panicking and the resulting process runs correctly.
#[tokio::test]
async fn test_platform_flags_do_not_break_child_process() {
    // Use a cross-platform command that reads stdin and writes to stdout
    #[cfg(windows)]
    let (program, args) = ("cmd.exe", vec!["/C", "echo", "hello"]);
    #[cfg(unix)]
    let (program, args) = ("echo", vec!["hello"]);

    let mut cmd = Command::new(program);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    configure_child_process_platform(&mut cmd);

    let output = cmd.output().await.expect("Failed to spawn child process");
    assert!(output.status.success(), "Child process exited with error");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().contains("hello"),
        "Expected 'hello' in stdout, got: {stdout}"
    );
}

/// Verify that a child process with platform flags can do bidirectional I/O
/// (stdin -> stdout), which is the pattern used by stdio MCP transports.
#[tokio::test]
async fn test_platform_flags_preserve_stdio_communication() {
    // Use a command that reads from stdin and echoes to stdout
    #[cfg(windows)]
    let mut cmd = Command::new("cmd.exe");
    #[cfg(windows)]
    cmd.args(["/C", "findstr", "."]);

    #[cfg(unix)]
    let mut cmd = Command::new("cat");

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    configure_child_process_platform(&mut cmd);

    let mut child = cmd.spawn().expect("Failed to spawn child process");

    // Write to stdin
    {
        use tokio::io::AsyncWriteExt;
        let stdin = child.stdin.as_mut().expect("stdin not available");
        stdin
            .write_all(b"test message\n")
            .await
            .expect("Failed to write to stdin");
        // Close stdin to signal EOF to the child
        stdin.shutdown().await.expect("Failed to close stdin");
    }

    // Read from stdout
    let output = child
        .wait_with_output()
        .await
        .expect("Failed to wait for child");

    assert!(output.status.success(), "Child process exited with error");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("test message"),
        "Expected stdin->stdout echo, got: {stdout}"
    );
}

/// Verify the transport description format
#[test]
fn test_stdio_transport_description() {
    use mcpmux_gateway::pool::transport::StdioTransport;
    use mcpmux_gateway::pool::Transport;
    use std::collections::HashMap;
    use std::time::Duration;
    use uuid::Uuid;

    let transport = StdioTransport::new(
        "node".to_string(),
        vec!["server.js".to_string()],
        HashMap::new(),
        Uuid::new_v4(),
        "test-server".to_string(),
        None,
        Duration::from_secs(30),
        None,
    );

    assert_eq!(transport.description(), "stdio:node");
    assert_eq!(
        transport.transport_type(),
        mcpmux_core::TransportType::Stdio
    );
}

/// Verify that connect returns Failed for a non-existent command
#[tokio::test]
async fn test_stdio_transport_connect_command_not_found() {
    use mcpmux_gateway::pool::transport::StdioTransport;
    use mcpmux_gateway::pool::{Transport, TransportConnectResult};
    use std::collections::HashMap;
    use std::time::Duration;
    use uuid::Uuid;

    let transport = StdioTransport::new(
        "nonexistent_command_that_does_not_exist_abc123".to_string(),
        vec![],
        HashMap::new(),
        Uuid::new_v4(),
        "test-server".to_string(),
        None,
        Duration::from_secs(5),
        None,
    );

    let result = transport.connect().await;
    match result {
        TransportConnectResult::Failed(msg) => {
            assert!(
                msg.contains("Command not found"),
                "Expected 'Command not found', got: {msg}"
            );
        }
        _ => panic!("Expected TransportConnectResult::Failed for nonexistent command"),
    }
}

/// Verify that configure_child_process_platform can be called multiple times
/// without issues (idempotency).
#[tokio::test]
async fn test_platform_flags_idempotent() {
    #[cfg(windows)]
    let program = "cmd.exe";
    #[cfg(unix)]
    let program = "true";

    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.args(["/C", "exit", "0"]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    // Apply twice - should not panic or cause issues
    configure_child_process_platform(&mut cmd);
    configure_child_process_platform(&mut cmd);

    let status = cmd.status().await.expect("Failed to spawn child process");
    assert!(status.success(), "Child process should exit successfully");
}

/// Verify that a docker command not found error includes a Docker-specific hint
#[tokio::test]
async fn test_docker_command_not_found_includes_hint() {
    use mcpmux_gateway::pool::transport::StdioTransport;
    use mcpmux_gateway::pool::{Transport, TransportConnectResult};
    use std::collections::HashMap;
    use std::time::Duration;
    use uuid::Uuid;

    let transport = StdioTransport::new(
        "docker".to_string(),
        vec![
            "run".to_string(),
            "-i".to_string(),
            "some-image".to_string(),
        ],
        HashMap::new(),
        Uuid::new_v4(),
        "test-docker-server".to_string(),
        None,
        Duration::from_secs(5),
        None,
    );

    let result = transport.connect().await;
    match result {
        TransportConnectResult::Failed(msg) => {
            // If docker is not installed, we get "Command not found" with hint.
            // If docker IS installed but daemon isn't running, we'd get a different error with hint.
            // Either way, the hint should be present.
            assert!(
                msg.contains("Docker Desktop"),
                "Expected Docker hint in error message, got: {msg}"
            );
        }
        // If docker happens to be installed and running, the test still passes
        // (connect would succeed or fail with handshake error that includes the hint)
        TransportConnectResult::Connected(_) => {
            // Docker is installed and running - that's fine, test passes
        }
        TransportConnectResult::OAuthRequired { .. } => {
            panic!("Unexpected OAuthRequired for docker stdio transport")
        }
    }
}

/// Verify that environment variables are passed through correctly
/// when platform flags are applied (important because CREATE_NO_WINDOW
/// is OR'd with CREATE_UNICODE_ENVIRONMENT internally).
#[tokio::test]
async fn test_platform_flags_preserve_env_vars() {
    #[cfg(windows)]
    let mut cmd = Command::new("cmd.exe");
    #[cfg(windows)]
    cmd.args(["/C", "echo", "%MCPMUX_TEST_VAR%"]);

    #[cfg(unix)]
    let mut cmd = Command::new("sh");
    #[cfg(unix)]
    cmd.args(["-c", "echo $MCPMUX_TEST_VAR"]);

    cmd.env("MCPMUX_TEST_VAR", "test_value_42")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    configure_child_process_platform(&mut cmd);

    let output = cmd.output().await.expect("Failed to spawn child process");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("test_value_42"),
        "Expected env var in output, got: {stdout}"
    );
}
