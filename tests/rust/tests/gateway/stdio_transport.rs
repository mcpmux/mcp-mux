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

/// Verify that stderr from a child process is captured through an OS pipe
/// and can be read line-by-line. Uses std::process::Command and spawn_blocking
/// to ensure clean fd lifecycle.
#[tokio::test]
async fn test_stderr_capture_via_os_pipe() {
    let (reader, writer) = os_pipe::pipe().expect("Failed to create pipe");

    // Start the stderr reader FIRST (before spawning child) on a blocking thread.
    // This mirrors production usage where the reader is spawned before the child connects.
    let reader_handle = tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        let buf_reader = std::io::BufReader::new(reader);
        buf_reader
            .lines()
            .map(|l| l.unwrap())
            .collect::<Vec<String>>()
    });

    // Spawn a child process that writes to stderr using our pipe's write end.
    // Use std::process::Command for predictable fd cleanup.
    let status = tokio::task::spawn_blocking(move || {
        #[cfg(unix)]
        let mut cmd = std::process::Command::new("sh");
        #[cfg(unix)]
        cmd.args([
            "-c",
            "echo 'stderr line 1' >&2; echo 'stderr line 2' >&2; echo 'error: something failed' >&2",
        ]);

        #[cfg(windows)]
        let mut cmd = std::process::Command::new("cmd.exe");
        #[cfg(windows)]
        cmd.args([
            "/C",
            "echo stderr line 1 1>&2 & echo stderr line 2 1>&2 & echo error: something failed 1>&2",
        ]);

        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::from(writer));

        cmd.status().expect("Failed to spawn child process")
    })
    .await
    .expect("spawn_blocking panicked");

    assert!(status.success(), "Child process should exit successfully");

    // Wait for reader to finish (pipe is closed since child exited and writer was consumed)
    let lines = reader_handle.await.expect("Reader task panicked");

    assert!(
        lines.len() >= 3,
        "Expected at least 3 stderr lines, got {}: {:?}",
        lines.len(),
        lines
    );

    let has_stderr = lines.iter().any(|l| l.contains("stderr"));
    let has_error = lines.iter().any(|l| l.contains("error"));
    assert!(
        has_stderr || has_error,
        "Expected stderr or error content, got: {:?}",
        lines
    );
}

/// Verify that stderr capture with ServerLogManager logs process output
/// to the correct location with the correct LogSource.
#[tokio::test]
async fn test_stderr_capture_logs_to_server_log_manager() {
    use mcpmux_core::{LogConfig, LogSource, ServerLogManager};
    use std::sync::Arc;

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let log_config = LogConfig {
        base_dir: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    let log_manager = Arc::new(ServerLogManager::new(log_config));
    let space_id = uuid::Uuid::new_v4();
    let server_id = "test-stderr-server".to_string();

    let (reader, writer) = os_pipe::pipe().expect("Failed to create pipe");

    // Start stderr reader on blocking thread (mirrors production spawn_stderr_reader)
    let lm = Arc::clone(&log_manager);
    let sid = space_id;
    let svid = server_id.clone();
    let reader_handle = tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        let rt = tokio::runtime::Handle::current();
        let buf_reader = std::io::BufReader::new(reader);
        for line_result in buf_reader.lines() {
            match line_result {
                Ok(line) if line.is_empty() => continue,
                Ok(line) => {
                    let log = mcpmux_core::ServerLog::new(
                        mcpmux_core::LogLevel::Info,
                        LogSource::Stderr,
                        &line,
                    );
                    let _ = rt.block_on(lm.append(&sid.to_string(), &svid, log));
                }
                Err(_) => break,
            }
        }
    });

    // Spawn child on a blocking thread with std::process::Command
    let status = tokio::task::spawn_blocking(move || {
        #[cfg(unix)]
        let mut cmd = std::process::Command::new("sh");
        #[cfg(unix)]
        cmd.args([
            "-c",
            "echo '[test-server] Starting...' >&2; echo '[test-server] Ready' >&2",
        ]);

        #[cfg(windows)]
        let mut cmd = std::process::Command::new("cmd.exe");
        #[cfg(windows)]
        cmd.args([
            "/C",
            "echo [test-server] Starting... 1>&2 & echo [test-server] Ready 1>&2",
        ]);

        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::from(writer));

        cmd.status().expect("Failed to spawn child process")
    })
    .await
    .expect("spawn_blocking panicked");

    assert!(status.success());

    // Wait for reader to drain the pipe
    reader_handle.await.expect("Stderr reader task panicked");

    // Small delay for log file write to flush
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Read logs back
    let logs = log_manager
        .read_logs(&space_id.to_string(), &server_id, 100, None)
        .await
        .expect("Failed to read logs");

    assert!(
        !logs.is_empty(),
        "Expected at least one log entry from stderr capture"
    );

    // All logs should have source = Stderr
    for log in &logs {
        assert_eq!(
            log.source,
            LogSource::Stderr,
            "Expected LogSource::Stderr, got {:?}",
            log.source
        );
    }

    let has_starting = logs.iter().any(|l| l.message.contains("Starting"));
    let has_ready = logs.iter().any(|l| l.message.contains("Ready"));
    assert!(
        has_starting || has_ready,
        "Expected 'Starting' or 'Ready' in log messages, got: {:?}",
        logs.iter().map(|l| &l.message).collect::<Vec<_>>()
    );
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
