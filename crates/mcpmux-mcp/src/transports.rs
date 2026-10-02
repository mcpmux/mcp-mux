//! MCP Transport implementations
//!
//! - Stdio: Local process communication via child process
//! - HTTP: Remote server communication via Streamable HTTP
//!
//! Based on the working poc-rust implementation.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientInfo, Implementation,
        ListToolsResult, Tool,
    },
    service::RunningService,
    transport::TokioChildProcess,
    ClientHandler, RoleClient, ServiceExt,
};
use serde_json::Value;
#[cfg(not(windows))]
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[cfg(windows)]
use process_wrap::tokio::{CommandWrap, CreationFlags, KillOnDrop};
#[cfg(not(windows))]
use rmcp::transport::ConfigureCommandExt;
#[cfg(windows)]
use windows::Win32::System::Threading::PROCESS_CREATION_FLAGS;

/// Windows job-object containment for a spawned stdio server.
///
/// `process-wrap`'s built-in `JobObject` wrapper creates the child *suspended*
/// so it can attach before the child spawns descendants, then resumes it. On
/// Windows that resume allocates the child's console **with a visible window**,
/// which defeats `CREATE_NO_WINDOW` and makes the server pop open a terminal.
/// This wrapper attaches the kill-on-close job after the child is created and
/// running, keeping the console suppressed while still terminating the whole
/// process tree on drop.
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
        /// Attach the already-running child to the kill-on-close job.
        ///
        /// The child is briefly running outside any job between
        /// `CreateProcess` and this call; attaching before execution would
        /// require `CREATE_SUSPENDED`, which is what makes the terminal window
        /// reappear. See the equivalent wrapper in `mcpmux-gateway` for the
        /// full reasoning and measurements.
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
        // Wrap immediately so the `?` below still closes the handle on failure.
        let owned = JobOwner(JobHandle(CreateJobObjectW(None, None)?));

        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        SetInformationJobObject(
            owned.0 .0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )?;

        let handle = owned.0;
        std::mem::forget(owned);
        Ok(handle)
    }
}

/// Transport configuration for MCP server connections
#[derive(Debug, Clone)]
pub enum TransportConfig {
    /// STDIO connection (local process)
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    /// HTTP connection (Streamable HTTP)
    Http { url: String },
}

/// Server configuration for connecting
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub transport: TransportConfig,
}

/// Status of a connected MCP server
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    /// Server is not connected
    Disconnected,
    /// Server is connecting
    Connecting,
    /// Server is connected and ready
    Connected,
    /// Connection failed with error
    Error(String),
}

/// Information about a connected MCP server
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub server_id: String,
    pub server_name: String,
    pub status: ConnectionStatus,
    pub tools: Vec<Tool>,
    pub protocol_version: Option<String>,
}

/// Type alias for a connected MCP client
pub type McpClient = RunningService<RoleClient, McpClientHandler>;

