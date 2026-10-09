//! MCP Server Implementation
//!
//! This module implements the Model Context Protocol server using rmcp's
//! ServerHandler trait and StreamableHttpService.
//!
//! Architecture:
//! - `handler`: Implements ServerHandler, delegates to existing services
//! - `context`: Utilities for extracting OAuth context from requests
//! - `discover_guard`: Keeps clients on the protocol versions with sessions
//! - `origin_guard`: Keeps web pages (browser `Origin`s) off the endpoint
//!
//! Note: MCPNotifier (notification bridge) is now in `consumers/` module.

pub mod context;
pub mod discover_guard;
pub mod handler;
pub mod oauth_middleware;
pub mod origin_guard;

pub use discover_guard::reject_server_discover;
pub use handler::McpMuxGatewayHandler;
pub use oauth_middleware::mcp_oauth_middleware;
