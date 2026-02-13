//! HTTP handlers for the gateway server

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use mcpmux_core::branding;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use super::{GatewayState, ServiceContainer};
use crate::auth::{create_access_token, create_refresh_token};
use crate::oauth::{process_dcr_request, DcrError, DcrRequest, DcrResponse};

/// App State structure holding both GatewayState and ServiceContainer
#[derive(Clone)]
pub struct AppState {
    pub gateway_state: Arc<RwLock<GatewayState>>,
    pub services: Arc<ServiceContainer>,
    pub base_url: String,
}

impl axum::extract::FromRef<AppState> for Arc<RwLock<GatewayState>> {
    fn from_ref(state: &AppState) -> Self {
        state.gateway_state.clone()
    }
}

/// Health check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Health check endpoint
pub async fn health() -> Json<HealthResponse> {
    debug!("[Gateway] Health check");
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// OAuth Authorization Server Metadata (RFC 8414)
#[derive(Serialize)]
pub struct OAuthServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: String,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub scopes_supported: Vec<String>,

    // MCP spec 2025-11-25: Support for Client ID Metadata Documents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id_metadata_document_supported: Option<bool>,
}

/// OAuth metadata endpoint (RFC 8414)
pub async fn oauth_metadata(
    axum::extract::State(app_state): axum::extract::State<AppState>,
) -> Json<OAuthServerMetadata> {
    info!("[Gateway] OAuth metadata request - serving authorization server metadata");
    let base = &app_state.base_url;
    Json(OAuthServerMetadata {
        issuer: base.to_string(),
        authorization_endpoint: format!("{}/oauth/authorize", base),
        token_endpoint: format!("{}/oauth/token", base),
        registration_endpoint: format!("{}/oauth/register", base),
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        code_challenge_methods_supported: vec!["S256".to_string()],
        token_endpoint_auth_methods_supported: vec!["none".to_string()],
        scopes_supported: vec!["mcp".to_string(), "offline_access".to_string()],

        // MCP spec 2025-11-25: Advertise CIMD support
        client_id_metadata_document_supported: Some(true),
    })
}

/// OAuth Protected Resource Metadata (RFC 9728)
#[derive(Serialize)]
pub struct ProtectedResourceMetadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
}

/// Protected resource metadata endpoint (RFC 9728)
/// This tells MCP clients where to find the authorization server
pub async fn resource_metadata(
    axum::extract::State(app_state): axum::extract::State<AppState>,
) -> Json<ProtectedResourceMetadata> {
    info!("[Gateway] Protected resource metadata request");
    let base = &app_state.base_url;
    Json(ProtectedResourceMetadata {
        resource: format!("{}/mcp", base),
        authorization_servers: vec![base.to_string()],
        scopes_supported: Some(vec!["mcp".to_string(), "offline_access".to_string()]),
    })
}

