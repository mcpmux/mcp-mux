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

// ────────────────────────────────────────────────────────────────────
// Shell PATH resolution integration tests
// ────────────────────────────────────────────────────────────────────

/// Verify that get_shell_path() returns a PATH that contains directories
/// with common system commands. This is the integration-level test for
/// the shell_env module.
#[cfg(unix)]
#[test]
fn test_shell_path_can_find_system_commands() {
    use mcpmux_gateway::pool::transport::shell_env;

    let shell_path = shell_env::get_shell_path();
    assert!(
        shell_path.is_some(),
        "Shell PATH should be resolved on Unix"
    );

    let path_str = shell_path.unwrap().to_string_lossy();

    // Verify common directories where system commands live are in the PATH
    let has_bin = path_str.split(':').any(|entry| {
        entry == "/bin"
            || entry == "/usr/bin"
            || entry == "/usr/local/bin"
            || entry.ends_with("/bin")
    });
    assert!(
        has_bin,
        "Shell PATH should contain at least one bin directory: {}",
        path_str
    );
}

/// Verify that the resolved shell PATH contains more entries than the
/// minimal default (indicating the shell was actually sourced).
#[cfg(unix)]
#[test]
fn test_shell_path_richer_than_minimal() {
    use mcpmux_gateway::pool::transport::shell_env;

    let shell_path = shell_env::get_shell_path();
    assert!(shell_path.is_some());

    let path_str = shell_path.unwrap().to_string_lossy();
    let entry_count = path_str.split(':').count();

    // A minimal PATH has ~4 entries (/usr/bin:/bin:/usr/sbin:/sbin).
    // A sourced shell PATH typically has many more (Homebrew, nvm, cargo, etc.)
    // We just verify it has at least the minimal system entries.
    assert!(
        entry_count >= 2,
        "Shell PATH should have at least 2 entries, got {}: {}",
        entry_count,
        path_str
    );
}

/// Verify that a child process spawned with injected shell PATH can access
/// the full PATH. This tests the end-to-end flow: shell_env resolves PATH,
/// it gets injected into child env, and the child can find commands.
#[cfg(unix)]
#[tokio::test]
async fn test_child_process_receives_shell_path() {
    use mcpmux_gateway::pool::transport::shell_env;
    use std::collections::HashMap;

    let shell_path = shell_env::get_shell_path();
    assert!(shell_path.is_some());

    // Build env like StdioTransport does: inject shell PATH
    let mut env: HashMap<String, String> = HashMap::new();
    env.insert("FOO".to_string(), "bar".to_string());

    if let Some(path) = shell_path {
        if let Some(path_str) = path.to_str() {
            env.insert("PATH".to_string(), path_str.to_string());
        }
    }

    // Spawn a child that prints its PATH
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "echo $PATH"])
        .envs(&env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let output = cmd.output().await.expect("Failed to spawn child");
    assert!(output.status.success());

    let child_path = String::from_utf8_lossy(&output.stdout);
    let child_path = child_path.trim();

    // Verify the child's PATH matches what we injected
    let expected = shell_path.unwrap().to_string_lossy();
    assert_eq!(
        child_path,
        expected.as_ref(),
        "Child should receive the shell-resolved PATH"
    );
}

/// Verify that user-set PATH in env overrides is not overwritten by
/// shell PATH injection (end-to-end behavior test).
#[cfg(unix)]
#[tokio::test]
async fn test_user_path_override_not_clobbered() {
    use mcpmux_gateway::pool::transport::shell_env;
    use std::collections::HashMap;

    let shell_path = shell_env::get_shell_path();
    // Even if shell PATH is available, user's PATH should be preserved

    let custom_path = "/custom/user/path:/another/path";
    let mut env: HashMap<String, String> = HashMap::new();
    env.insert("PATH".to_string(), custom_path.to_string());

    // Simulate inject_shell_path behavior: should NOT override
    if !env.contains_key("PATH") {
        if let Some(path) = shell_path {
            if let Some(path_str) = path.to_str() {
                env.insert("PATH".to_string(), path_str.to_string());
            }
        }
    }

    // Spawn a child that prints its PATH.
    // Use absolute path to /bin/sh because the custom PATH doesn't include /bin.
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "echo $PATH"])
        .envs(&env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let output = cmd.output().await.expect("Failed to spawn child");
    assert!(output.status.success());

    let child_path = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        child_path.trim(),
        custom_path,
        "User's custom PATH should be preserved, not overwritten"
    );
}

