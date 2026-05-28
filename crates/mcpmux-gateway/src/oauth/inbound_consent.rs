//! Inbound OAuth consent — shared logic for Tauri IPC and admin HTTP.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use urlencoding;

use crate::admin::ui_events::AdminUiEventBus;
use crate::server::{GatewayState, PendingAuthorization};

/// Tauri / SSE channel name for new consent requests.
pub const OAUTH_CONSENT_EVENT: &str = "oauth-consent-request";

/// Error type for consent operations.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentError {
    pub code: String,
    pub message: String,
}

impl ConsentError {
    /// Authorization request not found or already consumed.
    pub fn not_found(request_id: &str) -> Self {
        Self {
            code: "NOT_FOUND".to_string(),
            message: format!(
                "Authorization request '{}' not found or expired",
                request_id
            ),
        }
    }

    /// Authorization request expired.
    pub fn expired(request_id: &str) -> Self {
        Self {
            code: "EXPIRED".to_string(),
            message: format!("Authorization request '{}' has expired", request_id),
        }
    }

    /// Gateway process is not running.
    pub fn gateway_unavailable() -> Self {
        Self {
            code: "GATEWAY_UNAVAILABLE".to_string(),
            message: "Gateway is not running".to_string(),
        }
    }
}

/// Full consent request details returned by `get_pending_consent`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentRequestDetails {
    pub request_id: String,
    pub client_id: String,
    pub client_name: String,
    pub redirect_uri: String,
    pub scope: String,
    pub state: Option<String>,
    pub expires_at: i64,
    pub consent_token: String,
}

/// Request to approve or deny OAuth consent.
#[derive(Debug, Deserialize)]
pub struct ConsentApprovalRequest {
    pub request_id: String,
    pub approved: bool,
    pub consent_token: String,
    #[serde(default)]
    pub client_alias: Option<String>,
}