/// OAuth authorization query params
#[derive(Debug, Deserialize)]
pub struct AuthorizeParams {
    pub response_type: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

/// Pending authorization (stored while waiting for consent)
#[derive(Debug, Clone)]
pub struct PendingAuthorization {
    pub client_id: String,
    pub client_name: Option<String>,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    /// Unix timestamp when this request expires
    pub expires_at: i64,
}

/// OAuth authorization endpoint
///
/// This endpoint receives the authorization request and:
/// 1. Validates the client_id and redirect_uri
/// 2. Shows consent UI (TODO: for now auto-approves)
/// 3. Generates auth code and redirects back to client
pub async fn oauth_authorize(
    State(state): State<Arc<RwLock<GatewayState>>>,
    Query(params): Query<AuthorizeParams>,
) -> Response {
    info!(
        "[OAuth] Authorization request: client_id={}, response_type={}, redirect_uri={}",
        params.client_id, params.response_type, params.redirect_uri
    );

    if params.response_type != "code" {
        warn!(
            "[OAuth] Unsupported response_type: {}",
            params.response_type
        );
        return oauth_error_redirect(
            &params.redirect_uri,
            "unsupported_response_type",
            "Only 'code' response type is supported",
            params.state.as_deref(),
        );
    }

    // Resolve and validate client (CIMD or traditional)
    {
        let gateway_state = state.read().await;

        let client_metadata_service = match gateway_state.client_metadata_service() {
            Some(s) => s,
            None => {
                error!("[OAuth] ClientMetadataService not available");
                return oauth_error_redirect(
                    &params.redirect_uri,
                    "server_error",
                    "Service not available",
                    params.state.as_deref(),
                );
            }
        };

        // Resolve client (handles CIMD URL or traditional client_id)
        let client = match client_metadata_service
            .resolve_client(&params.client_id)
            .await
        {
            Ok(Some(c)) => c,
            Ok(None) => {
                warn!("[OAuth] Unknown client_id: {}", params.client_id);
                return oauth_error_redirect(
                    &params.redirect_uri,
                    "invalid_client",
                    "Client not registered",
                    params.state.as_deref(),
                );
            }
            Err(e) => {
                error!("[OAuth] Client resolution failed: {}", e);
                return oauth_error_redirect(
                    &params.redirect_uri,
                    "server_error",
                    "Client resolution error",
                    params.state.as_deref(),
                );
            }
        };

        // Validate redirect_uri against resolved client
        if !client.redirect_uris.contains(&params.redirect_uri) {
            warn!(
                "[OAuth] Invalid redirect_uri for client: {} (expected one of: {:?})",
                params.redirect_uri, client.redirect_uris
            );
            return oauth_error_redirect(
                &params.redirect_uri,
                "invalid_redirect_uri",
                "Redirect URI not registered for this client",
                params.state.as_deref(),
            );
        }
    }

    // PKCE is required for public clients
    if params.code_challenge.is_none() {
        warn!("[OAuth] PKCE required but code_challenge missing");
        return oauth_error_redirect(
            &params.redirect_uri,
            "invalid_request",
            "PKCE code_challenge is required",
            params.state.as_deref(),
        );
    }

    if let Some(ref scope) = params.scope {
        debug!("[OAuth] Requested scope: {}", scope);
    }
    debug!(
        "[OAuth] PKCE code_challenge present (method: {:?})",
        params.code_challenge_method
    );

    // Security: Always show consent prompt, even for previously approved clients
    // This ensures user explicitly approves each session
    // Note: DCR/CIMD prevent duplicate clients - they update existing by client_name

    info!(
        "[OAuth] Showing consent page for client: {}",
        params.client_id
    );

    // Get client display name from metadata service for new clients
    let display_name = {
        let gateway_state = state.read().await;
        if let Some(service) = gateway_state.client_metadata_service() {
            match service.resolve_client(&params.client_id).await {
                Ok(Some(client)) => client.client_name,
                _ => "Unknown Application".to_string(),
            }
        } else {
            "Unknown Application".to_string()
        }
    };

    // Store pending authorization request with expiration (5 minutes)
    let request_id = uuid::Uuid::new_v4().to_string();
    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 + 300) // 5 minutes
        .unwrap_or(i64::MAX);

    {
        let mut gateway_state = state.write().await;
        gateway_state.store_pending_authorization(
            &request_id,
            PendingAuthorization {
                client_id: params.client_id.clone(),
                client_name: Some(display_name.clone()),
                redirect_uri: params.redirect_uri.clone(),
                scope: params.scope.clone(),
                state: params.state.clone(),
                code_challenge: params.code_challenge.clone(),
                code_challenge_method: params.code_challenge_method.clone(),
                expires_at,
            },
        );
    }

    // Build deep link URL for the Tauri app (only request_id - app fetches details from backend)
    let deep_link_url = format!(
        "{}://authorize?request_id={}",
        branding::DEEP_LINK_SCHEME,
        urlencoding::encode(&request_id),
    );

    info!("[OAuth] Deep link URL: {}", deep_link_url);

    let app_name = branding::DISPLAY_NAME;

