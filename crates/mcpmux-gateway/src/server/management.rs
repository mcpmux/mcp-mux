//! Authenticated management API for the headless gateway.
//!
//! A distinct, bearer-token-gated router mounted under `/admin/api/`, separate
//! from the MCP + OAuth surface, so a headless `mcpmux serve` can be inspected
//! and managed over HTTP (the desktop app manages via Tauri IPC instead). It
//! covers the core web-admin loop: read Spaces / servers / FeatureSets /
//! bindings / clients, create a Space, a FeatureSet, and a workspace mapping,
//! delete a mapping, and subscribe to live change events over SSE.
//!
//! Auth: every `/admin/api/*` route requires `Authorization: Bearer <token>`,
//! compared in constant time. On a network bind this is the ONLY gate, so the
//! token must be strong (the serve binary generates 256 bits when the operator
//! doesn't supply one). The `/admin` console page is public (its data calls
//! carry the operator-entered token).

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{delete, get},
    Router,
};
use futures::Stream;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use mcpmux_core::{DomainEvent, FeatureSet, Space, WorkspaceBinding};

use super::handlers::AppState;

/// The expected admin bearer token, carried into the auth middleware.
#[derive(Clone)]
pub struct AdminToken(pub Arc<String>);

/// Constant-time string comparison (no length-independent early-out).
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

/// Extract a `token=` value from a URL query string (minimal, no deps).
///
/// The value is percent-decoded: both web clients build the URL with
/// `encodeURIComponent(token)`, so a token containing `+`, `=`, `%`, … arrives
/// encoded and must be decoded before the constant-time compare — otherwise
/// header auth works but SSE auth 401s for the same token.
fn token_from_query(query: Option<&str>) -> Option<String> {
    let q = query?;
    for pair in q.split('&') {
        if let Some(v) = pair.strip_prefix("token=") {
            return Some(percent_decode(v));
        }
    }
    None
}

/// Minimal `%XX` percent-decoding. No `+`-as-space: `encodeURIComponent`
/// never emits `+`, and a literal `+` in a token must survive round-tripping.
/// Malformed escapes pass through unchanged.
fn percent_decode(s: &str) -> String {
    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Bearer-token gate for `/admin/api/*`. Rejected requests never reach a
/// handler (no data is read or written without a valid token). Accepts the
/// token via `Authorization: Bearer` OR a `?token=` query param — the latter
/// solely because the SSE `EventSource` API can't set request headers. On a
/// public deploy the console is fronted by TLS so the URL isn't observable,
/// and our request logging skips streaming responses.
async fn require_admin_token(
    State(expected): State<AdminToken>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let header_token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let presented = header_token
        .or_else(|| token_from_query(request.uri().query()))
        .unwrap_or_default();
    if !presented.is_empty() && tokens_match(&presented, &expected.0) {
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "admin authentication required" })),
    )
        .into_response()
}

fn internal_error(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": msg })),
    )
        .into_response()
}

fn bad_request(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
}

// ---------------------------------------------------------------------------
// Shared helpers (REST + RPC mirror) — parity with the desktop commands
// ---------------------------------------------------------------------------

/// Serialize an [`mcpmux_core::InstalledServer`] with its decrypted secrets
/// masked. `input_values`, `env_overrides` and `extra_headers` hold user
/// credentials (API keys, tokens, custom auth headers). The desktop reads the
/// full struct over loopback IPC only; the management API can be reached over
/// the network, where the at-rest field encryption must not be undone by a
/// list endpoint. Keys survive (the UI can show *what* is configured), values
/// are masked.
pub(crate) fn redacted_server_json(server: &mcpmux_core::InstalledServer) -> serde_json::Value {
    let mut v = serde_json::to_value(server).unwrap_or_else(|_| json!({}));
    for field in ["input_values", "env_overrides", "extra_headers"] {
        if let Some(map) = v.get_mut(field).and_then(|m| m.as_object_mut()) {
            for val in map.values_mut() {
                *val = json!("•••");
            }
        }
    }
    v
}

