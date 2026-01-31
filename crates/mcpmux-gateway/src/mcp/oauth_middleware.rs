//! OAuth Middleware for rmcp Integration
//!
//! This middleware extracts OAuth Bearer tokens, verifies JWTs, resolves spaces,
//! and injects OAuthContext into request extensions for use by ServerHandler.
//!
//! Uses TraceContext from logging_middleware for request correlation.

use std::sync::Arc;
use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use tracing::{debug, info, warn};

use crate::auth::validate_token;
use crate::logging::TraceContext;
use crate::server::ServiceContainer;

/// OAuth middleware for MCP endpoints using rmcp
///
/// Extracts Bearer token → Verifies JWT → Resolves space → Injects OAuthContext
pub async fn mcp_oauth_middleware(
    axum::extract::State(services): axum::extract::State<Arc<ServiceContainer>>,
    mut request: Request<Body>,
    next: Next,
) -> Response<Body> {
    // Skip auth for OPTIONS (CORS preflight)
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }

    // Get or create trace context from upstream middleware
    let trace_id = request
        .extensions()
        .get::<TraceContext>()
        .map(|ctx| ctx.trace_id.clone())
        .unwrap_or_else(|| "??????".to_string());

    // Extract Authorization header
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let Some(auth_value) = auth_header else {
        warn!(trace_id = %trace_id, "Missing Authorization header");
        return unauthorized_response("Missing Authorization header");
    };

    // Extract Bearer token
    let token = match auth_value.strip_prefix("Bearer ") {
        Some(t) => t,
        None => {
            warn!(trace_id = %trace_id, "Authorization header must use Bearer scheme");
            return unauthorized_response("Authorization header must use Bearer scheme");
        }
    };

    // Verify JWT and extract claims
    let jwt_secret = {
        let state = services.gateway_state.read().await;
        match state.get_jwt_secret() {
            Some(secret) => secret.to_vec(),
            None => {
                warn!(trace_id = %trace_id, "JWT secret not configured");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Server not configured for authentication"
                ).into_response();
            }
        }
    };

    let claims = match validate_token(token, &jwt_secret) {
        Some(claims) => claims,
        None => {
            warn!(trace_id = %trace_id, "Token verification failed");
            return unauthorized_response("Invalid token");
        }
    };

    // Resolve space for this client
    let space_id = match services.space_resolver_service
        .resolve_space_for_client(&claims.client_id)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            warn!(
                trace_id = %trace_id,
                client_id = %claims.client_id,
                "Failed to resolve space: {}", e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to resolve space: {}", e)
            ).into_response();
        }
    };

    // Inject OAuth context via custom headers (rmcp will preserve these)
    request.headers_mut().insert(
        "x-mcmux-client-id",
        claims.client_id.parse().expect("valid header value"),
    );
    request.headers_mut().insert(
        "x-mcmux-space-id",
        space_id.to_string().parse().expect("valid header value"),
    );

    // Extract MCP method from body if POST
    let mcp_method = if request.method() == axum::http::Method::POST {
        use axum::body::to_bytes;
        
        let (parts, body) = request.into_parts();
        
        match to_bytes(body, usize::MAX).await {
            Ok(body_bytes) => {
                let method = crate::server::logging_middleware::extract_mcp_method(&body_bytes);
                
                // Log single consolidated entry line
                info!(
                    trace_id = %trace_id,
                    client = %&claims.client_id[..claims.client_id.len().min(12)],
                    space = %&space_id.to_string()[..8],
                    method = method.as_deref().unwrap_or("-"),
                    "→ MCP"
                );
                
                // Reconstruct the request
                request = axum::http::Request::from_parts(parts, Body::from(body_bytes));
                method
            }
            Err(e) => {
                warn!(trace_id = %trace_id, "Failed to read body: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to read request body: {}", e)
                ).into_response();
            }
        }
    } else {
        // GET request (SSE stream)
        debug!(trace_id = %trace_id, "SSE stream request");
        None
    };
    
    let response = next.run(request).await;
    
    // Log errors only
    let status = response.status();
    if status.is_server_error() || status.is_client_error() {
        warn!(
            trace_id = %trace_id,
            status = %status,
            client = %claims.client_id,
            method = mcp_method.as_deref().unwrap_or("-"),
            "← MCP error"
        );
    }
    
    response
}

/// Generate unauthorized response
fn unauthorized_response(message: &str) -> Response<Body> {
    (
        StatusCode::UNAUTHORIZED,
        [(
            "WWW-Authenticate",
            r#"Bearer realm="MCMux Gateway", error="invalid_token""#,
        )],
        message.to_string(),
    )
        .into_response()
}