    // HTML page that triggers the deep link
    // The page shows a brief message while the app opens
    // Industry standard: Don't auto-close, let user close after approval
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{app_name} - Authorization</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
            background: linear-gradient(135deg, #0f0f23 0%, #1a1a2e 50%, #16213e 100%);
            color: #e6e6e6;
            padding: 1rem;
        }}
        .container {{
            text-align: center;
            max-width: 400px;
        }}
        .icon {{
            width: 64px;
            height: 64px;
            margin: 0 auto 1.5rem;
            background: linear-gradient(135deg, #64ffda 0%, #00bcd4 100%);
            border-radius: 16px;
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 2rem;
        }}
        h1 {{
            font-size: 1.5rem;
            font-weight: 600;
            margin-bottom: 0.75rem;
            color: #fff;
        }}
        .subtitle {{
            color: #8892b0;
            margin-bottom: 2rem;
            line-height: 1.5;
        }}
        .client-info {{
            background: rgba(255,255,255,0.05);
            border: 1px solid rgba(255,255,255,0.1);
            border-radius: 12px;
            padding: 1rem;
            margin-bottom: 1.5rem;
        }}
        .client-name {{
            font-weight: 500;
            color: #64ffda;
            margin-bottom: 0.25rem;
        }}
        .client-id {{
            font-size: 0.75rem;
            color: #6a7394;
            word-break: break-all;
        }}
        .action {{
            margin-top: 1.5rem;
        }}
        .btn {{
            display: inline-block;
            background: linear-gradient(135deg, #64ffda 0%, #00bcd4 100%);
            color: #0f0f23;
            padding: 0.75rem 2rem;
            border-radius: 8px;
            text-decoration: none;
            font-weight: 500;
            transition: transform 0.2s, box-shadow 0.2s;
            cursor: pointer;
            border: none;
            font-size: 1rem;
        }}
        .btn:hover {{
            transform: translateY(-2px);
            box-shadow: 0 4px 20px rgba(100, 255, 218, 0.3);
        }}
        .btn-secondary {{
            background: transparent;
            border: 1px solid rgba(255,255,255,0.2);
            color: #8892b0;
            margin-top: 1rem;
        }}
        .btn-secondary:hover {{
            background: rgba(255,255,255,0.05);
            border-color: rgba(255,255,255,0.3);
            box-shadow: none;
            transform: none;
        }}
        .note {{
            margin-top: 2rem;
            font-size: 0.875rem;
            color: #6a7394;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="icon">🔐</div>
        <h1>Authorization Request</h1>
        <p class="subtitle">
            Complete authorization in {app_name}
        </p>
        
        <div class="client-info">
            <div class="client-name">{display_name}</div>
            <div class="client-id">wants to connect</div>
        </div>
        
        <div class="action">
            <a href="{deep_link_url}" class="btn">Open {app_name}</a>
            <button class="btn btn-secondary" onclick="window.close()">Close this tab</button>
        </div>
        
        <p class="note">
            If prompted by your browser, click "Open" to allow.
        </p>
    </div>
    <script>
        // Trigger deep link using an iframe to avoid window flash on Windows
        // This method is less intrusive than window.location.href
        (function() {{
            var iframe = document.createElement('iframe');
            iframe.style.display = 'none';
            iframe.src = "{deep_link_url}";
            document.body.appendChild(iframe);
            
            // Fallback: remove iframe after a short delay
            // The protocol handler should have fired by then
            setTimeout(function() {{
                if (iframe.parentNode) {{
                    iframe.parentNode.removeChild(iframe);
                }}
            }}, 1000);
        }})();
    </script>
</body>
</html>"#
    );

    axum::response::Html(html).into_response()
}

/// Helper to create OAuth error redirect
fn oauth_error_redirect(
    redirect_uri: &str,
    error: &str,
    description: &str,
    state: Option<&str>,
) -> Response {
    let mut url = redirect_uri.to_string();
    url.push_str(if url.contains('?') { "&" } else { "?" });
    url.push_str(&format!(
        "error={}&error_description={}",
        error,
        urlencoding::encode(description)
    ));
    if let Some(s) = state {
        url.push_str(&format!("&state={}", s));
    }
    axum::response::Redirect::to(&url).into_response()
}

/// OAuth token request body
#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    #[allow(dead_code)] // Received but not used (PKCE flow)
    pub client_secret: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
}

/// OAuth token response
#[derive(Debug, Serialize)]
pub struct TokenResponseBody {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// OAuth error response
#[derive(Debug, Serialize)]
pub struct TokenErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

/// OAuth token endpoint with proper PKCE validation and JWT issuance
pub async fn oauth_token(
    State(state): State<Arc<RwLock<GatewayState>>>,
    axum::Form(request): axum::Form<TokenRequest>,
) -> Result<Json<TokenResponseBody>, (StatusCode, Json<TokenErrorResponse>)> {
    info!(
        "[OAuth] Token request: grant_type={}, client_id={:?}",
        request.grant_type, request.client_id
    );

    match request.grant_type.as_str() {
        "authorization_code" => {
            // Validate required fields
            let Some(code) = request.code.as_ref() else {
                warn!("[OAuth] Missing authorization code");
                return Err(token_error("invalid_request", "Missing authorization code"));
            };
            let Some(code_verifier) = request.code_verifier.as_ref() else {
                warn!("[OAuth] Missing code_verifier (PKCE required)");
                return Err(token_error("invalid_request", "Missing code_verifier"));
            };

            // Look up and consume the authorization code
            let pending = {
                let mut gateway_state = state.write().await;
                gateway_state.consume_pending_authorization(code)
            };

            let Some(pending) = pending else {
                warn!("[OAuth] Unknown or expired authorization code");
                return Err(token_error(
                    "invalid_grant",
                    "Authorization code is invalid or expired",
                ));
            };

            // Validate client_id matches
            if let Some(ref client_id) = request.client_id {
                if client_id != &pending.client_id {
                    warn!("[OAuth] client_id mismatch");
                    return Err(token_error("invalid_grant", "Client ID mismatch"));
                }
            }

            // Validate redirect_uri matches
            if let Some(ref redirect_uri) = request.redirect_uri {
                if redirect_uri != &pending.redirect_uri {
                    warn!("[OAuth] redirect_uri mismatch");
                    return Err(token_error("invalid_grant", "Redirect URI mismatch"));
                }
            }

            // Validate PKCE
            if let Some(ref code_challenge) = pending.code_challenge {
                if !verify_pkce(
                    code_verifier,
                    code_challenge,
                    pending.code_challenge_method.as_deref(),
                ) {
                    warn!("[OAuth] PKCE verification failed");
                    return Err(token_error("invalid_grant", "PKCE verification failed"));
                }
            }

            // Get JWT secret
            let gateway_state = state.read().await;
            let Some(secret) = gateway_state.get_jwt_secret() else {
                warn!("[OAuth] JWT secret not configured");
                return Err(token_error(
                    "server_error",
                    "Server not properly configured",
                ));
            };

            // Issue tokens
            let scope = pending.scope.as_deref();
            let access_token = create_access_token(&pending.client_id, scope, 3600, secret);
            let refresh_token = create_refresh_token(&pending.client_id, scope, secret);
            let client_id_for_tracking = pending.client_id.clone();
            drop(gateway_state);

            // Track that this client has active tokens and emit event
            {
                let mut gateway_state = state.write().await;
                gateway_state
                    .clients_with_tokens
                    .insert(client_id_for_tracking.clone());

                // Update last_seen in database
                if let Some(repo) = gateway_state.inbound_client_repository() {
                    if let Err(e) = repo.update_client_last_seen(&client_id_for_tracking).await {
                        warn!("[OAuth] Failed to update last_seen: {}", e);
                    }
                }

                // Emit domain event for token issued
                use mcpmux_core::DomainEvent;
                info!(
                    "[OAuth] Emitting token issued event for: {}",
                    client_id_for_tracking
                );
                gateway_state.emit_domain_event(DomainEvent::ClientTokenIssued {
                    client_id: client_id_for_tracking.clone(),
                });
            }

            info!(
                "[OAuth] Issued tokens for client: {} (expires_in=3600s)",
                client_id_for_tracking
            );

            Ok(Json(TokenResponseBody {
                access_token,
                token_type: "Bearer".to_string(),
                expires_in: 3600,
                refresh_token: Some(refresh_token),
                scope: pending.scope,
            }))
        }
        "refresh_token" => {
            let Some(refresh_token) = request.refresh_token.as_ref() else {
                warn!("[OAuth] Missing refresh_token");
                return Err(token_error("invalid_request", "Missing refresh_token"));
            };

            // Get JWT secret and validate refresh token
            let gateway_state = state.read().await;
            let Some(secret) = gateway_state.get_jwt_secret() else {
                return Err(token_error(
                    "server_error",
                    "Server not properly configured",
                ));
            };

            // Validate the refresh token
            let Some(claims) = crate::auth::validate_token(refresh_token, secret) else {
                warn!("[OAuth] Invalid or expired refresh token");
                return Err(token_error(
                    "invalid_grant",
                    "Refresh token is invalid or expired",
                ));
            };

            // Verify client still exists in DB before issuing new tokens.
            // The JWT may be valid (same secret) but the client may have been
            // removed (e.g., DB was reset). Without this check, the middleware
            // would fail with "Client not found" after we issue a new token.
            if let Some(repo) = gateway_state.inbound_client_repository() {
                match repo.get_client(&claims.client_id).await {
                    Ok(Some(_)) => {
                        // Client exists, update last_seen
                        if let Err(e) = repo.update_client_last_seen(&claims.client_id).await {
                            warn!("[OAuth] Failed to update last_seen: {}", e);
                        }
                    }
                    Ok(None) => {
                        warn!(
                            "[OAuth] Client {} not found in DB during refresh",
                            claims.client_id
                        );
                        return Err(token_error("invalid_grant", "Client no longer registered"));
                    }
                    Err(e) => {
                        warn!(
                            "[OAuth] Failed to look up client {}: {}",
                            claims.client_id, e
                        );
                        return Err(token_error("server_error", "Database error"));
                    }
                }
            }

            // Issue new access token
            let access_token =
                create_access_token(&claims.client_id, claims.scope.as_deref(), 3600, secret);

            info!("[OAuth] Refreshed tokens for client: {}", claims.client_id);

            Ok(Json(TokenResponseBody {
                access_token,
                token_type: "Bearer".to_string(),
                expires_in: 3600,
                refresh_token: Some(refresh_token.clone()), // Return same refresh token
                scope: claims.scope,
            }))
        }
        _ => {
            warn!("[OAuth] Unsupported grant_type: {}", request.grant_type);
            Err(token_error(
                "unsupported_grant_type",
                "Only authorization_code and refresh_token are supported",
            ))
        }
    }
}

/// Helper to create token error response
fn token_error(error: &str, description: &str) -> (StatusCode, Json<TokenErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(TokenErrorResponse {
            error: error.to_string(),
            error_description: Some(description.to_string()),
        }),
    )
}

/// Verify PKCE code_verifier against code_challenge
fn verify_pkce(code_verifier: &str, code_challenge: &str, method: Option<&str>) -> bool {
    match method.unwrap_or("S256") {
        "S256" => {
            use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
            use sha2::{Digest, Sha256};

            let mut hasher = Sha256::new();
            hasher.update(code_verifier.as_bytes());
            let hash = hasher.finalize();
            let computed_challenge = URL_SAFE_NO_PAD.encode(hash);

            computed_challenge == code_challenge
        }
        "plain" => code_verifier == code_challenge,
        _ => false,
    }
}

// ============================================================================
// OAuth Consent Approval (called by McpMux app after user approval)
// ============================================================================

/// Request body for consent approval
/// Future feature - not yet routed
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ConsentApprovalRequest {
    /// The request_id from the pending authorization
    pub request_id: String,
    /// Whether the user approved the request
    pub approved: bool,
    /// Optional alias name for the client
    #[serde(default)]
    pub client_alias: Option<String>,
}

/// Response from consent approval
/// Future feature - not yet routed
#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct ConsentApprovalResponse {
    /// Whether the approval was successful
    pub success: bool,
    /// The redirect URL for the client (with code or error)
    pub redirect_url: String,
    /// Optional error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Approve or deny an OAuth consent request
///
/// This endpoint is called by the McpMux desktop app after the user has reviewed
/// and approved (or denied) an OAuth authorization request.
/// Future feature - not yet routed
#[allow(dead_code)]
pub async fn oauth_consent_approve(
    State(state): State<Arc<RwLock<GatewayState>>>,
    Json(request): Json<ConsentApprovalRequest>,
) -> impl IntoResponse {
    info!(
        "[OAuth] Consent {} for request_id: {}",
        if request.approved {
            "approved"
        } else {
            "denied"
        },
        request.request_id
    );

    // Look up the pending authorization
    let pending = {
        let gateway_state = state.read().await;
        gateway_state
            .pending_authorizations
            .get(&request.request_id)
            .cloned()
    };

    let Some(pending) = pending else {
        warn!("[OAuth] Consent approval failed: request_id not found");
        return Json(ConsentApprovalResponse {
            success: false,
            redirect_url: String::new(),
            error: Some("Authorization request not found or expired".to_string()),
        });
    };

    // Remove the pending authorization (it's been processed)
    {
        let mut gateway_state = state.write().await;
        gateway_state
            .pending_authorizations
            .remove(&request.request_id);
    }

    if !request.approved {
        // User denied - redirect with error
        let mut redirect_url = pending.redirect_uri.clone();
        redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
        redirect_url.push_str("error=access_denied&error_description=User+denied+the+request");
        if let Some(ref state_param) = pending.state {
            redirect_url.push_str(&format!("&state={}", urlencoding::encode(state_param)));
        }

        info!(
            "[OAuth] User denied consent for client: {}",
            pending.client_id
        );
        return Json(ConsentApprovalResponse {
            success: true,
            redirect_url,
            error: None,
        });
    }

    // User approved - generate authorization code
    let code = format!("mc_{}", uuid::Uuid::new_v4().to_string().replace("-", ""));

    // Auth codes expire in 10 minutes (standard OAuth)
    let code_expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 + 600) // 10 minutes
        .unwrap_or(i64::MAX);

    // Store the authorization with the new code
    {
        let mut gateway_state = state.write().await;
        gateway_state.store_pending_authorization(
            &code,
            PendingAuthorization {
                client_id: pending.client_id.clone(),
                client_name: pending.client_name.clone(),
                redirect_uri: pending.redirect_uri.clone(),
                scope: pending.scope.clone(),
                state: pending.state.clone(),
                code_challenge: pending.code_challenge.clone(),
                code_challenge_method: pending.code_challenge_method.clone(),
                expires_at: code_expires_at,
            },
        );

        // Store client alias in database if provided
        if let Some(alias) = &request.client_alias {
            if let Some(repo) = gateway_state.inbound_client_repository() {
                // Get current client, update alias, and save back
                if let Ok(Some(mut client)) = repo.get_client(&pending.client_id).await {
                    client.client_alias = Some(alias.clone());
                    if let Err(e) = repo.save_client(&client).await {
                        warn!("[OAuth] Failed to save client alias: {}", e);
                    } else {
                        info!(
                            "[OAuth] Set client alias '{}' for client_id: {}",
                            alias, pending.client_id
                        );
                    }
                }
            }
        }
    }

    // Build redirect URL with authorization code
    let mut redirect_url = pending.redirect_uri.clone();
    redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
    redirect_url.push_str(&format!("code={}", code));
    if let Some(ref state_param) = pending.state {
        redirect_url.push_str(&format!("&state={}", urlencoding::encode(state_param)));
    }

    info!(
        "[OAuth] Authorization approved for client: {}, issuing code",
        pending.client_id
    );

    Json(ConsentApprovalResponse {
        success: true,
        redirect_url,
        error: None,
    })
}

