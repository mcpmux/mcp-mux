//! STDIO transport for MCP servers
//!
//! Handles connecting to MCP servers that run as child processes
//! communicating over stdin/stdout.
//!
//! Process stderr is captured via an OS pipe and streamed to the server
//! log manager, making terminal output visible in the desktop log viewer.
//! These logs are internal to the desktop app and are never exposed
//! externally via the HTTP gateway.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mcpmux_core::{LogLevel, LogSource, ServerLog, ServerLogManager};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::ServiceExt;
use tokio::process::Command;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::TransportType;
use super::{create_client_handler, Transport, TransportConnectResult};

/// Apply platform-specific flags to a child process command.
///
/// - **Windows**: Sets `CREATE_NO_WINDOW` (`0x08000000`) so the child process does not
///   allocate a visible console window. Required because release builds use
///   `windows_subsystem = "windows"` (GUI subsystem) and Windows would otherwise create
///   a new console for every spawned console-subsystem child.
///
/// - **Unix (macOS / Linux)**: Calls `process_group(0)` to place the child in its own
///   process group, preventing terminal signals (`SIGINT`, `SIGTSTP`) sent to the parent
///   from propagating to MCP server child processes.
pub fn configure_child_process_platform(cmd: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
}

/// Returns a helpful hint for common runtime-dependent commands when they fail.
fn command_hint(command: &str) -> &'static str {
    let cmd = command.rsplit(['/', '\\']).next().unwrap_or(command);
    if cmd == "docker" || cmd == "docker.exe" || cmd.starts_with("docker-") {
        " Ensure Docker Desktop is installed and running."
    } else {
        ""
    }
}

/// Create an OS pipe for stderr capture.
///
/// Returns `(reader, write_stdio)` where:
/// - `reader` is a blocking `PipeReader` for the read end
/// - `write_stdio` is a `Stdio` for the child process's stderr
fn create_stderr_pipe() -> std::io::Result<(os_pipe::PipeReader, Stdio)> {
    let (reader, writer) = os_pipe::pipe()?;
    Ok((reader, writer.into()))
}

/// Spawn a background task that reads lines from the process stderr pipe
/// and logs them to the server log manager.
///
/// The task runs on the blocking thread pool until the pipe is closed
/// (child process exits) or an I/O error occurs.
fn spawn_stderr_reader(
    stderr_file: os_pipe::PipeReader,
    log_manager: Option<Arc<ServerLogManager>>,
    space_id: Uuid,
    server_id: String,
) {
    let Some(log_manager) = log_manager else {
        return;
    };

    let space_id_str = space_id.to_string();

    tokio::task::spawn_blocking(move || {
        use std::io::BufRead;

        let rt = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return,
        };

        let reader = std::io::BufReader::new(stderr_file);

        for line_result in reader.lines() {
            match line_result {
                Ok(line) if line.is_empty() => continue,
                Ok(line) => {
                    let level = classify_stderr_line(&line);
                    let log = ServerLog::new(level, LogSource::Stderr, &line);
                    let _ = rt.block_on(log_manager.append(&space_id_str, &server_id, log));
                }
                Err(e) => {
                    debug!(
                        server_id = %server_id,
                        error = %e,
                        "Stderr reader stopped"
                    );
                    break;
                }
            }
        }

        debug!(server_id = %server_id, "Stderr reader finished (pipe closed)");
    });
}

/// Classify a stderr line into a log level based on content heuristics.
fn classify_stderr_line(line: &str) -> LogLevel {
    let lower = line.to_lowercase();
    if lower.contains("error") || lower.contains("panic") || lower.contains("fatal") {
        LogLevel::Error
    } else if lower.contains("warn") {
        LogLevel::Warn
    } else if lower.contains("debug") || lower.contains("trace") {
        LogLevel::Debug
    } else {
        LogLevel::Info
    }
}

/// STDIO transport for child process MCP servers
pub struct StdioTransport {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    space_id: Uuid,
    server_id: String,
    log_manager: Option<Arc<ServerLogManager>>,
    connect_timeout: Duration,
    event_tx: Option<tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>>,
}

