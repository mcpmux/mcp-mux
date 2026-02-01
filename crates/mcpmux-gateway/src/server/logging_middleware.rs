//! HTTP Request/Response Logging Middleware
//!
//! Centralized logging with trace IDs for request correlation.
//! Uses TraceContext for consistent, non-repetitive logging.

use axum::{body::Body, extract::Request, http::StatusCode, middleware::Next, response::Response};
use http_body_util::BodyExt;
use tracing::{debug, warn, Instrument};

use crate::logging::{RequestSpan, TraceContext};

/// Maximum body size to log (1MB)
const MAX_BODY_LOG_SIZE: usize = 1024 * 1024;

/// Paths that should have bodies redacted (contain sensitive data)
const SENSITIVE_PATHS: &[&str] = &["/oauth/token", "/oauth/register"];

/// Paths that should skip body logging (too large or not useful)
const SKIP_BODY_PATHS: &[&str] = &["/oauth/authorize", "/oauth/consent"];

/// Headers that should be redacted
const SENSITIVE_HEADERS: &[&str] = &["authorization", "cookie", "set-cookie", "x-api-key"];

/// Check if a path contains sensitive data
pub fn is_sensitive_path(path: &str) -> bool {
    SENSITIVE_PATHS.iter().any(|p| path.contains(p))
}

/// Check if a path should skip body logging
fn should_skip_body(path: &str) -> bool {
    SKIP_BODY_PATHS.iter().any(|p| path.contains(p))
}

/// Redact sensitive headers (compact format for DEBUG)
fn redact_headers_compact(headers: &axum::http::HeaderMap) -> String {
    headers
        .iter()
        .filter(|(name, _)| {
            // Only include important headers for debugging
            let n = name.as_str().to_lowercase();
            matches!(
                n.as_str(),
                "content-type"
                    | "accept"
                    | "user-agent"
                    | "mcp-session-id"
                    | "mcp-protocol-version"
            )
        })
        .map(|(name, value)| {
            let name_lower = name.as_str().to_lowercase();
            if SENSITIVE_HEADERS.contains(&name_lower.as_str()) {
                format!("{}=[REDACTED]", name)
            } else {
                format!("{}={:?}", name, value)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format bytes as string - compact version
pub fn format_body(bytes: &[u8], redact: bool) -> String {
    if redact {
        return "[REDACTED]".to_string();
    }

    if bytes.is_empty() {
        return "[empty]".to_string();
    }

    if bytes.len() > MAX_BODY_LOG_SIZE {
        return format!("[{} bytes]", bytes.len());
    }

    // Try to parse as UTF-8
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            // For JSON, extract just the method for MCP requests
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                if let Some(method) = json.get("method").and_then(|m| m.as_str()) {
                    return method.to_string();
                }
                // Compact JSON for other cases
                return serde_json::to_string(&json).unwrap_or_else(|_| text.to_string());
            }
            // Truncate long text
            if text.len() > 200 {
                format!("{}...", &text[..200])
            } else {
                text.to_string()
            }
        }
        Err(_) => format!("[binary: {} bytes]", bytes.len()),
    }
}

/// Format MCP response body - summarizes the response content
fn format_mcp_response(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let json: serde_json::Value = serde_json::from_str(text).ok()?;

    // Check for error response
    if let Some(error) = json.get("error") {
        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        return Some(format!("error: {} ({})", message, code));
    }

    // Check for result
    if let Some(result) = json.get("result") {
        // Summarize common result types
        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            return Some(format!("tools: {}", tools.len()));
        }
        if let Some(resources) = result.get("resources").and_then(|r| r.as_array()) {
            return Some(format!("resources: {}", resources.len()));
        }
        if let Some(prompts) = result.get("prompts").and_then(|p| p.as_array()) {
            return Some(format!("prompts: {}", prompts.len()));
        }
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            let types: Vec<&str> = content
                .iter()
                .filter_map(|c| c.get("type").and_then(|t| t.as_str()))
                .collect();
            return Some(format!(
                "content: {} items [{}]",
                content.len(),
                types.join(", ")
            ));
        }
        // For initialize response
        if result.get("protocolVersion").is_some() {
            let version = result
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let name = result
                .get("serverInfo")
                .and_then(|s| s.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("?");
            return Some(format!("initialized: {} ({})", name, version));
        }
        // Generic result
        if result.is_object() {
            let keys: Vec<&str> = result
                .as_object()
                .unwrap()
                .keys()
                .map(|k| k.as_str())
                .collect();
            if keys.is_empty() {
                return Some("ok".to_string());
            }
            return Some(format!("result: {{{}}}", keys.join(", ")));
        }
        return Some("ok".to_string());
    }

    // No result or error - likely a notification response (202)
    None
}

/// Extract MCP method from JSON-RPC body
pub fn extract_mcp_method(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    json.get("method")
        .and_then(|m| m.as_str())
        .map(String::from)
}

