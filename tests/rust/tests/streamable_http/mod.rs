//! Streamable HTTP Transport Integration Tests
//!
//! Tests the full stateful Streamable HTTP transport with:
//! - Session management (Mcp-Session-Id)
//! - Server-initiated notifications (list_changed via SSE)
//! - Proper protocol negotiation

mod notifications;