// ============================================================================
// OAuth Clients List (for McpMux app to display connected clients)
// ============================================================================

/// Response for OAuth client info
/// OAuth client information response
#[derive(Debug, Serialize)]
pub struct OAuthClientInfoResponse {
    pub client_id: String,
    pub registration_type: String,
    pub client_name: String,
    pub client_alias: Option<String>,
    pub redirect_uris: Vec<String>,
    pub scope: Option<String>,

    // RFC 7591 Client Metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,

    // CIMD-specific fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_cached_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_cache_ttl: Option<i64>,

    // MCP client preferences
    pub connection_mode: String,
    pub locked_space_id: Option<String>,
    pub last_seen: Option<String>,
    pub created_at: String,
    pub has_active_tokens: bool,
}

/// List all registered OAuth clients
/// List all registered OAuth clients
pub async fn oauth_list_clients(
    State(state): State<Arc<RwLock<GatewayState>>>,
) -> impl IntoResponse {
    let gateway_state = state.read().await;

    // Get clients from database (required)
    let Some(repo) = gateway_state.inbound_client_repository() else {
        warn!("[OAuth] Database not available for listing clients");
        return Json(vec![]); // Return empty list if database unavailable
    };

    match repo.list_clients().await {
        Ok(db_clients) => {
            let clients: Vec<OAuthClientInfoResponse> = db_clients
                .into_iter()
                .map(|c| {
                    let has_active = gateway_state.clients_with_tokens.contains(&c.client_id);
                    OAuthClientInfoResponse {
                        client_id: c.client_id,
                        registration_type: c.registration_type.as_str().to_string(),
                        client_name: c.client_name,
                        client_alias: c.client_alias,
                        redirect_uris: c.redirect_uris,
                        scope: c.scope,
                        logo_uri: c.logo_uri,
                        client_uri: c.client_uri,
                        software_id: c.software_id,
                        software_version: c.software_version,
                        metadata_url: c.metadata_url,
                        metadata_cached_at: c.metadata_cached_at,
                        metadata_cache_ttl: c.metadata_cache_ttl,
                        connection_mode: c.connection_mode,
                        locked_space_id: c.locked_space_id,
                        last_seen: c.last_seen,
                        created_at: c.created_at,
                        has_active_tokens: has_active,
                    }
                })
                .collect();
            info!("[OAuth] Listed {} clients from database", clients.len());
            Json(clients)
        }
        Err(e) => {
            error!("[OAuth] Failed to list clients from database: {}", e);
            Json(vec![]) // Return empty list on error
        }
    }
}