/// Logging middleware for requests and responses
///
/// Generates a trace_id and logs a single entry/exit line per request.
pub async fn http_logging_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let method = request.method().to_string();
    let uri = request.uri().clone();
    let path = uri.path().to_string();
    let headers = request.headers().clone();
    let is_sensitive = is_sensitive_path(&path);

    // Create trace context
    let ctx = TraceContext::new(&method, &path);

    // For MCP routes, capture response body for logging
    if path == "/mcp" {
        // Create span for this request
        let span = RequestSpan::enter(&ctx);

        // Store trace_id in request extensions for downstream use
        let mut request = request;
        request.extensions_mut().insert(ctx.clone());

        async move {
            // Minimal header logging at DEBUG level
            debug!(
                trace_id = %ctx.trace_id,
                headers = %redact_headers_compact(&headers),
                "MCP request"
            );

            let response = next.run(request).await;
            let status = response.status().as_u16();

            // Extract response body to log it
            let (parts, body) = response.into_parts();
            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(e) => {
                    warn!(trace_id = %ctx.trace_id, "Failed to read response body: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };

            // Log response summary at DEBUG level
            if let Some(summary) = format_mcp_response(&body_bytes) {
                debug!(
                    trace_id = %ctx.trace_id,
                    response = %summary,
                    "MCP response"
                );
            }

            // Single exit log
            RequestSpan::log_exit(&ctx, status, None);

            // Reconstruct response
            let response = Response::from_parts(parts, Body::from(body_bytes));

            Ok(response)
        }
        .instrument(span)
        .await
    } else {
        // Non-MCP routes: full request/response logging
        let span = RequestSpan::enter(&ctx);

        async move {
            // Log entry
            RequestSpan::log_entry(&ctx);

            // Extract and log request body for non-MCP routes
            let (parts, body) = request.into_parts();
            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(e) => {
                    warn!(trace_id = %ctx.trace_id, "Failed to read request body: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };

            if !should_skip_body(&path) && !body_bytes.is_empty() {
                debug!(
                    trace_id = %ctx.trace_id,
                    body = %format_body(&body_bytes, is_sensitive),
                    "Request body"
                );
            }

            // Reconstruct request with body
            let request = Request::from_parts(parts, Body::from(body_bytes));

            // Call next middleware/handler
            let response = next.run(request).await;

            // Extract and log response
            let (parts, body) = response.into_parts();
            let status = parts.status;

            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(e) => {
                    warn!(trace_id = %ctx.trace_id, "Failed to read response body: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };

            // Log response body only if small enough
            if !should_skip_body(&path) && !body_bytes.is_empty() && body_bytes.len() < 1000 {
                debug!(
                    trace_id = %ctx.trace_id,
                    body = %format_body(&body_bytes, is_sensitive),
                    "Response body"
                );
            }

            // Single exit log
            RequestSpan::log_exit(&ctx, status.as_u16(), None);

            // Reconstruct response
            let response = Response::from_parts(parts, Body::from(body_bytes));

            Ok(response)
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_sensitive_path() {
        assert!(is_sensitive_path("/oauth/token"));
        assert!(is_sensitive_path("/api/oauth/token"));
        assert!(is_sensitive_path("/oauth/register"));
        assert!(!is_sensitive_path("/oauth/authorize"));
        assert!(!is_sensitive_path("/health"));
    }

    #[test]
    fn test_format_body() {
        // Empty
        assert_eq!(format_body(&[], false), "[empty]");

        // JSON with method
        let json = br#"{"method":"tools/list","jsonrpc":"2.0"}"#;
        assert_eq!(format_body(json, false), "tools/list");

        // Redacted
        assert!(format_body(json, true).contains("REDACTED"));

        // Binary
        let binary = &[0x00, 0x01, 0xFF];
        assert!(format_body(binary, false).contains("binary"));
    }

    #[test]
    fn test_extract_mcp_method() {
        let body = br#"{"method":"tools/call","params":{},"jsonrpc":"2.0","id":1}"#;
        assert_eq!(extract_mcp_method(body), Some("tools/call".to_string()));

        let no_method = br#"{"result":{}}"#;
        assert_eq!(extract_mcp_method(no_method), None);
    }

    #[test]
    fn test_format_mcp_response() {
        // Tools list
        let tools = br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"a"},{"name":"b"}]}}"#;
        assert_eq!(format_mcp_response(tools), Some("tools: 2".to_string()));

        // Resources list
        let resources = br#"{"jsonrpc":"2.0","id":1,"result":{"resources":[{"uri":"x"}]}}"#;
        assert_eq!(
            format_mcp_response(resources),
            Some("resources: 1".to_string())
        );

        // Error response
        let error =
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid request"}}"#;
        assert_eq!(
            format_mcp_response(error),
            Some("error: Invalid request (-32600)".to_string())
        );

        // Empty result
        let empty = br#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        assert_eq!(format_mcp_response(empty), Some("ok".to_string()));

        // No result (notification ack)
        let no_result = br#"{}"#;
        assert_eq!(format_mcp_response(no_result), None);
    }
}
