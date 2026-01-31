//! # McpMux MCP Library
//!
//! MCP protocol implementation, client pool, and gateway functionality.
//!
//! This crate provides:
//! - MCP client management (stdio and HTTP transports)
//! - Connection pooling by config hash
//! - Request routing and aggregation
//! - OAuth token management for remote servers
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        ClientPool                               │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │  HashMap<pool_key, PooledClient>                         │   │
//! │  │                                                          │   │
//! │  │  pool_key = server_id + ":" + sha256(config+credential)  │   │
//! │  │                                                          │   │
//! │  │  "github:aabbccdd" → McpSession (work token)            │   │
//! │  │  "github:11223344" → McpSession (personal token)        │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      ServerManager                              │
//! │  ┌──────────────────┐  ┌──────────────────┐                    │
//! │  │  StdioTransport  │  │   HttpTransport  │                    │
//! │  │  (child process) │  │   (HTTP client)  │                    │
//! │  └──────────────────┘  └──────────────────┘                    │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use mcpmux_mcp::{ServerManager, ClientPool, PoolConfig};
//! use mcpmux_core::Server;
//!
//! // Create server manager
//! let manager = ServerManager::new();
//!
//! // Connect to a server
//! let server = Server::stdio("github", "GitHub", "npx", vec![
//!     "-y".to_string(),
//!     "@modelcontextprotocol/server-github".to_string(),
//! ]);
//! 
//! let mut env = HashMap::new();
//! env.insert("GITHUB_TOKEN".to_string(), "ghp_xxx".to_string());
//!
//! manager.connect(&server, env).await?;
//!
//! // List tools
//! let tools = manager.get_tools("github").await;
//!
//! // Call a tool
//! let result = manager.call_tool("github", "search_code", Some(json!({
//!     "query": "rust async"
//! }))).await?;
//! ```

pub mod client_pool;
pub mod transports;

pub use client_pool::{ClientPool, PoolConfig, PooledClient};
pub use transports::{ConnectionStatus, McpClient, McpClientHandler, McpSession, ServerInfo, ServerManager};
