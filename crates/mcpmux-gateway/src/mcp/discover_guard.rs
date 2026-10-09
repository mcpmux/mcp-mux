//! Keep MCP clients on the protocol revisions that have sessions.
//!
//! From 2026-07-28 MCP has no sessions: a client opens with `server/discover`
//! and every request stands alone. The gateway needs a session per client for
//! `on_initialized`, the roots probe and list_changed over SSE, so it only
//! offers versions up to 2025-11-25.
//!
//! rmcp answers `server/discover` itself, and a client asking for 2026-07-28
//! only gets "unsupported protocol version" back, which ends its connection
//! attempt. Answering "method not found" instead, as every server from before
//! 2026-07-28 does, makes the client fall back to `initialize` and a session.

use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// rmcp's own limit on a request body; larger requests get 413 there too.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// JSON-RPC "method not found".
const METHOD_NOT_FOUND: i64 = -32601;

/// Answer `server/discover` with "method not found" before rmcp sees it.
pub async fn reject_server_discover(request: Request, next: Next) -> Response {
    if request.method() != Method::POST {
        return next.run(request).await;
    }

    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, MAX_BODY_BYTES).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };

    if let Some(id) = discover_request_id(&bytes) {
        return Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": METHOD_NOT_FOUND,
                "message": "Method not found: server/discover",
            },
        }))
        .into_response();
    }

    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

/// The id of a `server/discover` request, or `None` for any other message.
fn discover_request_id(body: &[u8]) -> Option<Value> {
    #[derive(Deserialize)]
    struct Message {
        method: Option<String>,
        id: Option<Value>,
    }

    let message: Message = serde_json::from_slice(body).ok()?;
    if message.method.as_deref() != Some("server/discover") {
        return None;
    }
    message.id
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{middleware, routing::post, Router};
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/mcp", post(|body: String| async move { body }))
            .layer(middleware::from_fn(reject_server_discover))
    }

    async fn send(body: &str) -> (StatusCode, String) {
        let request = Request::post("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap();
        let response = app().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn discover_gets_method_not_found_with_its_id() {
        let (status, body) =
            send(r#"{"jsonrpc":"2.0","id":7,"method":"server/discover","params":{}}"#).await;

        assert_eq!(status, StatusCode::OK);
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["id"], 7);
        assert_eq!(body["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn other_requests_reach_the_server_unchanged() {
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;

        let (status, body) = send(request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, request);
    }

    #[tokio::test]
    async fn a_discover_notification_is_not_answered() {
        let notification = r#"{"jsonrpc":"2.0","method":"server/discover"}"#;

        let (_, body) = send(notification).await;

        assert_eq!(body, notification);
    }
}
