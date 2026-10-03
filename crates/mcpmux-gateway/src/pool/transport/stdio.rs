//! STDIO transport for MCP servers
//!
//! Handles connecting to MCP servers that run as child processes
//! communicating over stdin/stdout.
//!
//! Process stderr is captured via tokio's piped stderr and streamed to the
//! server log manager, making terminal output visible in the desktop log
//! viewer. This works generically for any runtime (npx, node, docker, python,
//! etc.). These logs are internal to the desktop app and are never exposed
//! externally via the HTTP gateway.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mcpmux_core::{LogLevel, LogSource, ServerLog, ServerLogManager};
#[cfg(not(windows))]
use rmcp::transport::ConfigureCommandExt;
use rmcp::transport::TokioChildProcess;
use rmcp::ServiceExt;
use tokio::io::AsyncBufReadExt;
use tokio::process::{ChildStderr, Command};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[cfg(windows)]
use process_wrap::tokio::{CommandWrap, CreationFlags, KillOnDrop};
#[cfg(windows)]
use windows::Win32::System::Threading::PROCESS_CREATION_FLAGS;

use super::shell_env;
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

/// Windows job-object containment for a spawned stdio server.
///
/// `process-wrap`'s built-in [`JobObject`] wrapper must create the child
/// *suspended* so it can attach before the child spawns descendants, and then
/// resume it. On Windows that resume allocates the child's console **with a
/// visible window**, which defeats `CREATE_NO_WINDOW` — every stdio MCP server
/// ends up popping open a terminal window. Measured on Windows 11 with
/// Windows Terminal installed as the default console host: with `JobObject`
/// every child produced one visible terminal window, without it none did.
///
/// This wrapper attaches the job *after* the child is created and running, so
/// the console stays suppressed while the whole process tree is still
/// terminated when the child is dropped.
#[cfg(windows)]
mod job_tree {
    use std::io;

    use process_wrap::tokio::{ChildWrapper, CommandWrap, CommandWrapper};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// Attaches the spawned child to a kill-on-close Job Object.
    #[derive(Debug)]
    pub struct JobTree;

    /// Raw job handle. `HANDLE` is not `Send`, but moving one between threads
    /// is sound; process-wrap marks its equivalent newtype the same way.
    #[derive(Debug, Clone, Copy)]
    struct JobHandle(HANDLE);

    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}

    /// Owns the job handle: closing it terminates every process in the job.
    #[derive(Debug)]
    struct JobOwner(JobHandle);

    impl Drop for JobOwner {
        fn drop(&mut self) {
            // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE tears down the whole tree here.
            unsafe {
                let _ = CloseHandle(self.0 .0);
            }
        }
    }

    #[derive(Debug)]
    struct JobTreeChild {
        inner: Box<dyn ChildWrapper>,
        job: JobOwner,
    }

    impl ChildWrapper for JobTreeChild {
        fn inner(&self) -> &dyn ChildWrapper {
            self.inner.inner()
        }

        fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
            self.inner.inner_mut()
        }

        fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
            // Leave the job handle open: closing it would terminate the whole
            // tree and leave the returned Child useless. This mirrors
            // process-wrap's own JobObjectChild::into_inner.
            let this = *self;
            std::mem::forget(this.job);
            this.inner.into_inner()
        }
    }

    impl CommandWrapper for JobTree {
        /// Attach the already-running child to the job.
        ///
        /// Note the trade-off: between `CreateProcess` and this call the child
        /// is briefly running outside any job, so a fast wrapper (`cmd`, `npx`)
        /// could in principle create a descendant that never joins the job and
        /// would survive when the handle closes. Attaching before execution
        /// requires `CREATE_SUSPENDED`, which is exactly what brings the
        /// terminal window back, so this window is accepted deliberately. In
        /// practice the window is sub-millisecond and the wrappers that would
        /// exploit it need tens of milliseconds to start their own child.
        fn wrap_child(
            &mut self,
            inner: Box<dyn ChildWrapper>,
            _core: &CommandWrap,
        ) -> io::Result<Box<dyn ChildWrapper>> {
            let job = unsafe { create_kill_on_close_job()? };

            let handle = inner
                .inner_child()
                .raw_handle()
                .ok_or_else(|| io::Error::other("spawned child has no process handle"))?;

            let process = HANDLE(handle);
            if let Err(e) = unsafe { AssignProcessToJobObject(job.0, process) } {
                drop(JobOwner(job));
                return Err(io::Error::other(format!(
                    "AssignProcessToJobObject failed: {e}"
                )));
            }

            Ok(Box::new(JobTreeChild {
                inner,
                job: JobOwner(job),
            }))
        }
    }

    unsafe fn create_kill_on_close_job() -> io::Result<JobHandle> {
        // Wrap the handle immediately so the `?` below still closes it if
        // configuring the job fails; otherwise every retry leaks one handle.
        let owned = JobOwner(JobHandle(CreateJobObjectW(None, None)?));

        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        SetInformationJobObject(
            owned.0 .0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )?;

        // Success: hand the raw handle to the caller, which wraps it in its
        // own `JobOwner` once the child has been attached.
        let handle = owned.0;
        std::mem::forget(owned);
        Ok(handle)
    }
}