/// Build a stdio child command whose descendants share the application's
/// lifecycle on Windows.
#[cfg(windows)]
fn stdio_child_command(
    executable: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> CommandWrap {
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut command = CommandWrap::with_new(executable, |cmd| {
        cmd.args(args).envs(env).stderr(Stdio::null());
    });
    // CREATE_NO_WINDOW must be the only console-related flag: adding
    // CREATE_SUSPENDED (as process-wrap's JobObject does) brings the terminal
    // window back. The kill-on-close job is attached after spawn instead.
    command.wrap(CreationFlags(PROCESS_CREATION_FLAGS(CREATE_NO_WINDOW)));
    command.wrap(KillOnDrop);
    command.wrap(job_tree::JobTree);
    command
}

/// Custom client handler for McpMux
#[derive(Clone)]
pub struct McpClientHandler {
    info: ClientInfo,
}

impl McpClientHandler {
    pub fn new(server_id: &str) -> Self {
        let mut client_info =
            Implementation::new(format!("mcpmux-{}", server_id), env!("CARGO_PKG_VERSION"));
        client_info.title = Some("McpMux Gateway".to_string());
        Self {
            info: ClientInfo::new(ClientCapabilities::default(), client_info),
        }
    }
}

impl ClientHandler for McpClientHandler {
    fn get_info(&self) -> ClientInfo {
        self.info.clone()
    }
}

/// A connected MCP client session
pub struct McpSession {
    pub server_id: String,
    pub status: ConnectionStatus,
    pub tools: Vec<Tool>,
    client: McpClient,
}

impl McpSession {
    /// Connect to a stdio-based MCP server
    pub async fn connect_stdio(
        server_id: String,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        // Parse the command string - it may contain embedded arguments (e.g., "docker run -i ...")
        // This is common in user configs copied from Cursor, Claude Desktop, etc.
        let (executable, parsed_args) = Self::parse_command(command, args)?;

        info!(
            server_id = %server_id,
            executable = %executable,
            args = ?parsed_args,
            "Connecting to stdio MCP server"
        );

        // Clone for the closure
        let args_for_closure = parsed_args.clone();
        let env = env.clone();

        #[cfg(windows)]
        let child_command = stdio_child_command(&executable, &args_for_closure, &env);

        #[cfg(not(windows))]
        let child_command = Command::new(&executable).configure(move |cmd| {
            cmd.args(&args_for_closure)
                .envs(&env)
                .stderr(Stdio::null())
                .kill_on_drop(true);
            cmd.process_group(0);
        });

        let transport = TokioChildProcess::new(child_command).context(format!(
            "Failed to spawn child process. Command not found: {}. Ensure it's installed and in PATH.",
            executable
        ))?;

        // Create the MCP client with handler
        let client_handler = McpClientHandler::new(&server_id);
        let client = client_handler
            .serve(transport)
            .await
            .context("Failed to initialize MCP client")?;

        // Get server info
        let peer_info = client.peer_info();
        debug!(
            server_id = %server_id,
            ?peer_info,
            "Connected to MCP server"
        );

        // List available tools
        let tools_result = client
            .peer()
            .list_tools(Default::default())
            .await
            .context("Failed to list tools")?;

        let tools = tools_result.tools;
        info!(
            server_id = %server_id,
            tool_count = tools.len(),
            "Retrieved tools from server"
        );

        Ok(Self {
            server_id,
            status: ConnectionStatus::Connected,
            tools,
            client,
        })
    }

    /// Call a tool on this server
    pub async fn call_tool(&self, name: &str, arguments: Option<Value>) -> Result<CallToolResult> {
        debug!(server_id = %self.server_id, tool = %name, "Calling tool");

        let args = arguments.and_then(|v| v.as_object().cloned());

        let result = self
            .client
            .peer()
            .call_tool({
                let mut params = CallToolRequestParams::new(name.to_string());
                params.arguments = args;
                params
            })
            .await
            .context("Tool call failed")?;

        Ok(result)
    }

    /// List tools available on this server
    pub async fn list_tools(&self) -> Result<ListToolsResult> {
        let result = self
            .client
            .peer()
            .list_tools(Default::default())
            .await
            .context("Failed to list tools")?;
        Ok(result)
    }

    /// Disconnect from the server
    pub async fn disconnect(self) -> Result<()> {
        info!(server_id = %self.server_id, "Disconnecting from MCP server");
        self.client
            .cancel()
            .await
            .context("Failed to cancel service")?;
        Ok(())
    }

    /// Get the underlying client for direct access
    pub fn client(&self) -> &McpClient {
        &self.client
    }

    /// Parse a command string that may contain embedded arguments.
    ///
    /// This handles common formats from user configs (Cursor, Claude Desktop, etc.):
    /// - "docker run -i --rm image" → ("docker", ["run", "-i", "--rm", "image"])
    /// - "npx -y @some/server" → ("npx", ["-y", "@some/server"])
    /// - "node" with args: ["server.js"] → ("node", ["server.js"])
    ///
    /// If the command contains spaces and no separate args are provided,
    /// it will be parsed using shell-words to properly handle quoting.
    fn parse_command(command: &str, args: &[String]) -> Result<(String, Vec<String>)> {
        // If separate args are provided, use command as-is (it should be just the executable)
        if !args.is_empty() {
            return Ok((command.to_string(), args.to_vec()));
        }

        // Check if command contains spaces (embedded arguments)
        if command.contains(' ') {
            // Parse the command string using shell-words for proper quoting
            let parts = shell_words::split(command)
                .context("Failed to parse command string - check for unmatched quotes")?;

            if parts.is_empty() {
                return Err(anyhow::anyhow!("Empty command after parsing"));
            }

            let executable = parts[0].clone();
            let parsed_args = parts[1..].to_vec();

            Ok((executable, parsed_args))
        } else {
            // Simple command with no embedded args
            Ok((command.to_string(), Vec::new()))
        }
    }
}

/// Manager for MCP server connections
pub struct ServerManager {
    /// Active sessions by server_id
    sessions: Arc<RwLock<HashMap<String, Arc<RwLock<McpSession>>>>>,
}

impl ServerManager {
    /// Create a new server manager
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Connect to an MCP server based on its configuration
    pub async fn connect(
        &self,
        server: &ServerConfig,
        env_vars: HashMap<String, String>,
    ) -> Result<()> {
        let server_id = server.id.clone();

        // Check if already connected
        {
            let sessions = self.sessions.read().await;
            if sessions.contains_key(&server_id) {
                debug!(server_id = %server_id, "Server already connected");
                return Ok(());
            }
        }

        // Connect based on transport type
        let session = match &server.transport {
            TransportConfig::Stdio { command, args, env } => {
                // Merge server env with provided env vars
                let mut merged_env = env.clone();
                merged_env.extend(env_vars);

                McpSession::connect_stdio(server_id.clone(), command, args, &merged_env).await?
            }
            TransportConfig::Http { url } => {
                // HTTP transport will be implemented in Phase 2
                warn!(server_id = %server_id, url = %url, "HTTP transport not yet implemented");
                return Err(anyhow::anyhow!("HTTP transport not yet implemented"));
            }
        };

        // Store the session
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(server_id, Arc::new(RwLock::new(session)));
        }

        Ok(())
    }

    /// Disconnect from an MCP server
    pub async fn disconnect(&self, server_id: &str) -> Result<()> {
        let session = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(server_id)
        };

        if let Some(session_arc) = session {
            // Get exclusive access to disconnect
            let session = Arc::try_unwrap(session_arc)
                .map_err(|_| anyhow::anyhow!("Session still in use"))?
                .into_inner();
            session.disconnect().await?;
        }

        Ok(())
    }

    /// Get connection status for a server
    pub async fn status(&self, server_id: &str) -> ConnectionStatus {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(server_id) {
            session.read().await.status.clone()
        } else {
            ConnectionStatus::Disconnected
        }
    }

    /// Get tools for a connected server
    pub async fn get_tools(&self, server_id: &str) -> Option<Vec<Tool>> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(server_id) {
            Some(session.read().await.tools.clone())
        } else {
            None
        }
    }

    /// Call a tool on a connected server
    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> Result<CallToolResult> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(server_id)
            .ok_or_else(|| anyhow::anyhow!("Server not connected: {}", server_id))?;

        let session = session.read().await;
        session.call_tool(tool_name, arguments).await
    }

    /// Get all connected server IDs
    pub async fn connected_servers(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }

    /// Disconnect all servers
    pub async fn disconnect_all(&self) -> Result<()> {
        let server_ids: Vec<String> = {
            let sessions = self.sessions.read().await;
            sessions.keys().cloned().collect()
        };

        for server_id in server_ids {
            if let Err(e) = self.disconnect(&server_id).await {
                error!(server_id = %server_id, error = %e, "Failed to disconnect server");
            }
        }

        Ok(())
    }
}

impl Default for ServerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn stdio_child_command_contains_windows_tree_containment() {
        let command = stdio_child_command("cmd.exe", &[], &HashMap::new());

        assert!(command.has_wrap::<CreationFlags>());
        assert!(command.has_wrap::<KillOnDrop>());
        assert!(command.has_wrap::<job_tree::JobTree>());
    }

    #[test]
    fn test_connection_status() {
        let status = ConnectionStatus::Connected;
        assert_eq!(status, ConnectionStatus::Connected);
    }

    #[test]
    fn test_server_manager_new() {
        let manager = ServerManager::new();
        assert!(manager.sessions.try_read().is_ok());
    }

    #[test]
    fn test_client_handler() {
        let handler = McpClientHandler::new("test-server");
        let info = handler.get_info();
        assert!(info.client_info.name.contains("mcpmux"));
    }
}
