//! MCP Server Implementation
//!
//! This module implements the Model Context Protocol server using rmcp's
//! ServerHandler trait and StreamableHttpService.
//!
//! Architecture:
//! - `handler`: Implements ServerHandler, delegates to existing services
//! - `context`: Utilities for extracting OAuth context from requests
//!
//! Note: MCPNotifier (notification bridge) is now in `consumers/` module.

pub mod handler;
pub mod context;
pub mod oauth_middleware;

pub use handler::McpMuxGatewayHandler;
pub use oauth_middleware::mcp_oauth_middleware;
