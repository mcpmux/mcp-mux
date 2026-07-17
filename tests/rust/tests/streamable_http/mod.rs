//! Streamable HTTP Transport Integration Tests
//!
//! Tests the full stateful Streamable HTTP transport with:
//! - Session management (Mcp-Session-Id)
//! - Server-initiated notifications (list_changed via SSE)
//! - Proper protocol negotiation

mod api_key_auth;
mod auth_disable;
mod auth_oauth_e2e;
mod gateway_notifications;
mod network_advertising;
mod notifications;
