//! OAuth consent admin REST handlers (web admin only).

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::admin::command_bridge::oauth as bridge;
use crate::admin::command_bridge::oauth::OAuthConsentBody;
use crate::admin::handlers::error::ApiError;
use crate::admin::router::AdminState;

#[derive(Debug, Deserialize)]
pub struct PendingConsentQuery {
    #[serde(rename = "requestId")]
    pub request_id: String,
}

fn ok(value: Value) -> Json<Value> {
    Json(value)
}

fn consent_error(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("Invalid consent token") || message.contains("Consent token") {
        return ApiError::bad_request(message);
    }
    if message.starts_with("NOT_FOUND") || message.starts_with("EXPIRED") {
        return ApiError::bad_request(message);
    }
    if message.contains("Gateway not running") {
        return ApiError::service_unavailable(message);
    }
    ApiError::from_bridge(err)
}

/// GET /api/v1/oauth/consent/pending — load validated consent details for the modal.
pub async fn get_pending_consent(
    State(state): State<AdminState>,
    Query(query): Query<PendingConsentQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_pending_consent(&state.bridge, query.request_id)
        .await
        .map(ok)
        .map_err(consent_error)
}

/// POST /api/v1/oauth/consent/approve — approve pending OAuth consent (CSRF required).
pub async fn approve_oauth_consent(
    State(state): State<AdminState>,
    Json(body): Json<OAuthConsentBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::approve_oauth_consent(&state.bridge, body)
        .await
        .map(ok)
        .map_err(consent_error)
}

/// POST /api/v1/oauth/consent/reject — deny pending OAuth consent (CSRF required).
pub async fn reject_oauth_consent(
    State(state): State<AdminState>,
    Json(body): Json<OAuthConsentBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::reject_oauth_consent(&state.bridge, body)
        .await
        .map(ok)
        .map_err(consent_error)
}
