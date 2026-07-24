//! CSRF token middleware for admin mutating HTTP routes.

use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tracing::debug;
use uuid::Uuid;

use super::super::router::AdminState;

/// Header name clients must send on POST/PUT/DELETE requests.
pub const CSRF_HEADER: &str = "X-CSRF-Token";

/// Generate a fresh random CSRF token.
pub fn generate_csrf_token() -> String {
    Uuid::new_v4().to_string()
}

/// Return the current CSRF token for SPA bootstrap.
pub async fn get_csrf_token(State(state): State<AdminState>) -> Json<serde_json::Value> {
    let token = state.csrf_token.lock().clone();
    Json(json!({ "token": token }))
}

fn is_csrf_exempt(method: &Method, path: &str) -> bool {
    if matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS) {
        return true;
    }
    matches!(
        path,
        "/api/v1/csrf-token" | "/api/v1/health" | "/api/v1/events"
    )
}

/// Require matching `X-CSRF-Token` on mutating requests.
pub async fn csrf_middleware(
    State(state): State<AdminState>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    if is_csrf_exempt(&method, &path) {
        return next.run(request).await;
    }

    if !matches!(
        method,
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    ) {
        return next.run(request).await;
    }

    let expected = state.csrf_token.lock().clone();
    let provided = request
        .headers()
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    if provided.as_bytes().ct_ne(expected.as_bytes()).into() {
        debug!(path = %path, "[Admin] CSRF token mismatch");
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Invalid or missing CSRF token" })),
        )
            .into_response();
    }

    next.run(request).await
}

/// Shared CSRF token storage for admin state construction.
pub fn new_csrf_token_store() -> Arc<Mutex<String>> {
    Arc::new(Mutex::new(generate_csrf_token()))
}
