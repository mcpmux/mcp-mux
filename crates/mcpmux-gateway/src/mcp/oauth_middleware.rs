//! OAuth Middleware for rmcp Integration
//!
//! This middleware extracts OAuth Bearer tokens, verifies JWTs, resolves spaces,
//! and injects OAuthContext into request extensions for use by ServerHandler.
//!
//! Uses TraceContext from logging_middleware for request correlation.

use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::auth::validate_token;
use crate::logging::TraceContext;
use crate::server::ServiceContainer;

/// Synthetic client identity used when system-wide inbound auth is disabled and
/// a connection arrives without a (valid) Bearer token. Routing still prefers
/// the `X-Mcpmux-Workspace` header → binding; this id only feeds the rootless
/// `client_grants` fallback (which finds none) → Space default.
const ANONYMOUS_CLIENT_ID: &str = "mcpmux-anonymous";

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

    let base_url = {
        let state = services.gateway_state.read().await;
        state.base_url.clone()
    };

    // System-wide inbound auth can be disabled (localhost-only convenience):
    // when off, a connection is accepted without a Bearer token and routed by
    // the workspace header / default space. A valid token is still honored when
    // present, so flipping the setting never breaks an already-configured
    // client. Default is auth-required.
    let require_auth = !services.gateway_state.read().await.auth_disabled();

    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let token = auth_header
        .as_deref()
        .and_then(|v| v.strip_prefix("Bearer "));

    // Verify the Bearer token whenever one is present.
    let claims = match token {
        Some(token) => {
            let jwt_secret = {
                let state = services.gateway_state.read().await;
                state.get_jwt_secret().map(|s| s.to_vec())
            };
            match jwt_secret {
                Some(secret) => validate_token(token, &secret),
                None => {
                    warn!(trace_id = %trace_id, "JWT secret not configured");
                    None
                }
            }
        }
        None => None,
    };

    // Resolve (client_id, space_id) from the token, or — when auth is disabled
    // — fall back to an anonymous identity on the default space.
    let (client_id, space_id) = if let Some(claims) = claims {
        match services
            .space_resolver_service
            .resolve_space_for_client(&claims.client_id)
            .await
        {
            Ok(id) => (claims.client_id, id),
            Err(e) => {
                warn!(
                    trace_id = %trace_id,
                    client_id = %claims.client_id,
                    "Failed to resolve space: {}", e
                );
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to resolve space: {}", e),
                )
                    .into_response();
            }
        }
    } else if require_auth {
        // No valid token and auth is required → 401 with the specific reason.
        let msg = match auth_header.as_deref() {
            None => "Missing Authorization header",
            Some(v) if !v.starts_with("Bearer ") => "Authorization header must use Bearer scheme",
            _ => "Invalid token",
        };
        warn!(trace_id = %trace_id, "{}", msg);
        return unauthorized_response(&base_url, msg);
    } else {
        // Auth disabled → accept anonymously on the default space. Routing
        // still prefers the workspace header (pinned below) → binding.
        match services.dependencies.space_repo.get_default().await {
            Ok(Some(space)) => (ANONYMOUS_CLIENT_ID.to_string(), space.id),
            Ok(None) => {
                warn!(trace_id = %trace_id, "Auth disabled but no default space configured");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "No default space configured",
                )
                    .into_response();
            }
            Err(e) => {
                warn!(trace_id = %trace_id, "Failed to resolve default space: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to resolve default space: {}", e),
                )
                    .into_response();
            }
        }
    };

    // Inject OAuth context via custom headers (rmcp will preserve these)
    request.headers_mut().insert(
        "x-mcpmux-client-id",
        client_id.parse().expect("valid header value"),
    );
    request.headers_mut().insert(
        "x-mcpmux-space-id",
        space_id.to_string().parse().expect("valid header value"),
    );

    // Pin an explicit workspace root advertised by the client via the
    // `X-Mcpmux-Workspace` header (injected by McpMux's per-workspace client
    // configs). It shadows the client's MCP-reported roots in the resolver, so
    // a connection routes to its workspace binding even when the client never
    // reports `roots` or reports a stale one (e.g. Cursor sharing one MCP host
    // across windows). Unlike client/space id above, this header is
    // client-asserted — the same trust model as MCP roots: any approved local
    // client can claim any binding (see FeatureSetResolver trust model). Keyed
    // by the `mcp-session-id` the client echoes on every post-initialize
    // request (the same key the handler stores reported roots under).
    let pin = {
        let headers = request.headers();
        let sid = headers
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let ws = headers
            .get("x-mcpmux-workspace")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        sid.zip(ws)
    };
    if let Some((sid, ws)) = pin {
        services.session_roots.set_pinned(&sid, &ws);
    }

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
                    client = %&client_id[..client_id.len().min(12)],
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
                    format!("Failed to read request body: {}", e),
                )
                    .into_response();
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
            client = %client_id,
            method = mcp_method.as_deref().unwrap_or("-"),
            "← MCP error"
        );
    }

    response
}

/// Generate unauthorized response with RFC 9728 protected-resource discovery.
fn unauthorized_response(base_url: &str, message: &str) -> Response<Body> {
    let resource_metadata_url = format!(
        "{}/.well-known/oauth-protected-resource/mcp",
        base_url.trim_end_matches('/')
    );
    let www_authenticate = format!(
        r#"Bearer realm="McpMux Gateway", error="invalid_token", error_description="{}", resource_metadata="{}""#,
        message, resource_metadata_url
    );
    let body = serde_json::json!({
        "error": "invalid_token",
        "error_description": message,
        "resource_metadata": resource_metadata_url,
    });

    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, www_authenticate)],
        axum::Json(body),
    )
        .into_response()
}
