//! STDIO transport for MCP servers
//!
//! Handles connecting to MCP servers that run as child processes
//! communicating over stdin/stdout.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mcpmux_core::{LogLevel, LogSource, ServerLog, ServerLogManager};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::ServiceExt;
use tokio::process::Command;
use tracing::{debug, error, info};
use uuid::Uuid;

use super::TransportType;
use super::{create_client_handler, Transport, TransportConnectResult};

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

    /// Log a message
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
                let err = format!(
                    "Command not found: {}. Ensure it's installed and in PATH.",
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

        // Clone for closure and stderr capture
        let args = self.args.clone();
        let env = self.env.clone();
        let _log_manager = self.log_manager.clone();
        let _space_id = self.space_id;
        let _server_id = self.server_id.clone();

        // Create transport using child process with stderr capture
        // Use resolved command_path instead of self.command to ensure we use the full path
        let transport =
            match TokioChildProcess::new(Command::new(&command_path).configure(move |cmd| {
                cmd.args(&args)
                    .envs(&env)
                    .stderr(Stdio::piped()) // Capture stderr for logging
                    .kill_on_drop(true);

                // Note: We can't easily access stderr after TokioChildProcess wraps it
                // This is a limitation of the current rmcp API
                // For now, we log connection events only
                // TODO: Consider forking rmcp or using a custom transport wrapper
            })) {
                Ok(t) => t,
                Err(e) => {
                    let err = format!("Failed to spawn process: {}", e);
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
                let err = format!("MCP handshake failed: {}", e);
                error!(server_id = %self.server_id, "{}", err);
                self.log(LogLevel::Error, LogSource::Connection, err.clone())
                    .await;
                return TransportConnectResult::Failed(err);
            }
            Err(_) => {
                let err = format!("Connection timeout ({:?})", self.connect_timeout);
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
