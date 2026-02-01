//! Trace Context - Request correlation and structured logging
//!
//! Generates unique trace IDs and provides structured spans for request tracing.

use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, info_span, Span};

/// Global request counter for trace ID generation
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a short, unique trace ID for this request
/// Format: 6 hex characters (e.g., "a1b2c3")
pub fn generate_trace_id() -> String {
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);

    // Mix counter and timestamp for uniqueness
    let mixed = counter.wrapping_add(timestamp);
    format!("{:06x}", mixed & 0xFFFFFF)
}

/// Trace context for a single request
///
/// Contains all the correlation data needed to track a request through the system.
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// Unique trace ID (6 hex chars)
    pub trace_id: String,
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Request path (e.g., /mcp)
    pub path: String,
    /// MCP method if applicable (e.g., tools/list)
    pub mcp_method: Option<String>,
    /// Client ID from JWT
    pub client_id: Option<String>,
    /// Space ID
    pub space_id: Option<String>,
    /// Request start time
    pub started_at: std::time::Instant,
}

impl TraceContext {
    /// Create a new trace context for an incoming request
    pub fn new(method: &str, path: &str) -> Self {
        Self {
            trace_id: generate_trace_id(),
            method: method.to_string(),
            path: path.to_string(),
            mcp_method: None,
            client_id: None,
            space_id: None,
            started_at: std::time::Instant::now(),
        }
    }

    /// Set the MCP method (parsed from JSON-RPC body)
    pub fn with_mcp_method(mut self, method: Option<String>) -> Self {
        self.mcp_method = method;
        self
    }

    /// Set client context after auth
    pub fn with_client(mut self, client_id: String, space_id: String) -> Self {
        self.client_id = Some(client_id);
        self.space_id = Some(space_id);
        self
    }

    /// Get elapsed time since request started
    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    /// Short client ID for logging (first 8 chars or "anon")
    pub fn short_client(&self) -> &str {
        self.client_id
            .as_ref()
            .map(|c| &c[..c.len().min(12)])
            .unwrap_or("anon")
    }

    /// Short space ID for logging (first 8 chars)
    pub fn short_space(&self) -> &str {
        self.space_id
            .as_ref()
            .map(|s| &s[..s.len().min(8)])
            .unwrap_or("")
    }
}

/// Request span builder for structured logging
pub struct RequestSpan;

impl RequestSpan {
    /// Create a tracing span for an incoming request
    ///
    /// This span will automatically include trace_id in all child logs.
    pub fn enter(ctx: &TraceContext) -> Span {
        info_span!(
            "request",
            trace_id = %ctx.trace_id,
            method = %ctx.method,
            path = %ctx.path,
        )
    }

    /// Log request entry (single consolidated line)
    pub fn log_entry(ctx: &TraceContext) {
        let mcp_method = ctx.mcp_method.as_deref().unwrap_or("-");
        let client = ctx.short_client();

        if ctx.path == "/mcp" {
            info!(
                trace_id = %ctx.trace_id,
                "→ {} {} {} client={}",
                ctx.method,
                ctx.path,
                mcp_method,
                client
            );
        } else {
            info!(
                trace_id = %ctx.trace_id,
                "→ {} {}",
                ctx.method,
                ctx.path
            );
        }
    }

    /// Log request completion (single consolidated line)
    pub fn log_exit(ctx: &TraceContext, status: u16, detail: Option<&str>) {
        let elapsed = ctx.elapsed_ms();

        match detail {
            Some(d) => info!(
                trace_id = %ctx.trace_id,
                "← {} {} ({}ms)",
                status,
                d,
                elapsed
            ),
            None => info!(
                trace_id = %ctx.trace_id,
                "← {} ({}ms)",
                status,
                elapsed
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_trace_id() {
        let id1 = generate_trace_id();
        let id2 = generate_trace_id();

        // Should be 6 hex chars
        assert_eq!(id1.len(), 6);
        assert_eq!(id2.len(), 6);

        // Should be unique
        assert_ne!(id1, id2);

        // Should be valid hex
        assert!(id1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_trace_context() {
        let ctx = TraceContext::new("POST", "/mcp")
            .with_mcp_method(Some("tools/list".to_string()))
            .with_client("mcp_abc123".to_string(), "space-uuid".to_string());

        assert_eq!(ctx.method, "POST");
        assert_eq!(ctx.path, "/mcp");
        assert_eq!(ctx.mcp_method, Some("tools/list".to_string()));
        assert_eq!(ctx.short_client(), "mcp_abc123");
    }

    #[test]
    fn test_short_client() {
        let ctx = TraceContext::new("GET", "/health");
        assert_eq!(ctx.short_client(), "anon");

        let ctx = ctx.with_client("mcp_very_long_client_id".to_string(), "space".to_string());
        assert_eq!(ctx.short_client(), "mcp_very_lon"); // 12 chars max
    }
}