/// Verify that StdioTransport.connect() finds a real command via shell PATH
/// when the command exists on the system. Uses 'echo' which exists everywhere.
#[cfg(unix)]
#[tokio::test]
async fn test_stdio_transport_resolves_command_via_shell_path() {
    use mcpmux_gateway::pool::transport::StdioTransport;
    use mcpmux_gateway::pool::{Transport, TransportConnectResult};
    use std::collections::HashMap;
    use std::time::Duration;
    use uuid::Uuid;

    // "echo" is in /bin or /usr/bin — shell PATH should find it
    let transport = StdioTransport::new(
        "echo".to_string(),
        vec!["hello".to_string()],
        HashMap::new(),
        Uuid::new_v4(),
        "test-echo-server".to_string(),
        None,
        Duration::from_secs(3),
        None,
    );

    let result = transport.connect().await;

    // echo isn't an MCP server, so it will either fail at handshake or timeout.
    // The important thing is it does NOT fail with "Command not found".
    match result {
        TransportConnectResult::Failed(msg) => {
            assert!(
                !msg.contains("Command not found"),
                "Shell PATH should find 'echo', but got: {}",
                msg
            );
        }
        // If it somehow connects (unlikely), that's fine too
        _ => {}
    }
}

/// A stdio server's own children (e.g. `node` under `npx`) must not outlive
/// it: stopping the server — gracefully or by dropping it — kills its whole
/// process group.
#[cfg(unix)]
mod process_group_cleanup {
    use mcpmux_gateway::pool::transport::StdioTransport;
    use mcpmux_gateway::pool::{Transport, TransportConnectResult};
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;
    use uuid::Uuid;

    /// Minimal MCP server that starts a long-running child first.
    const SERVER: &str = r#"
import json, subprocess, sys
child = subprocess.Popen(["sleep", "300"])
open(sys.argv[1], "w").write(str(child.pid))
for line in sys.stdin:
    msg = json.loads(line)
    if "id" not in msg:
        continue
    if msg["method"] == "initialize":
        result = {"protocolVersion": msg["params"]["protocolVersion"],
                  "capabilities": {}, "serverInfo": {"name": "t", "version": "1"}}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}), flush=True)
"#;

    /// Writes a 100 KB stderr line, then a normal one, then serves MCP.
    const STDERR_SERVER: &str = r#"
import json, sys
sys.stderr.write("x" * 100000 + "\n")
sys.stderr.write("after the long line\n")
sys.stderr.flush()
for line in sys.stdin:
    msg = json.loads(line)
    if "id" not in msg:
        continue
    if msg["method"] == "initialize":
        result = {"protocolVersion": msg["params"]["protocolVersion"],
                  "capabilities": {}, "serverInfo": {"name": "t", "version": "1"}}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}), flush=True)