impl StdioTransport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
        space_id: Uuid,
        server_id: String,
        log_manager: Option<Arc<ServerLogManager>>,
        connect_timeout: Duration,
        event_tx: Option<tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>>,
    ) -> Self {
        Self {
            command,
            args,
            env,
            space_id,
            server_id,
            log_manager,
            connect_timeout,
            event_tx,
        }
    }

    /// Log a message to the server log manager.
    async fn log(&self, level: LogLevel, source: LogSource, message: String) {
        if let Some(log_manager) = &self.log_manager {
            let log = ServerLog::new(level, source, message);
            if let Err(e) = log_manager
                .append(&self.space_id.to_string(), &self.server_id, log)
                .await
            {
                error!("Failed to write log: {}", e);
            }
        }
    }

    /// Internal helper: attempt connection with a given stderr Stdio target.
    async fn connect_with_stderr(
        &self,
        command_path: &std::path::Path,
        stderr_config: Stdio,
    ) -> TransportConnectResult {
        let args = self.args.clone();
        let env = self.env.clone();

        let transport =
            match TokioChildProcess::new(Command::new(command_path).configure(move |cmd| {
                cmd.args(&args)
                    .envs(&env)
                    .stderr(stderr_config)
                    .kill_on_drop(true);

                configure_child_process_platform(cmd);
            })) {
                Ok(t) => t,
                Err(e) => {
                    let hint = command_hint(&self.command);
                    let err = format!("Failed to spawn process: {e}.{hint}");
                    error!(server_id = %self.server_id, "{}", err);
                    self.log(LogLevel::Error, LogSource::Connection, err.clone())
                        .await;
                    return TransportConnectResult::Failed(err);
                }
            };

        // Create client handler
        let client_handler =
            create_client_handler(&self.server_id, self.space_id, self.event_tx.clone());

        // Connect with timeout
        let connect_future = client_handler.serve(transport);
        let client = match tokio::time::timeout(self.connect_timeout, connect_future).await {
            Ok(Ok(client)) => client,
            Ok(Err(e)) => {
                let hint = command_hint(&self.command);
                let err = format!("MCP handshake failed: {e}.{hint}");
                error!(server_id = %self.server_id, "{}", err);
                self.log(LogLevel::Error, LogSource::Connection, err.clone())
                    .await;
                return TransportConnectResult::Failed(err);
            }
            Err(_) => {
                let hint = command_hint(&self.command);
                let err = format!("Connection timeout ({:?}).{hint}", self.connect_timeout);
                error!(server_id = %self.server_id, "{}", err);
                self.log(LogLevel::Error, LogSource::Connection, err.clone())
                    .await;
                return TransportConnectResult::Failed(err);
            }
        };

        info!(
            server_id = %self.server_id,
            "STDIO server connected"
        );

        self.log(
            LogLevel::Info,
            LogSource::Connection,
            "Server connected successfully".to_string(),
        )
        .await;

        TransportConnectResult::Connected(client)
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn connect(&self) -> TransportConnectResult {
        info!(
            server_id = %self.server_id,
            command = %self.command,
            "Connecting to STDIO server"
        );

        // Log connection attempt
        self.log(
            LogLevel::Info,
            LogSource::Connection,
            format!("Connecting to server: {} {:?}", self.command, self.args),
        )
        .await;

        // Validate command exists
        let command_path = match which::which(&self.command)
            .or_else(|_| which::which(format!("{}.exe", &self.command)))
        {
            Ok(path) => path,
            Err(_) => {
                let hint = command_hint(&self.command);
                let err = format!(
                    "Command not found: {}. Ensure it's installed and in PATH.{hint}",
                    self.command
                );
                error!(server_id = %self.server_id, "{}", err);
                self.log(LogLevel::Error, LogSource::Connection, err.clone())
                    .await;
                return TransportConnectResult::Failed(err);
            }
        };

        debug!(
            server_id = %self.server_id,
            path = ?command_path,
            "Found command"
        );

        // Create an OS pipe for stderr capture.
        // The write end goes to the child process, the read end stays with us
        // for streaming process output into the log viewer.
        let (stderr_read, stderr_write) = match create_stderr_pipe() {
            Ok(pair) => pair,
            Err(e) => {
                warn!(
                    server_id = %self.server_id,
                    error = %e,
                    "Failed to create stderr pipe, falling back to null"
                );
                // Connection still works, just without process log capture
                return self.connect_with_stderr(&command_path, Stdio::null()).await;
            }
        };

        // Spawn the background stderr reader before connecting.
        // It blocks on the read end until the child writes to stderr.
        spawn_stderr_reader(
            stderr_read,
            self.log_manager.clone(),
            self.space_id,
            self.server_id.clone(),
        );

        self.connect_with_stderr(&command_path, stderr_write).await
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Stdio
    }

    fn description(&self) -> String {
        format!("stdio:{}", self.command)
    }
}