/// Response from consent approval.
#[derive(Debug, Serialize, Deserialize)]
pub struct ConsentApprovalResponse {
    pub success: bool,
    pub redirect_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Validate a pending authorization and return authoritative consent details.
///
/// Removes expired entries from the pending-authorizations map when encountered.
pub async fn get_pending_consent(
    gateway_state: &Arc<RwLock<GatewayState>>,
    request_id: String,
) -> Result<ConsentRequestDetails, ConsentError> {
    info!(
        "[OAuth] Fetching pending consent: request_id='{}'",
        request_id
    );

    let auth = {
        let state = gateway_state.read().await;
        state.pending_authorizations.get(&request_id).cloned()
    };

    let auth = auth.ok_or_else(|| ConsentError::not_found(&request_id))?;

    let now = now_unix_secs();
    if auth.expires_at < now {
        warn!("[OAuth] Request '{}' has expired", request_id);
        let mut state = gateway_state.write().await;
        state.pending_authorizations.remove(&request_id);
        return Err(ConsentError::expired(&request_id));
    }

    let consent_token = auth.consent_token.clone().ok_or_else(|| {
        error!("[OAuth] Pending authorization missing consent_token");
        ConsentError {
            code: "NOT_FOUND".to_string(),
            message: "Authorization request is missing consent token — it may have been created before this security update. Please retry.".to_string(),
        }
    })?;

    let details = ConsentRequestDetails {
        request_id: request_id.clone(),
        client_id: auth.client_id.clone(),
        client_name: auth
            .client_name
            .clone()
            .unwrap_or_else(|| auth.client_id.clone()),
        redirect_uri: auth.redirect_uri.clone(),
        scope: auth.scope.clone().unwrap_or_default(),
        state: auth.state.clone(),
        expires_at: auth.expires_at,
        consent_token,
    };

    info!(
        "[OAuth] Consent details validated: client='{}' scopes='{}'",
        details.client_name, details.scope
    );

    Ok(details)
}

/// Approve or deny a pending OAuth consent request.
pub async fn approve_oauth_consent(
    gateway_state: &Arc<RwLock<GatewayState>>,
    request: ConsentApprovalRequest,
) -> Result<ConsentApprovalResponse, String> {
    info!(
        "[OAuth] Consent {} for request_id: {}",
        if request.approved {
            "approved"
        } else {
            "denied"
        },
        request.request_id
    );

    let pending = {
        let state = gateway_state.read().await;
        state
            .pending_authorizations
            .get(&request.request_id)
            .cloned()
    };

    let Some(pending) = pending else {
        error!("[OAuth] Consent approval failed: request_id not found");
        return Ok(ConsentApprovalResponse {
            success: false,
            redirect_url: String::new(),
            error: Some("Authorization request not found or expired".to_string()),
        });
    };

    match &pending.consent_token {
        Some(expected_token) => {
            if request.consent_token != *expected_token {
                error!(
                    "[OAuth] Consent token mismatch for request_id: {} — possible unauthorized approval attempt",
                    request.request_id
                );
                return Err("Invalid consent token".to_string());
            }
        }
        None => {
            error!(
                "[OAuth] Pending authorization missing consent_token for request_id: {}",
                request.request_id
            );
            return Err("Consent token not available".to_string());
        }
    }

    {
        let mut state = gateway_state.write().await;
        state.pending_authorizations.remove(&request.request_id);
    }

    if !request.approved {
        let redirect_url = build_denied_redirect(&pending);
        info!(
            "[OAuth] User denied consent for client: {}",
            pending.client_id
        );
        return Ok(ConsentApprovalResponse {
            success: true,
            redirect_url,
            error: None,
        });
    }

    let code = format!("mc_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
    let code_expires_at = now_unix_secs() + 600;

    {
        let mut state = gateway_state.write().await;

        let new_pending = PendingAuthorization {
            client_id: pending.client_id.clone(),
            client_name: pending.client_name.clone(),
            redirect_uri: pending.redirect_uri.clone(),
            scope: pending.scope.clone(),
            state: pending.state.clone(),
            code_challenge: pending.code_challenge.clone(),
            code_challenge_method: pending.code_challenge_method.clone(),
            expires_at: code_expires_at,
            consent_token: None,
        };

        state.store_pending_authorization(&code, new_pending);

        if let Some(repo) = state.inbound_client_repository() {
            if let Err(e) = repo.approve_client(&pending.client_id).await {
                error!("[OAuth] Failed to approve client: {}", e);
            } else {
                info!("[OAuth] Client approved: {}", pending.client_id);
            }

            if let Some(alias) = request
                .client_alias
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(String::from)
            {
                if let Err(e) = repo
                    .update_client_alias(&pending.client_id, Some(alias.clone()))
                    .await
                {
                    error!("[OAuth] Failed to save client alias: {}", e);
                } else {
                    info!(
                        "[OAuth] Set client alias '{}' for: {}",
                        alias, pending.client_id
                    );
                }
            }
        }

        state.emit_domain_event(mcpmux_core::DomainEvent::ClientRegistered {
            client_id: pending.client_id.clone(),
            client_name: pending.client_id.clone(),
            registration_type: Some("unknown".to_string()),
        });
    }

    let redirect_url = build_approved_redirect(&pending, &code);

    info!(
        "[OAuth] Authorization approved for client: {}, issuing code",
        pending.client_id
    );

    Ok(ConsentApprovalResponse {
        success: true,
        redirect_url,
        error: None,
    })
}

fn build_denied_redirect(pending: &PendingAuthorization) -> String {
    let mut redirect_url = pending.redirect_uri.clone();
    redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
    redirect_url.push_str("error=access_denied&error_description=User+denied+the+request");
    if let Some(ref state_param) = pending.state {
        redirect_url.push_str(&format!("&state={}", urlencoding::encode(state_param)));
    }
    redirect_url
}

fn build_approved_redirect(pending: &PendingAuthorization, code: &str) -> String {
    let mut redirect_url = pending.redirect_uri.clone();
    redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
    redirect_url.push_str(&format!("code={code}"));
    if let Some(ref state_param) = pending.state {
        redirect_url.push_str(&format!("&state={}", urlencoding::encode(state_param)));
    }
    redirect_url
}

/// Publish an `oauth-consent-request` UI event for web admin SSE subscribers.
pub fn emit_consent_request(ui_bus: &AdminUiEventBus, request_id: &str) {
    info!(
        "[OAuth] Publishing SSE event '{}': request_id='{}'",
        OAUTH_CONSENT_EVENT, request_id
    );
    ui_bus.publish(
        OAUTH_CONSENT_EVENT,
        serde_json::json!({ "requestId": request_id }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;
    use tracing_test::traced_test;

    async fn seed_pending(
        gateway_state: &Arc<RwLock<GatewayState>>,
        request_id: &str,
        consent_token: &str,
    ) {
        let expires_at = now_unix_secs() + 300;
        let pending = PendingAuthorization {
            client_id: "test-client".to_string(),
            client_name: Some("Test Client".to_string()),
            redirect_uri: "http://127.0.0.1/callback".to_string(),
            scope: Some("mcp".to_string()),
            state: Some("state-1".to_string()),
            code_challenge: None,
            code_challenge_method: None,
            expires_at,
            consent_token: Some(consent_token.to_string()),
        };
        gateway_state
            .write()
            .await
            .store_pending_authorization(request_id, pending);
    }

    #[traced_test]
    #[tokio::test]
    async fn approve_oauth_consent_does_not_log_authorization_code() {
        let (tx, _) = broadcast::channel(4);
        let gateway_state = Arc::new(RwLock::new(GatewayState::new(tx)));

        seed_pending(&gateway_state, "req-log-test", "consent-secret").await;

        let response = approve_oauth_consent(
            &gateway_state,
            ConsentApprovalRequest {
                request_id: "req-log-test".to_string(),
                approved: true,
                consent_token: "consent-secret".to_string(),
                client_alias: None,
            },
        )
        .await
        .expect("approve consent");

        assert!(response.success);
        assert!(response.redirect_url.contains("code=mc_"));
        assert!(!logs_contain("code=mc_"));
        assert!(!logs_contain("Redirect URL"));
    }
}