/// Create a Space with the desktop's `SpaceService::create` semantics — the
/// first Space becomes the default and builtin FeatureSets (Starter, …) are
/// seeded — so headless writes keep the resolver's default-Space/Starter
/// fallback intact. Emits `SpaceCreated`.
pub(crate) async fn create_space_with_builtins(
    app_state: &AppState,
    name: &str,
    icon: Option<&str>,
    description: Option<&str>,
) -> Result<Space, String> {
    let deps = &app_state.services.dependencies;
    let service = mcpmux_core::SpaceService::with_feature_set_repository(
        deps.space_repo.clone(),
        deps.feature_set_repo.clone(),
    );
    let mut space = service
        .create(name.to_string(), icon.map(str::to_string))
        .await
        .map_err(|e| e.to_string())?;
    if let Some(desc) = description {
        // The service constructor has no description param; persist it as a
        // follow-up update so the REST shape keeps working.
        space = space.with_description(desc);
        deps.space_repo
            .update(&space)
            .await
            .map_err(|e| e.to_string())?;
    }
    app_state
        .gateway_state
        .read()
        .await
        .emit_domain_event(DomainEvent::SpaceCreated {
            space_id: space.id,
            name: space.name.clone(),
            icon: space.icon.clone(),
        });
    Ok(space)
}

/// Delete a Space through the same service the desktop uses, which refuses to
/// delete the default Space (the resolver's fallback for unmapped sessions).
/// Emits `SpaceDeleted`.
pub(crate) async fn delete_space_guarded(app_state: &AppState, id: &Uuid) -> Result<(), String> {
    let deps = &app_state.services.dependencies;
    mcpmux_core::SpaceService::new(deps.space_repo.clone())
        .delete(id)
        .await
        .map_err(|e| e.to_string())?;
    app_state
        .gateway_state
        .read()
        .await
        .emit_domain_event(DomainEvent::SpaceDeleted { space_id: *id });
    Ok(())
}

/// Resolve a binding input's storage key with the desktop command's rules:
/// `path` bindings are normalized + validated (relative paths, filesystem
/// roots, reserved characters rejected — a binding the resolver could never
/// match must not be storable); `id` bindings take any non-empty trimmed key.
pub(crate) fn binding_key_for(raw_root: &str, is_id: bool) -> Result<String, String> {
    if is_id {
        let key = raw_root.trim();
        if key.is_empty() {
            return Err("Mapping id cannot be empty".into());
        }
        return Ok(key.to_string());
    }
    match mcpmux_core::validate_workspace_root(raw_root) {
        mcpmux_core::WorkspaceRootValidation::Ok { normalized } => Ok(normalized),
        mcpmux_core::WorkspaceRootValidation::Empty => Err("workspace_root cannot be empty".into()),
        mcpmux_core::WorkspaceRootValidation::Invalid { reason } => Err(reason),
    }
}