/// Request body for updating client settings
#[derive(Debug, Deserialize)]
pub struct UpdateClientRequest {
    pub client_alias: Option<String>,
    pub connection_mode: Option<String>,
    pub locked_space_id: Option<String>,
}

/// Update client settings (connection mode, alias, etc.)
/// Get resolved features (tools/prompts/resources) for a client
///
/// DIP: Thin handler that orchestrates services
/// Get resolved features (tools/prompts/resources) for a client
///
/// Supports both DCR and CIMD clients. CIMD client_ids (URLs) should be URL-encoded.
/// Axum automatically URL-decodes path parameters.
pub async fn oauth_get_client_features(
    State(state): State<AppState>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> Response {
    info!(
        "[OAuth] Getting resolved features for client: {}",
        client_id
    );

    // Step 1: Resolve space for client (SRP: SpaceResolverService)
    let space_id = match state
        .services
        .space_resolver_service
        .resolve_space_for_client(&client_id)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            warn!(
                "[OAuth] Failed to resolve space for client {}: {}",
                client_id, e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "space_resolution_failed",
                    "error_description": format!("Failed to resolve space: {}", e)
                })),
            )
                .into_response();
        }
    };

    debug!(
        "[OAuth] Resolved space {} for client {}",
        space_id, client_id
    );

    // Step 2: Get client grants (SRP: AuthorizationService)
    let feature_set_ids = match state
        .services
        .authorization_service
        .get_client_grants(&client_id, &space_id)
        .await
    {
        Ok(grants) => grants,
        Err(e) => {
            warn!(
                "[OAuth] Failed to get grants for client {}: {}",
                client_id, e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "authorization_failed",
                    "error_description": format!("Failed to get grants: {}", e)
                })),
            )
                .into_response();
        }
    };

    debug!(
        "[OAuth] Client {} has {} grants",
        client_id,
        feature_set_ids.len()
    );

    // Step 3: Resolve features (SRP: FeatureService)
    let space_id_str = space_id.to_string();

    let tools = state
        .services
        .pool_services
        .feature_service
        .get_tools_for_grants(&space_id_str, &feature_set_ids)
        .await
        .unwrap_or_default();

    let prompts = state
        .services
        .pool_services
        .feature_service
        .get_prompts_for_grants(&space_id_str, &feature_set_ids)
        .await
        .unwrap_or_default();

    let resources = state
        .services
        .pool_services
        .feature_service
        .get_resources_for_grants(&space_id_str, &feature_set_ids)
        .await
        .unwrap_or_default();

    info!(
        "[OAuth] Client {} features: {} tools, {} prompts, {} resources",
        client_id,
        tools.len(),
        prompts.len(),
        resources.len()
    );

    // Convert to response format
    let tools_response: Vec<_> = tools
        .iter()
        .map(|f| {
            json!({
                "name": f.feature_name,
                "description": f.description,
                "server_id": f.server_id,
            })
        })
        .collect();

    let prompts_response: Vec<_> = prompts
        .iter()
        .map(|f| {
            json!({
                "name": f.feature_name,
                "description": f.description,
                "server_id": f.server_id,
            })
        })
        .collect();

    let resources_response: Vec<_> = resources
        .iter()
        .map(|f| {
            json!({
                "name": f.feature_name,
                "description": f.description,
                "server_id": f.server_id,
            })
        })
        .collect();

    Json(json!({
        "space_id": space_id_str,
        "feature_set_ids": feature_set_ids,
        "tools": tools_response,
        "prompts": prompts_response,
        "resources": resources_response,
    }))
    .into_response()
}

