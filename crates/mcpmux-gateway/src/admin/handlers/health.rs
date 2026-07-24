//! Admin health endpoint.

use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::atomic::Ordering;

use super::super::router::AdminState;

/// JSON body for `GET /api/v1/health`.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Always `"ok"` when the admin server is reachable.
    pub status: &'static str,
    /// Whether the MCP gateway process reports itself as running.
    pub gateway_running: bool,
}

/// Returns admin and gateway liveness for tunnel health checks.
pub async fn health(State(state): State<AdminState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        gateway_running: state.gateway_running.load(Ordering::Relaxed),
    })
}