/// Build the stdio child command with the platform's required containment.
///
/// Windows job objects make the launched command and every descendant one
/// lifecycle unit. Dropping the RMCP transport therefore terminates wrapper
/// chains such as `npx -> cmd -> node`, rather than only the direct child.
#[cfg(windows)]
fn stdio_child_command(
    command_path: &std::path::Path,
    args: &[String],
    env: &HashMap<String, String>,
) -> CommandWrap {
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut command = CommandWrap::with_new(command_path, |cmd| {
        cmd.args(args).envs(env);
    });
    // CREATE_NO_WINDOW must be the *only* console-related flag: it is the sole
    // thing suppressing the terminal window, and adding CREATE_SUSPENDED (as
    // process-wrap's JobObject does) brings the window back.
    command.wrap(CreationFlags(PROCESS_CREATION_FLAGS(CREATE_NO_WINDOW)));
    command.wrap(KillOnDrop);
    command.wrap(job_tree::JobTree);
    command
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

/// Spawn an async task that reads lines from the child process stderr
/// and logs them to the server log manager.
///
/// The task runs until the stderr stream is closed (child process exits)
/// or an I/O error occurs.
fn spawn_stderr_reader(
    stderr: ChildStderr,
    log_manager: Option<Arc<ServerLogManager>>,
    space_id: Uuid,
    server_id: String,
) {
    let Some(log_manager) = log_manager else {
        return;
    };

    let space_id_str = space_id.to_string();

    tokio::spawn(async move {
        let reader = tokio::io::BufReader::new(stderr);
        let mut lines = reader.lines();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) if line.is_empty() => continue,
                Ok(Some(line)) => {
                    let level = classify_stderr_line(&line);
                    let log = ServerLog::new(level, LogSource::Stderr, &line);
                    let _ = log_manager.append(&space_id_str, &server_id, log).await;
                }
                Ok(None) => {
                    // EOF - child process closed stderr
                    debug!(server_id = %server_id, "Stderr reader finished (stream closed)");
                    break;
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
}

#[async_trait]
impl Transport for StdioTransport {
    async fn connect(&self) -> TransportConnectResult {
        info!(
            server_id = %self.server_id,
            command = %self.command,
            "Connecting to STDIO server"
        );

        // Log connection attempt. Do NOT log resolved args — `${input:...}`
        // placeholders are already substituted by this point, so secret
        // inputs passed as CLI args (a common MCP server pattern, e.g.
        // `--api-key sk-...`) would land in plaintext `current.log` and
        // defeat the encrypted-credentials guarantee. Log the command and an
        // arg count only.
        self.log(
            LogLevel::Info,
            LogSource::Connection,
            format!(
                "Connecting to server: {} ({} arg(s))",
                self.command,
                self.args.len()
            ),
        )
        .await;

        // Resolve the user's full shell PATH (cached after first call).
        // On macOS/Linux, GUI apps have a minimal PATH that doesn't include
        // Homebrew, nvm, Volta, fnm, or /usr/local/bin — this fixes that.
        let shell_path = shell_env::get_shell_path();

        // Validate command exists, using the shell-resolved PATH when available
        let command_path = match resolve_command(&self.command, shell_path) {
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

        // Build the child process environment:
        // - Start with user-configured env vars (from resolution.rs)
        // - Inject the shell-resolved PATH so child processes can find
        //   their own dependencies (e.g., npx needs to find node)
        let args = self.args.clone();
        let mut env = self.env.clone();
        inject_shell_path(&mut env, shell_path);

        #[cfg(windows)]
        let child = TokioChildProcess::builder(stdio_child_command(&command_path, &args, &env))
            .stderr(Stdio::piped())
            .spawn();

        #[cfg(not(windows))]
        let child = TokioChildProcess::builder(Command::new(&command_path).configure(move |cmd| {
            cmd.args(&args).envs(&env).kill_on_drop(true);
            configure_child_process_platform(cmd);
        }))
        .stderr(Stdio::piped())
        .spawn();

        let (transport, child_stderr) = match child {
            Ok(result) => result,
            Err(e) => {
                let hint = command_hint(&self.command);
                let err = format!("Failed to spawn process: {e}.{hint}");
                error!(server_id = %self.server_id, "{}", err);
                self.log(LogLevel::Error, LogSource::Connection, err.clone())
                    .await;
                return TransportConnectResult::Failed(err);
            }
        };

        // Start the async stderr reader if we got a handle
        if let Some(stderr) = child_stderr {
            spawn_stderr_reader(
                stderr,
                self.log_manager.clone(),
                self.space_id,
                self.server_id.clone(),
            );
        } else {
            warn!(
                server_id = %self.server_id,
                "No stderr handle available - process logs will not be captured"
            );
        }

        // Create client handler
        let client_handler = create_client_handler(
            &self.server_id,
            self.space_id,
            self.event_tx.clone(),
            self.log_manager.clone(),
        );

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

    fn transport_type(&self) -> TransportType {
        TransportType::Stdio
    }

    fn description(&self) -> String {
        format!("stdio:{}", self.command)
    }
}

/// Resolve a command binary using the shell-resolved PATH when available.
///
/// Falls back to the standard `which::which()` (which uses the process PATH)
/// if no shell PATH was resolved.
fn resolve_command(
    command: &str,
    shell_path: Option<&std::ffi::OsString>,
) -> Result<std::path::PathBuf, which::Error> {
    if let Some(path) = shell_path {
        which::which_in(command, Some(path), ".")
            .or_else(|_| which::which_in(format!("{}.exe", command), Some(path), "."))
    } else {
        which::which(command).or_else(|_| which::which(format!("{}.exe", command)))
    }
}

/// Inject the shell-resolved PATH into the child process environment.
///
/// This ensures child processes (e.g., npx spawning node) can find their
/// own dependencies even when the parent GUI app has a minimal PATH.
///
/// Only injects if the user hasn't explicitly set PATH in their env overrides.
fn inject_shell_path(env: &mut HashMap<String, String>, shell_path: Option<&std::ffi::OsString>) {
    if env.contains_key("PATH") {
        return; // User explicitly set PATH — respect it
    }

    if let Some(path) = shell_path {
        if let Some(path_str) = path.to_str() {
            env.insert("PATH".to_string(), path_str.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[cfg(windows)]
    #[test]
    fn stdio_child_command_contains_windows_tree_containment() {
        let command = stdio_child_command(std::path::Path::new("cmd.exe"), &[], &HashMap::new());

        assert!(command.has_wrap::<CreationFlags>());
        assert!(command.has_wrap::<KillOnDrop>());
        assert!(command.has_wrap::<job_tree::JobTree>());
    }

    // ── resolve_command tests ──────────────────────────────────────

    #[test]
    fn test_resolve_command_finds_sh_with_shell_path() {
        // /bin/sh exists on every Unix system
        #[cfg(unix)]
        {
            let path = OsString::from("/bin:/usr/bin");
            let result = resolve_command("sh", Some(&path));
            assert!(result.is_ok(), "Should find 'sh' in /bin:/usr/bin");
        }
    }

    #[test]
    fn test_resolve_command_finds_command_without_shell_path() {
        // Without shell_path, falls back to which::which (uses process PATH)
        #[cfg(unix)]
        {
            let result = resolve_command("sh", None);
            assert!(result.is_ok(), "Should find 'sh' via process PATH");
        }
    }

    #[test]
    fn test_resolve_command_returns_error_for_nonexistent() {
        let fake_path = OsString::from("/nonexistent/path");
        let result = resolve_command("this_command_surely_does_not_exist_xyz", Some(&fake_path));
        assert!(result.is_err(), "Should fail for nonexistent command");
    }

    #[test]
    fn test_resolve_command_not_found_in_restricted_path() {
        // Even if 'sh' exists, it shouldn't be found if PATH points elsewhere
        let path = OsString::from("/tmp/empty_dir_that_does_not_exist");
        let result = resolve_command("sh", Some(&path));
        assert!(
            result.is_err(),
            "Should not find 'sh' in a path that doesn't contain it"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_command_with_full_shell_path() {
        // Use the actual shell-resolved PATH to find a real command
        if let Some(shell_path) = shell_env::get_shell_path() {
            let result = resolve_command("sh", Some(shell_path));
            assert!(result.is_ok(), "Should find 'sh' using resolved shell PATH");
        }
    }

    // ── inject_shell_path tests ────────────────────────────────────

    #[test]
    fn test_inject_shell_path_adds_when_missing() {
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());

        let path = OsString::from("/usr/bin:/usr/local/bin");
        inject_shell_path(&mut env, Some(&path));

        assert_eq!(
            env.get("PATH"),
            Some(&"/usr/bin:/usr/local/bin".to_string()),
            "PATH should be injected"
        );
        assert_eq!(
            env.get("FOO"),
            Some(&"bar".to_string()),
            "Existing vars should be preserved"
        );
    }

    #[test]
    fn test_inject_shell_path_respects_existing_path() {
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/custom/path".to_string());

        let path = OsString::from("/usr/bin:/usr/local/bin");
        inject_shell_path(&mut env, Some(&path));

        assert_eq!(
            env.get("PATH"),
            Some(&"/custom/path".to_string()),
            "User-set PATH should not be overridden"
        );
    }

    #[test]
    fn test_inject_shell_path_noop_when_none() {
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());

        inject_shell_path(&mut env, None);

        assert!(
            !env.contains_key("PATH"),
            "Should not inject PATH when shell_path is None"
        );
    }

    #[test]
    fn test_inject_shell_path_empty_env() {
        let mut env = HashMap::new();

        let path = OsString::from("/a:/b:/c");
        inject_shell_path(&mut env, Some(&path));

        assert_eq!(env.get("PATH"), Some(&"/a:/b:/c".to_string()));
        assert_eq!(env.len(), 1, "Should only have PATH");
    }

    // ── command_hint tests ─────────────────────────────────────────

    #[test]
    fn test_command_hint_docker() {
        assert!(command_hint("docker").contains("Docker Desktop"));
        assert!(command_hint("/usr/local/bin/docker").contains("Docker Desktop"));
    }

    #[test]
    fn test_command_hint_non_docker() {
        assert_eq!(command_hint("npx"), "");
        assert_eq!(command_hint("node"), "");
        assert_eq!(command_hint("python"), "");
    }

    // ── classify_stderr_line tests ─────────────────────────────────

    #[test]
    fn test_classify_stderr_error() {
        assert_eq!(
            classify_stderr_line("ERROR: something failed"),
            LogLevel::Error
        );
        assert_eq!(
            classify_stderr_line("fatal: not a git repository"),
            LogLevel::Error
        );
        assert_eq!(
            classify_stderr_line("thread 'main' panicked"),
            LogLevel::Error
        );
    }

    #[test]
    fn test_classify_stderr_warn() {
        assert_eq!(
            classify_stderr_line("WARN: deprecated feature"),
            LogLevel::Warn
        );
        assert_eq!(
            classify_stderr_line("Warning: something is off"),
            LogLevel::Warn
        );
    }

    #[test]
    fn test_classify_stderr_debug() {
        assert_eq!(
            classify_stderr_line("DEBUG: internal state"),
            LogLevel::Debug
        );
        assert_eq!(
            classify_stderr_line("trace: verbose output"),
            LogLevel::Debug
        );
    }

    #[test]
    fn test_classify_stderr_info_default() {
        assert_eq!(
            classify_stderr_line("Server listening on port 3000"),
            LogLevel::Info
        );
    }
}
