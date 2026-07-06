//! Authenticated management API (read-only slice).
//!
//! A distinct, bearer-token-gated router mounted under `/admin/api/`, separate
//! from the MCP + OAuth surface. It lets a headless `mcpmux serve` be inspected
//! over HTTP (the desktop app manages via Tauri IPC instead). This is the
//! foundation for the full management surface + web admin (cloud-support M1-07 /
//! M1-09): today it exposes the read endpoints web-admin-v0 needs; write
//! endpoints and an SSE event stream layer on next.
//!
//! Auth: every `/admin/api/*` route requires `Authorization: Bearer <token>`,
//! compared in constant time. On a network bind this is the ONLY gate, so the
//! token must be strong (the serve binary generates 256 bits when the operator
//! doesn't supply one).

use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde_json::json;

use super::handlers::AppState;

/// The expected admin bearer token, carried into the auth middleware.
#[derive(Clone)]
pub struct AdminToken(pub Arc<String>);

/// Constant-time-ish string comparison (length-independent early-out avoided).
fn tokens_match(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Bearer-token gate for `/admin/api/*`. Rejected requests never reach a
/// handler (no data is read without a valid token).
async fn require_admin_token(
    State(expected): State<AdminToken>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !presented.is_empty() && tokens_match(presented, &expected.0) {
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "admin authentication required" })),
    )
        .into_response()
}

/// `GET /admin/api/info` — gateway identity + posture.
async fn admin_info(State(app_state): State<AppState>) -> Response {
    let state = app_state.gateway_state.read().await;
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "base_url": app_state.base_url,
        "network_bind": state.network_bind,
        "auth_required": !state.auth_disabled(),
    }))
    .into_response()
}

/// `GET /admin/api/spaces` — all Spaces.
async fn admin_list_spaces(State(app_state): State<AppState>) -> Response {
    match app_state.services.dependencies.space_repo.list().await {
        Ok(spaces) => Json(json!({ "spaces": spaces })).into_response(),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// `GET /admin/api/clients` — registered inbound clients (identity only).
async fn admin_list_clients(State(app_state): State<AppState>) -> Response {
    match app_state
        .services
        .dependencies
        .inbound_client_repo
        .list_clients()
        .await
    {
        Ok(clients) => {
            let out: Vec<_> = clients
                .into_iter()
                .map(|c| {
                    json!({
                        "client_id": c.client_id,
                        "client_name": c.client_name,
                        "client_alias": c.client_alias,
                        "registration_type": c.registration_type.as_str(),
                        "last_seen": c.last_seen,
                    })
                })
                .collect();
            Json(json!({ "clients": out })).into_response()
        }
        Err(e) => internal_error(&e.to_string()),
    }
}

fn internal_error(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": msg })),
    )
        .into_response()
}

/// `GET /admin` — the minimal web admin console. Self-contained HTML (no
/// external assets) that signs in with the admin token and renders the
/// read-only management data. Public: the page itself holds no secrets; every
/// data call it makes carries the token the operator pastes.
async fn admin_console() -> Response {
    let html = include_str!("admin_console.html");
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

/// Build the management router: the token-gated `/admin/api/*` read endpoints
/// plus the public `/admin` console page. Compose into the gateway (or the
/// serve binary) with the app state and the required admin token.
pub fn management_router(app_state: AppState, admin_token: Arc<String>) -> Router {
    // Token-gated JSON API.
    let api = Router::new()
        .route("/admin/api/info", get(admin_info))
        .route("/admin/api/spaces", get(admin_list_spaces))
        .route("/admin/api/clients", get(admin_list_clients))
        .with_state(app_state)
        .layer(axum::middleware::from_fn_with_state(
            AdminToken(admin_token),
            require_admin_token,
        ));
    // Public console page (its API calls carry the operator-entered token).
    Router::new().route("/admin", get(admin_console)).merge(api)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison_is_correct() {
        assert!(tokens_match("abc123", "abc123"));
        assert!(!tokens_match("abc123", "abc124"));
        assert!(!tokens_match("abc", "abc123")); // length mismatch
        assert!(!tokens_match("", "x"));
    }
}