/// Update client settings (connection mode, alias, etc.)
///
/// Supports both DCR and CIMD clients. CIMD client_ids (URLs) should be URL-encoded.
pub async fn oauth_update_client(
    State(state): State<Arc<RwLock<GatewayState>>>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Json(req): Json<UpdateClientRequest>,
) -> Response {
    info!("[OAuth] Updating client settings: {}", client_id);

    let gateway_state = state.read().await;

    let Some(repo) = gateway_state.inbound_client_repository() else {
        warn!("[OAuth] Database not available for client update");
        return (StatusCode::SERVICE_UNAVAILABLE, "Database not available").into_response();
    };

    // Validate connection_mode if provided
    if let Some(ref mode) = req.connection_mode {
        if !["follow_active", "locked", "ask_on_change"].contains(&mode.as_str()) {
            return (StatusCode::BAD_REQUEST, "Invalid connection_mode").into_response();
        }
    }

    // Handle locked_space_id: convert to Option<Option<String>>
    let locked_space_id = if req.connection_mode.as_deref() == Some("locked") {
        Some(req.locked_space_id.clone())
    } else if req.connection_mode.as_deref() == Some("follow_active")
        || req.connection_mode.as_deref() == Some("ask_on_change")
    {
        // Clear locked_space_id when switching away from locked mode
        Some(None)
    } else {
        // Don't change if not explicitly setting mode
        None
    };

    match repo
        .update_client_settings(
            &client_id,
            req.client_alias,
            req.connection_mode,
            locked_space_id,
        )
        .await
    {
        Ok(Some(client)) => {
            let has_active = gateway_state
                .clients_with_tokens
                .contains(&client.client_id);
            let response = OAuthClientInfoResponse {
                client_id: client.client_id,
                registration_type: client.registration_type.as_str().to_string(),
                client_name: client.client_name,
                client_alias: client.client_alias,
                redirect_uris: client.redirect_uris,
                scope: client.scope,
                logo_uri: client.logo_uri,
                client_uri: client.client_uri,
                software_id: client.software_id,
                software_version: client.software_version,
                metadata_url: client.metadata_url,
                metadata_cached_at: client.metadata_cached_at,
                metadata_cache_ttl: client.metadata_cache_ttl,
                connection_mode: client.connection_mode,
                locked_space_id: client.locked_space_id,
                last_seen: client.last_seen,
                created_at: client.created_at,
                has_active_tokens: has_active,
            };
            info!("[OAuth] Client updated: {}", response.client_id);
            Json(response).into_response()
        }
        Ok(None) => {
            warn!("[OAuth] Client not found: {}", client_id);
            (StatusCode::NOT_FOUND, "Client not found").into_response()
        }
        Err(e) => {
            warn!("[OAuth] Failed to update client: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update client: {}", e),
            )
                .into_response()
        }
    }
}