/// Reject a duplicate mapping key with a readable message (the schema's
/// `UNIQUE(workspace_root)` would otherwise surface an opaque SQLite error).
pub(crate) async fn ensure_binding_key_free(
    app_state: &AppState,
    key: &str,
    exclude: Option<Uuid>,
) -> Result<(), String> {
    let existing = app_state
        .services
        .dependencies
        .workspace_binding_repo
        .list()
        .await
        .map_err(|e| e.to_string())?;
    if existing
        .iter()
        .any(|b| Some(b.id) != exclude && b.workspace_root == key)
    {
        return Err(format!(
            "A mapping already exists for {key}. Edit the existing mapping instead of adding a second one."
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

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

/// `GET /admin/api/status` — lightweight runtime status (active sessions).
async fn admin_status(State(app_state): State<AppState>) -> Response {
    let state = app_state.gateway_state.read().await;
    Json(json!({
        "active_sessions": state.sessions.len(),
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

#[derive(Debug, Deserialize)]
struct SpaceIdQuery {
    space_id: Option<String>,
}

/// `GET /admin/api/servers?space_id=…` — installed servers for a Space
/// (defaults to the default Space when `space_id` is omitted).
async fn admin_list_servers(
    State(app_state): State<AppState>,
    Query(q): Query<SpaceIdQuery>,
) -> Response {
    let space_id = match resolve_space_id(&app_state, q.space_id).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    match app_state
        .services
        .dependencies
        .installed_server_repo
        .list_for_space(&space_id.to_string())
        .await
    {
        Ok(servers) => {
            let out: Vec<_> = servers.iter().map(redacted_server_json).collect();
            Json(json!({ "servers": out })).into_response()
        }
        Err(e) => internal_error(&e.to_string()),
    }
}

/// `GET /admin/api/feature-sets?space_id=…` — FeatureSets for a Space.
async fn admin_list_feature_sets(
    State(app_state): State<AppState>,
    Query(q): Query<SpaceIdQuery>,
) -> Response {
    let space_id = match resolve_space_id(&app_state, q.space_id).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    match app_state
        .services
        .dependencies
        .feature_set_repo
        .list_by_space(&space_id.to_string())
        .await
    {
        Ok(sets) => Json(json!({ "feature_sets": sets })).into_response(),
        Err(e) => internal_error(&e.to_string()),
    }
}

/// `GET /admin/api/bindings` — all workspace mappings.
async fn admin_list_bindings(State(app_state): State<AppState>) -> Response {
    match app_state
        .services
        .dependencies
        .workspace_binding_repo
        .list()
        .await
    {
        Ok(bindings) => Json(json!({ "bindings": bindings })).into_response(),
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

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreateSpaceRequest {
    name: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// `POST /admin/api/spaces` — create a Space (desktop `SpaceService` parity:
/// seeds builtin FeatureSets; first Space becomes the default).
async fn admin_create_space(
    State(app_state): State<AppState>,
    Json(req): Json<CreateSpaceRequest>,
) -> Response {
    let name = req.name.trim();
    if name.is_empty() {
        return bad_request("name is required");
    }
    let icon = req.icon.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let description = req
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match create_space_with_builtins(&app_state, name, icon, description).await {
        Ok(space) => (StatusCode::CREATED, Json(json!({ "space": space }))).into_response(),
        Err(e) => internal_error(&e),
    }
}

#[derive(Debug, Deserialize)]
struct CreateFeatureSetRequest {
    space_id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

/// `POST /admin/api/feature-sets` — create a custom FeatureSet in a Space.
async fn admin_create_feature_set(
    State(app_state): State<AppState>,
    Json(req): Json<CreateFeatureSetRequest>,
) -> Response {
    let name = req.name.trim();
    if name.is_empty() {
        return bad_request("name is required");
    }
    let space_uuid = match Uuid::parse_str(req.space_id.trim()) {
        Ok(u) => u,
        Err(_) => return bad_request("space_id must be a UUID"),
    };
    let mut fs = FeatureSet::new_custom(name, req.space_id.trim());
    fs.description = req
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    fs.icon = req
        .icon
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let fs_id = fs.id.clone();
    let fs_name = fs.name.clone();
    if let Err(e) = app_state
        .services
        .dependencies
        .feature_set_repo
        .create(&fs)
        .await
    {
        return internal_error(&e.to_string());
    }
    app_state
        .gateway_state
        .read()
        .await
        .emit_domain_event(DomainEvent::FeatureSetCreated {
            space_id: space_uuid,
            feature_set_id: fs_id,
            name: fs_name,
            feature_set_type: Some("custom".to_string()),
        });
    (StatusCode::CREATED, Json(json!({ "feature_set": fs }))).into_response()
}

#[derive(Debug, Deserialize)]
struct CreateBindingRequest {
    workspace_root: String,
    space_id: String,
    #[serde(default)]
    feature_set_ids: Vec<String>,
    /// "path" (default) or "id" (verbatim key match).
    #[serde(default)]
    binding_type: Option<String>,
}

/// `POST /admin/api/bindings` — create a workspace mapping. Path roots get the
/// same normalization + validation + duplicate pre-check the desktop command
/// applies (an unnormalized root would never match a reported workspace).
async fn admin_create_binding(
    State(app_state): State<AppState>,
    Json(req): Json<CreateBindingRequest>,
) -> Response {
    let space_uuid = match Uuid::parse_str(req.space_id.trim()) {
        Ok(u) => u,
        Err(_) => return bad_request("space_id must be a UUID"),
    };
    let is_id = req.binding_type.as_deref() == Some("id");
    let key = match binding_key_for(&req.workspace_root, is_id) {
        Ok(k) => k,
        Err(e) => return bad_request(&e),
    };
    if let Err(e) = ensure_binding_key_free(&app_state, &key, None).await {
        return bad_request(&e);
    }
    let binding = if is_id {
        WorkspaceBinding::new_id(key, space_uuid, req.feature_set_ids.clone())
    } else {
        WorkspaceBinding::new_multi(key, space_uuid, req.feature_set_ids.clone())
    };
    let workspace_root = binding.workspace_root.clone();
    if let Err(e) = app_state
        .services
        .dependencies
        .workspace_binding_repo
        .create(&binding)
        .await
    {
        return internal_error(&e.to_string());
    }
    app_state
        .gateway_state
        .read()
        .await
        .emit_domain_event(DomainEvent::WorkspaceBindingChanged {
            space_id: space_uuid,
            workspace_root,
        });
    (StatusCode::CREATED, Json(json!({ "binding": binding }))).into_response()
}

/// `DELETE /admin/api/bindings/:id` — remove a workspace mapping.
async fn admin_delete_binding(
    State(app_state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let binding_id = match Uuid::parse_str(id.trim()) {
        Ok(u) => u,
        Err(_) => return bad_request("binding id must be a UUID"),
    };
    let repo = &app_state.services.dependencies.workspace_binding_repo;
    // Look up first so we can emit a precise change event.
    let existing = repo.get(&binding_id).await.ok().flatten();
    if let Err(e) = repo.delete(&binding_id).await {
        return internal_error(&e.to_string());
    }
    if let Some(b) = existing {
        app_state.gateway_state.read().await.emit_domain_event(
            DomainEvent::WorkspaceBindingChanged {
                space_id: b.space_id,
                workspace_root: b.workspace_root,
            },
        );
    }
    (StatusCode::OK, Json(json!({ "deleted": true }))).into_response()
}

// ---------------------------------------------------------------------------
// Events (SSE)
// ---------------------------------------------------------------------------

/// Map a domain event to the UI event name the React app listens for (the same
/// grouped names the desktop's Tauri bridge emits), so the web admin's `listen`
/// subscriptions fire on SSE. Unmapped events fall back to a hyphenated
/// type name.
fn ui_event_name(event: &mcpmux_core::DomainEvent) -> String {
    use mcpmux_core::DomainEvent as E;
    match event {
        E::SpaceCreated { .. } | E::SpaceUpdated { .. } | E::SpaceDeleted { .. } => {
            "space-changed".into()
        }
        E::FeatureSetCreated { .. }
        | E::FeatureSetUpdated { .. }
        | E::FeatureSetDeleted { .. }
        | E::FeatureSetMembersChanged { .. } => "feature-set-changed".into(),
        E::WorkspaceBindingChanged { .. } => "workspace-binding-changed".into(),
        E::ClientRegistered { .. }
        | E::ClientReconnected { .. }
        | E::ClientUpdated { .. }
        | E::ClientDeleted { .. }
        | E::ClientTokenIssued { .. }
        | E::ClientGrantChanged { .. } => "client-changed".into(),
        // Must match the desktop Tauri bridge's name (gateway.rs
        // `map_domain_event_to_ui`) — the same React app listens on both
        // transports.
        E::ServerStatusChanged { .. } => "server-status-changed".into(),
        other => other.type_name().replace('_', "-"),
    }
}

/// `GET /admin/api/events` — server-sent stream of domain change events, so the
/// web admin re-fetches on change without polling. Each SSE `event:` is the UI
/// event name the app listens for; `data:` is the event JSON.
async fn admin_events(
    State(app_state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut rx = app_state
        .gateway_state
        .read()
        .await
        .subscribe_domain_events();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let name = ui_event_name(&event);
                    let data = serde_json::to_string(&event)
                        .unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event(name).data(data));
                }
                // Lagged: skip missed events and keep streaming.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                // Sender gone → end the stream.
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---------------------------------------------------------------------------
// Console page + router
// ---------------------------------------------------------------------------

/// Resolve a `space_id` query param to a UUID, defaulting to the default Space.
async fn resolve_space_id(app_state: &AppState, raw: Option<String>) -> Result<Uuid, Response> {
    if let Some(s) = raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Uuid::parse_str(s).map_err(|_| bad_request("space_id must be a UUID"));
    }
    match app_state
        .services
        .dependencies
        .space_repo
        .get_default()
        .await
    {
        Ok(Some(space)) => Ok(space.id),
        Ok(None) => Err(bad_request("no default Space; pass space_id")),
        Err(e) => Err(internal_error(&e.to_string())),
    }
}

/// `GET /admin` — the web admin console. Self-contained HTML (no external
/// assets) that signs in with the admin token and drives the read/write API.
async fn admin_console() -> Response {
    let html = include_str!("admin_console.html");
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

/// Build the management router: the token-gated `/admin/api/*` endpoints plus
/// the public `/admin` console page. Compose into the gateway (or the serve
/// binary) with the app state and the required admin token.
pub fn management_router(app_state: AppState, admin_token: Arc<String>) -> Router {
    let api = Router::new()
        .route("/admin/api/info", get(admin_info))
        .route("/admin/api/status", get(admin_status))
        // Command-mirror JSON-RPC — the desktop React UI served headless drives
        // this with the same command names/payloads it sends over Tauri IPC.
        .route(
            "/admin/api/rpc/{command}",
            axum::routing::post(super::management_rpc::rpc),
        )
        .route(
            "/admin/api/spaces",
            get(admin_list_spaces).post(admin_create_space),
        )
        .route("/admin/api/servers", get(admin_list_servers))
        .route(
            "/admin/api/feature-sets",
            get(admin_list_feature_sets).post(admin_create_feature_set),
        )
        .route(
            "/admin/api/bindings",
            get(admin_list_bindings).post(admin_create_binding),
        )
        .route("/admin/api/bindings/{id}", delete(admin_delete_binding))
        .route("/admin/api/clients", get(admin_list_clients))
        .route("/admin/api/events", get(admin_events))
        .with_state(app_state)
        .layer(axum::middleware::from_fn_with_state(
            AdminToken(admin_token),
            require_admin_token,
        ));
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

    #[test]
    fn token_from_query_extracts_the_param() {
        assert_eq!(token_from_query(Some("token=abc")), Some("abc".to_string()));
        assert_eq!(
            token_from_query(Some("foo=1&token=xyz&bar=2")),
            Some("xyz".to_string())
        );
        assert_eq!(token_from_query(Some("foo=1")), None);
        assert_eq!(token_from_query(None), None);
    }

    #[test]
    fn token_from_query_percent_decodes_the_value() {
        // encodeURIComponent('a+b=c%') === 'a%2Bb%3Dc%25' — the SSE clients
        // send the token through encodeURIComponent, so the gate must decode.
        assert_eq!(
            token_from_query(Some("token=a%2Bb%3Dc%25")),
            Some("a+b=c%".to_string())
        );
        // Malformed escapes pass through unchanged rather than panicking.
        assert_eq!(token_from_query(Some("token=a%2")), Some("a%2".to_string()));
        assert_eq!(
            token_from_query(Some("token=a%zz")),
            Some("a%zz".to_string())
        );
    }

    #[test]
    fn server_secrets_are_redacted_but_keys_survive() {
        let mut server = mcpmux_core::InstalledServer::new("space", "srv");
        server
            .input_values
            .insert("API_KEY".into(), "sk-secret".into());
        server.env_overrides.insert("TOKEN".into(), "t0ken".into());
        server
            .extra_headers
            .insert("Authorization".into(), "Bearer xyz".into());
        let v = redacted_server_json(&server);
        assert_eq!(v["input_values"]["API_KEY"], "•••");
        assert_eq!(v["env_overrides"]["TOKEN"], "•••");
        assert_eq!(v["extra_headers"]["Authorization"], "•••");
        let s = v.to_string();
        assert!(!s.contains("sk-secret") && !s.contains("t0ken") && !s.contains("Bearer xyz"));
    }
}