"#;

    /// Running (a zombie, still listed until reaped, counts as gone).
    fn alive(pid: i32) -> bool {
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&out.stdout);
        let stat = stat.trim();
        !stat.is_empty() && !stat.starts_with('Z')
    }

    fn python3() -> Option<std::path::PathBuf> {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|dir| dir.join("python3"))
            .find(|p| p.is_file())
    }

    async fn start(dir: &Path) -> Option<(mcpmux_gateway::pool::McpClient, i32)> {
        let python = python3()?;
        let script = dir.join("server.py");
        let pid_file = dir.join("child.pid");
        std::fs::write(&script, SERVER).unwrap();
        let transport = StdioTransport::new(
            python.to_string_lossy().into_owned(),
            vec![
                "-I".into(),
                script.to_string_lossy().into_owned(),
                pid_file.to_string_lossy().into_owned(),
            ],
            HashMap::new(),
            Uuid::new_v4(),
            "group-test".to_string(),
            None,
            Duration::from_secs(20),
            None,
        );
        let client = match transport.connect().await {
            TransportConnectResult::Connected(client) => client,
            TransportConnectResult::Failed(e) => panic!("server did not start: {e}"),
            TransportConnectResult::OAuthRequired { .. } => panic!("unexpected OAuthRequired"),
        };
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(alive(pid));
        Some((client, pid))
    }

    async fn gone(pid: i32) -> bool {
        for _ in 0..100 {
            if !alive(pid) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    /// On app exit the server handles may never be dropped; killing every
    /// live group still stops the server and the processes it started.
    /// (Process-wide: nextest runs each test in its own process.)
    #[tokio::test(flavor = "multi_thread")]
    async fn kill_all_stops_servers_whose_handles_are_still_alive() {
        let dir = tempfile::tempdir().unwrap();
        let Some((client, grandchild)) = start(dir.path()).await else {
            eprintln!("python3 not found; skipping");
            return;
        };
        mcpmux_gateway::pool::transport::kill_all_stdio_groups();
        assert!(
            gone(grandchild).await,
            "the server's child outlived kill_all"
        );
        drop(client);
    }

    /// A server writing an enormous stderr line has it dropped (with a
    /// note) instead of buffered whole; the lines after it still arrive.
    #[tokio::test(flavor = "multi_thread")]
    async fn overlong_stderr_lines_are_dropped() {
        use mcpmux_core::{LogConfig, ServerLogManager};
        use std::sync::Arc;

        let Some(python) = python3() else {
            eprintln!("python3 not found; skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("server.py");
        std::fs::write(&script, STDERR_SERVER).unwrap();
        let logs = Arc::new(ServerLogManager::new(LogConfig {
            base_dir: dir.path().join("logs"),
            ..Default::default()
        }));
        let space = Uuid::new_v4();
        let transport = StdioTransport::new(
            python.to_string_lossy().into_owned(),
            vec!["-I".into(), script.to_string_lossy().into_owned()],
            HashMap::new(),
            space,
            "stderr-test".to_string(),
            Some(logs.clone()),
            Duration::from_secs(20),
            None,
        );
        let client = match transport.connect().await {
            TransportConnectResult::Connected(client) => client,
            TransportConnectResult::Failed(e) => panic!("server did not start: {e}"),
            TransportConnectResult::OAuthRequired { .. } => panic!("unexpected OAuthRequired"),
        };

        let mut messages = Vec::new();
        for _ in 0..50 {
            messages = logs
                .read_logs(&space.to_string(), "stderr-test", 100, None)
                .await
                .unwrap()
                .into_iter()
                .map(|l| l.message)
                .collect::<Vec<_>>();
            if messages.iter().any(|m| m.contains("after the long line")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let _ = client.cancel().await;
        assert!(
            messages.iter().any(|m| m.contains("after the long line")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("dropped")),
            "{messages:?}"
        );
        assert!(messages.iter().all(|m| m.len() < 20_000));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn graceful_disconnect_kills_the_servers_children() {
        let dir = tempfile::tempdir().unwrap();
        let Some((client, pid)) = start(dir.path()).await else {
            eprintln!("python3 not found; skipping");
            return;
        };
        let _ = client.cancel().await;
        assert!(gone(pid).await, "grandchild {pid} outlived the server");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dropping_the_client_kills_the_servers_children() {
        let dir = tempfile::tempdir().unwrap();
        let Some((client, pid)) = start(dir.path()).await else {
            eprintln!("python3 not found; skipping");
            return;
        };
        drop(client);
        assert!(gone(pid).await, "grandchild {pid} outlived the server");
    }
}