/// Delete a client and revoke all tokens
/// Delete a client and revoke all tokens
///
/// Supports both DCR and CIMD clients. CIMD client_ids (URLs) should be URL-encoded.
pub async fn oauth_delete_client(
    State(state): State<Arc<RwLock<GatewayState>>>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> Response {
    info!("[OAuth] Deleting client: {}", client_id);

    let mut gateway_state = state.write().await;

    let Some(repo) = gateway_state.inbound_client_repository() else {
        warn!("[OAuth] Database not available for client deletion");
        return (StatusCode::SERVICE_UNAVAILABLE, "Database not available").into_response();
    };

    match repo.delete_client(&client_id).await {
        Ok(true) => {
            // Remove from active tokens set
            gateway_state.clients_with_tokens.remove(&client_id);
            info!("[OAuth] Client deleted: {}", client_id);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => {
            warn!("[OAuth] Client not found: {}", client_id);
            (StatusCode::NOT_FOUND, "Client not found").into_response()
        }
        Err(e) => {
            warn!("[OAuth] Failed to delete client: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete client: {}", e),
            )
                .into_response()
        }
    }
}

// ============================================================================
// Dynamic Client Registration (RFC 7591)
// ============================================================================

/// POST /oauth/register - Dynamic Client Registration endpoint
pub async fn oauth_register(
    State(state): State<Arc<RwLock<GatewayState>>>,
    Json(request): Json<DcrRequest>,
) -> Result<Json<DcrResponse>, (StatusCode, Json<DcrError>)> {
    info!(
        "[DCR] Registration request from: {} (redirect_uris: {:?})",
        request.client_name, request.redirect_uris
    );

    let gateway_state = state.read().await;

    // Get database repository (required for DCR)
    let repo = gateway_state.inbound_client_repository().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DcrError::invalid_client_metadata("Database not available")),
        )
    })?;

    // Process DCR request (saves to database)
    match process_dcr_request(repo, request).await {
        Ok(response) => {
            info!(
                "[DCR] Successfully registered client: {} ({})",
                response.client_name, response.client_id
            );
            Ok(Json(response))
        }
        Err(error) => {
            warn!(
                "[DCR] Registration failed: {} - {:?}",
                error.error, error.error_description
            );
            Err((StatusCode::BAD_REQUEST, Json(error)))
        }
    }
}
