//! Dynamic Client Registration (RFC 7591)
//!
//! Implements the OAuth 2.0 Dynamic Client Registration Protocol
//! for registering MCP clients with the gateway.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Dynamic Client Registration Request (RFC 7591)
#[derive(Debug, Clone, Deserialize)]
pub struct DcrRequest {
    /// Human-readable name of the client
    pub client_name: String,
    /// Array of allowed redirect URIs
    pub redirect_uris: Vec<String>,
    /// OAuth 2.0 grant types the client may use
    #[serde(default)]
    pub grant_types: Vec<String>,
    /// OAuth 2.0 response types the client may use
    #[serde(default)]
    pub response_types: Vec<String>,
    /// Authentication method for the token endpoint
    #[serde(default)]
    pub token_endpoint_auth_method: Option<String>,
    /// Scope values the client may request
    #[serde(default)]
    pub scope: Option<String>,

    // RFC 7591 Client Metadata
    /// URL for the client's logo
    #[serde(default)]
    pub logo_uri: Option<String>,
    /// URL of the client's homepage
    #[serde(default)]
    pub client_uri: Option<String>,
    /// URL for the client's terms of service
    #[serde(default)]
    pub tos_uri: Option<String>,
    /// URL for the client's privacy policy
    #[serde(default)]
    pub policy_uri: Option<String>,
    /// Contact email addresses
    #[serde(default)]
    pub contacts: Option<Vec<String>>,
    /// Unique identifier for the software (e.g., "com.cursor.app")
    #[serde(default)]
    pub software_id: Option<String>,
    /// Version of the client software
    #[serde(default)]
    pub software_version: Option<String>,
}

/// Dynamic Client Registration Response (RFC 7591)
#[derive(Debug, Clone, Serialize)]
pub struct DcrResponse {
    /// Unique client identifier
    pub client_id: String,
    /// Human-readable name of the client
    pub client_name: String,
    /// Array of allowed redirect URIs
    pub redirect_uris: Vec<String>,
    /// OAuth 2.0 grant types the client may use
    pub grant_types: Vec<String>,
    /// OAuth 2.0 response types the client may use
    pub response_types: Vec<String>,
    /// Authentication method for the token endpoint
    pub token_endpoint_auth_method: String,
    /// Timestamp of when the client was registered
    pub client_id_issued_at: u64,
    /// Scope values the client may request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    // RFC 7591 Client Metadata
    /// URL for the client's logo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,
    /// URL of the client's homepage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    /// URL for the client's terms of service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_uri: Option<String>,
    /// URL for the client's privacy policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_uri: Option<String>,
    /// Contact email addresses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contacts: Option<Vec<String>>,
    /// Unique identifier for the software
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,
    /// Version of the client software
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,
}

/// Helper to build InboundClient from DcrRequest
/// Eliminates ~100 lines of duplication between update and create paths
#[allow(clippy::too_many_arguments)]
fn build_inbound_client_from_request(
    request: &DcrRequest,
    client_id: String,
    redirect_uris: Vec<String>,
    grant_types: Vec<String>,
    response_types: Vec<String>,
    token_endpoint_auth_method: String,
    client_alias: Option<String>,
    last_seen: Option<String>,
    created_at: String,
    updated_at: String,
) -> mcpmux_storage::InboundClient {
    mcpmux_storage::InboundClient {
        client_id,
        registration_type: mcpmux_storage::RegistrationType::Dcr,
        client_name: request.client_name.clone(),
        client_alias,
        redirect_uris,
        grant_types,
        response_types,
        token_endpoint_auth_method,
        scope: request.scope.clone(),
        // Not approved until user explicitly consents
        approved: false,
        // RFC 7591 client metadata
        logo_uri: request.logo_uri.clone(),
        client_uri: request.client_uri.clone(),
        software_id: request.software_id.clone(),
        software_version: request.software_version.clone(),
        // CIMD fields (empty for DCR)
        metadata_url: None,
        metadata_cached_at: None,
        metadata_cache_ttl: None,
        last_seen,
        created_at,
        updated_at,
        // Capability bits default off / unknown; the gateway flips them
        // on the first `initialize` for any session of this client.
        reports_roots: false,
        roots_capability_known: false,
    }
}

/// DCR Error Response
#[derive(Debug, Clone, Serialize)]
pub struct DcrError {
    /// Error code
    pub error: String,
    /// Human-readable error description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

impl DcrError {
    pub fn invalid_redirect_uri(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_redirect_uri".to_string(),
            error_description: Some(description.into()),
        }
    }

    pub fn invalid_client_metadata(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_client_metadata".to_string(),
            error_description: Some(description.into()),
        }
    }
}

/// Check whether a requested redirect URI matches one in the registered list.
///
/// Per RFC 8252 §7.3, when the registered redirect URI is a loopback address
/// (`127.0.0.1`, `::1`, or `localhost`), the authorization server MUST ignore
/// the port component when matching — native public clients obtain an ephemeral
/// port from the OS at request time, so the port will differ between the DCR
/// registration and the `/authorize` request.
///
/// For non-loopback URIs (HTTPS, custom schemes like `cursor://`), strict
/// byte-exact equality is required.
pub fn is_redirect_uri_allowed(registered: &[String], requested: &str) -> bool {
    registered
        .iter()
        .any(|r| redirect_uri_matches(r, requested))
}

fn redirect_uri_matches(registered: &str, requested: &str) -> bool {
    if registered == requested {
        return true;
    }

    let (Ok(reg_url), Ok(req_url)) = (url::Url::parse(registered), url::Url::parse(requested))
    else {
        return false;
    };

    let is_loopback = |u: &url::Url| match u.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        None => false,
    };

    if !is_loopback(&reg_url) || !is_loopback(&req_url) {
        return false;
    }

    // Only the port may differ (RFC 8252 §7.3); everything else must match.
    reg_url.scheme() == req_url.scheme()
        && reg_url.host() == req_url.host()
        && reg_url.path() == req_url.path()
        && reg_url.query() == req_url.query()
        && req_url.fragment().is_none()
}

fn is_loopback_redirect_uri(uri: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };

    if url.scheme() != "http" {
        return false;
    }

    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

/// URI schemes that must never be accepted as a redirect target, even though
/// they are technically non-http "custom" schemes. Approving a consent hands
/// the redirect to the OS, so a redirect must not be able to run script, open
/// local or network files, or reach an OS or browser handler that can open or
/// run content (Windows Search, Office, Edge, Safari, ...).
const DANGEROUS_REDIRECT_SCHEMES: &[&str] = &[
    "javascript",
    "data",
    "vbscript",
    "file",
    "blob",
    "about",
    "view-source",
    "jar",
    "ftp",
    "sftp",
    "smb",
    "cifs",
    "nfs",
    "afp",
    "dav",
    "webdav",
    "ldap",
    "ldaps",
    "telnet",
    "ssh",
    "rdp",
    "vnc",
    "search",
    "search-ms",
    "shell",
    "mk",
    "its",
    "mhtml",
    "res",
    "hcp",
    "help",
    "intent",
    "itms-services",
    "chrome",
    "chrome-extension",
    "edge",
];

/// Scheme prefixes reserved for OS and browser handlers (`ms-word:`,
/// `microsoft-edge:`, `x-safari-https:`, ...).
const DANGEROUS_REDIRECT_SCHEME_PREFIXES: &[&str] =
    &["ms-", "microsoft-", "x-safari-", "x-apple-", "windows-"];

fn is_custom_scheme_redirect_uri(uri: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };

    // `url` normalizes the scheme to lowercase, so the denylist comparison is
    // case-insensitive (e.g. `JavaScript:` is parsed as `javascript`).
    let scheme = url.scheme();
    scheme != "http"
        && scheme != "https"
        // McpMux's own deep links would run app actions with the code attached.
        && !scheme.eq_ignore_ascii_case(mcpmux_core::branding::DEEP_LINK_SCHEME)
        && !DANGEROUS_REDIRECT_SCHEMES.contains(&scheme)
        && !DANGEROUS_REDIRECT_SCHEME_PREFIXES
            .iter()
            .any(|prefix| scheme.starts_with(prefix))
}

fn is_chatgpt_connector_redirect_uri(uri: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };

    url.scheme() == "https"
        && matches!(url.host(), Some(url::Host::Domain(host)) if host.eq_ignore_ascii_case("chatgpt.com"))
        && (url.path() == "/connector/oauth" || url.path().starts_with("/connector/oauth/"))
}

/// Whether `uri` is an acceptable OAuth redirect target for an inbound client
/// (loopback http, a non-dangerous custom scheme, or the ChatGPT connector
/// callback). Applies to DCR registrations, CIMD documents, and every
/// redirect the gateway or desktop app is about to perform.
pub fn is_valid_registered_redirect_uri(uri: &str) -> bool {
    // RFC 6749 §3.1.2: a redirection endpoint URI must not include a fragment.
    if url::Url::parse(uri).map_or(true, |u| u.fragment().is_some()) {
        return false;
    }
    is_loopback_redirect_uri(uri)
        || is_custom_scheme_redirect_uri(uri)
        || is_chatgpt_connector_redirect_uri(uri)
}

/// Keep only acceptable redirect URIs (see [`is_valid_registered_redirect_uri`]),
/// de-duplicated, in their original order.
pub fn filter_valid_redirect_uris(uris: &[String]) -> Vec<String> {
    let mut filtered = Vec::new();
    for uri in uris {
        if is_valid_registered_redirect_uri(uri) && !filtered.contains(uri) {
            filtered.push(uri.clone());
        }
    }
    filtered
}

/// Validate redirect URIs per RFC 8252 (OAuth 2.0 for Native Apps)
///
/// Allowed redirect URI types:
/// 1. Loopback: http://127.0.0.1:PORT/... or http://localhost:PORT/...
/// 2. Custom URL schemes: cursor://, vscode://, claude://, etc.
/// 3. ChatGPT connector OAuth callback URLs:
///    https://chatgpt.com/connector/oauth/...
///
/// NOT allowed (these are filtered from the request, not hard-failed):
/// - Other https:// URLs (except for confidential clients with proper secrets)
/// - http:// URLs to non-loopback addresses
/// - Dangerous schemes (javascript:, data:, vbscript:, file:, blob:) — rejected
///   even though they are non-http, since navigating a redirect to one can
///   execute script or load arbitrary content
///
/// Invalid URIs are silently skipped rather than rejecting the entire registration.
/// This is necessary because some clients (notably Cursor) send a mix of valid and
/// invalid redirect URIs in a single DCR request — failing the whole registration
/// would lock those clients out entirely, even though they only ever use the valid
/// URIs in practice. An error is only returned if zero valid URIs remain.
pub fn validate_redirect_uris(uris: &[String]) -> Result<(), DcrError> {
    if uris.is_empty() {
        return Err(DcrError::invalid_redirect_uri(
            "At least one redirect_uri is required",
        ));
    }

    let mut valid_count = 0;

    for uri in uris {
        let is_loopback = is_loopback_redirect_uri(uri);
        let is_custom_scheme = is_custom_scheme_redirect_uri(uri);
        let is_chatgpt_connector = is_chatgpt_connector_redirect_uri(uri);

        if !is_valid_registered_redirect_uri(uri) {
            // Skip invalid URIs (e.g. https://www.cursor.com/agents/mcp/oauth/callback)
            // rather than rejecting the entire registration — clients like Cursor send a
            // mix of valid and invalid URIs and only ever use the valid ones in practice.
            warn!(
                "[DCR] Skipping invalid redirect_uri: {} (must be loopback, custom scheme, or ChatGPT connector callback)",
                uri
            );
            continue;
        }

        debug!(
            "[DCR] Validated redirect_uri: {} (loopback={}, custom_scheme={}, chatgpt_connector={})",
            uri, is_loopback, is_custom_scheme, is_chatgpt_connector
        );
        valid_count += 1;
    }

    if valid_count == 0 {
        return Err(DcrError::invalid_redirect_uri(
            "No valid redirect_uris provided — must include at least one loopback \
             (http://127.0.0.1 or http://localhost), custom URL scheme \
             (e.g., cursor://, vscode://), or ChatGPT connector callback",
        ));
    }

    Ok(())
}

/// Process a DCR request and return a registered client or error
///
/// Uses the database as the single source of truth (no in-memory registry)
pub async fn process_dcr_request(
    repo: &mcpmux_storage::InboundClientRepository,
    request: DcrRequest,
) -> Result<DcrResponse, DcrError> {
    info!(
        "[DCR] Processing registration for: {} (redirect_uris: {:?})",
        request.client_name, request.redirect_uris
    );

    // Validate and filter redirect URIs. DCR clients sometimes submit a mixed list;
    // only registered-safe URIs are persisted and returned.
    validate_redirect_uris(&request.redirect_uris)?;
    let valid_redirect_uris = filter_valid_redirect_uris(&request.redirect_uris);

    // Idempotent registration: an app that registers again with the same name
    // and only redirect URIs it already has gets its existing client back,
    // unchanged. Anything else becomes a separate client, so registering under
    // another app's name can never change that app's record (redirects,
    // metadata, approval).
    // The client's redirects must match the request's in both directions:
    // a client registered first with a wider set (another app's callbacks
    // included) is not handed to the app asking for a narrower one.
    let existing = repo
        .find_dcr_clients_by_name(&request.client_name)
        .await
        .map_err(|e| DcrError::invalid_client_metadata(format!("Database error: {}", e)))?
        .into_iter()
        .find(|c| {
            let registered = filter_valid_redirect_uris(&c.redirect_uris);
            valid_redirect_uris
                .iter()
                .all(|uri| is_redirect_uri_allowed(&registered, uri))
                && registered
                    .iter()
                    .all(|uri| is_redirect_uri_allowed(&valid_redirect_uris, uri))
        });

    if let Some(existing) = existing {
        info!(
            "[DCR] Returning existing client for repeat registration: {} ({})",
            existing.client_name, existing.client_id
        );
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        return Ok(DcrResponse {
            client_id: existing.client_id,
            client_name: existing.client_name,
            redirect_uris: filter_valid_redirect_uris(&existing.redirect_uris),
            grant_types: existing.grant_types,
            response_types: existing.response_types,
            // Public client: no secret is ever issued, whatever an older
            // registration stored.
            token_endpoint_auth_method: "none".to_string(),
            scope: existing.scope,
            client_id_issued_at: now_unix,
            logo_uri: existing.logo_uri,
            client_uri: existing.client_uri,
            tos_uri: request.tos_uri,
            policy_uri: request.policy_uri,
            contacts: request.contacts,
            software_id: existing.software_id,
            software_version: existing.software_version,
        });
    }

    // Generate a client_id that is not in use (ids are short, so check).
    let mut client_id = format!("mcp_{}", &Uuid::new_v4().simple().to_string()[..8]);
    while repo
        .get_client(&client_id)
        .await
        .map_err(|e| DcrError::invalid_client_metadata(format!("Database error: {}", e)))?
        .is_some()
    {
        client_id = format!("mcp_{}", &Uuid::new_v4().simple().to_string()[..8]);
    }
    let now_str = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Default grant_types and response_types per OAuth 2.1
    let grant_types = if request.grant_types.is_empty() {
        vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ]
    } else {
        request.grant_types.clone()
    };

    let response_types = if request.response_types.is_empty() {
        vec!["code".to_string()]
    } else {
        request.response_types.clone()
    };

    // Registered clients are public: no secret is ever issued, so record
    // (and answer) "none" whatever was asked for.
    let token_endpoint_auth_method = "none".to_string();

    // Use helper to build new client (default settings)
    let client = build_inbound_client_from_request(
        &request,
        client_id.clone(),
        valid_redirect_uris.clone(),
        grant_types.clone(),
        response_types.clone(),
        token_endpoint_auth_method.clone(),
        None, // No alias yet
        Some(now_str.clone()),
        now_str.clone(),
        now_str,
    );

    // Save to database (single source of truth)
    repo.save_client(&client)
        .await
        .map_err(|e| DcrError::invalid_client_metadata(format!("Failed to save client: {}", e)))?;

    info!(
        "[DCR] New client registered: {} ({})",
        request.client_name, client_id
    );

    Ok(DcrResponse {
        client_id,
        client_name: request.client_name,
        redirect_uris: valid_redirect_uris,
        grant_types,
        response_types,
        token_endpoint_auth_method,
        scope: request.scope,
        client_id_issued_at: now_unix,
        // RFC 7591 client metadata
        logo_uri: request.logo_uri,
        client_uri: request.client_uri,
        tos_uri: request.tos_uri,
        policy_uri: request.policy_uri,
        contacts: request.contacts,
        software_id: request.software_id,
        software_version: request.software_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uris_with_a_fragment_are_rejected() {
        assert!(!is_valid_registered_redirect_uri(
            "http://127.0.0.1:8080/callback#frag"
        ));
        assert!(!is_valid_registered_redirect_uri("cursor://callback#x"));
        assert!(is_valid_registered_redirect_uri(
            "http://127.0.0.1:8080/callback"
        ));
    }

    #[test]
    fn loopback_matching_ignores_only_the_port() {
        let registered = vec!["http://127.0.0.1:8080/callback?app=a".to_string()];
        assert!(is_redirect_uri_allowed(
            &registered,
            "http://127.0.0.1:9999/callback?app=a"
        ));
        assert!(!is_redirect_uri_allowed(
            &registered,
            "http://127.0.0.1:9999/callback?app=b"
        ));
        assert!(!is_redirect_uri_allowed(
            &registered,
            "http://127.0.0.1:9999/callback?app=a#x"
        ));
    }

    #[test]
    fn test_validate_loopback_uris() {
        // Valid loopback URIs
        assert!(validate_redirect_uris(&["http://127.0.0.1:8080/callback".to_string()]).is_ok());
        assert!(validate_redirect_uris(&["http://localhost:3000/callback".to_string()]).is_ok());
        assert!(validate_redirect_uris(&["http://[::1]:8080/callback".to_string()]).is_ok());
    }

    #[test]
    fn test_validate_custom_scheme_uris() {
        // Valid custom scheme URIs
        assert!(validate_redirect_uris(&["cursor://callback".to_string()]).is_ok());
        assert!(validate_redirect_uris(&["vscode://callback".to_string()]).is_ok());
        assert!(validate_redirect_uris(&["claude://auth/callback".to_string()]).is_ok());
    }

    #[test]
    fn test_reject_invalid_uris() {
        // Invalid URIs (non-loopback http) — fail when no valid URIs remain
        assert!(validate_redirect_uris(&["http://example.com/callback".to_string()]).is_err());
        assert!(validate_redirect_uris(&["https://example.com/callback".to_string()]).is_err());
    }

    #[test]
    fn test_mixed_valid_and_invalid_uris_pass() {
        // Real-world case: Cursor sends a mix of valid (custom scheme + loopback) and
        // invalid (https) URIs. Registration must succeed as long as at least one valid
        // URI is present — otherwise clients that send any non-loopback HTTPS URI cannot
        // register at all.
        let uris = vec![
            "cursor://anysphere.cursor-mcp/oauth/callback".to_string(),
            "https://www.cursor.com/agents/mcp/oauth/callback".to_string(),
            "http://localhost:8787/callback".to_string(),
        ];
        assert!(validate_redirect_uris(&uris).is_ok());
    }

    #[test]
    fn test_validate_chatgpt_connector_redirect_uri() {
        assert!(validate_redirect_uris(
            &["https://chatgpt.com/connector/oauth/abc123".to_string()]
        )
        .is_ok());
    }

    #[test]
    fn test_reject_dangerous_custom_schemes() {
        // Script/content schemes must never count as a valid redirect target, even
        // though they aren't http(s). A redirect navigated to one of these (e.g. via
        // the consent modal's window.location.href fallback) could execute script or
        // load arbitrary content.
        for uri in [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
            "file:///etc/passwd",
            "blob:https://evil.example/uuid",
        ] {
            assert!(
                !is_custom_scheme_redirect_uri(uri),
                "{uri} must not be a valid custom-scheme redirect"
            );
            assert!(
                validate_redirect_uris(&[uri.to_string()]).is_err(),
                "{uri} must be rejected when it is the only redirect_uri"
            );
        }

        // McpMux's own deep-link scheme, in any case.
        let own = mcpmux_core::branding::DEEP_LINK_SCHEME;
        for uri in [
            format!("{own}://install?server=x"),
            format!("{}://callback", own.to_ascii_uppercase()),
        ] {
            assert!(!is_custom_scheme_redirect_uri(&uri), "{uri}");
            assert!(validate_redirect_uris(&[uri.clone()]).is_err(), "{uri}");
        }

        // Legitimate native-app schemes still pass, with or without an authority.
        assert!(is_custom_scheme_redirect_uri("cursor://callback"));
        assert!(is_custom_scheme_redirect_uri(
            "com.example.app:/oauth2redirect"
        ));
    }

    #[test]
    fn test_chatgpt_connector_anchoring_rejects_lookalikes() {
        // Only https://chatgpt.com/connector/oauth[/...] is accepted.
        assert!(is_chatgpt_connector_redirect_uri(
            "https://chatgpt.com/connector/oauth/abc123"
        ));
        assert!(is_chatgpt_connector_redirect_uri(
            "https://chatgpt.com/connector/oauth"
        ));

        for uri in [
            "http://chatgpt.com/connector/oauth/abc",       // not https
            "https://chatgpt.com.evil.com/connector/oauth", // suffix host
            "https://chatgpt.com@evil.com/connector/oauth", // userinfo, host=evil.com
            "https://evil.com/connector/oauth",             // wrong host
            "https://chatgpt.com/connector/oauthEVIL",      // path not anchored
            "https://chatgpt.com/connector/evil",           // wrong path
        ] {
            assert!(
                !is_chatgpt_connector_redirect_uri(uri),
                "{uri} must not match the ChatGPT connector rule"
            );
        }
    }

    #[test]
    fn test_loopback_rejects_userinfo_and_subdomain_tricks() {
        // Loopback-looking userinfo or subdomains must not be treated as loopback.
        for uri in [
            "http://localhost@evil.com/callback",
            "http://127.0.0.1@evil.com/callback",
            "http://localhost.evil.com/callback",
            "http://127.0.0.1.evil.com/callback",
        ] {
            assert!(
                !is_loopback_redirect_uri(uri),
                "{uri} must not be treated as loopback"
            );
        }

        // Genuine loopback hosts still pass.
        assert!(is_loopback_redirect_uri("http://127.0.0.1:8080/callback"));
        assert!(is_loopback_redirect_uri("http://localhost/callback"));
        assert!(is_loopback_redirect_uri("http://[::1]:8080/callback"));
    }

    #[test]
    fn test_all_invalid_uris_fail() {
        let uris = vec![
            "https://www.cursor.com/agents/mcp/oauth/callback".to_string(),
            "http://example.com/callback".to_string(),
        ];
        assert!(validate_redirect_uris(&uris).is_err());
    }

    #[test]
    fn test_filter_redirect_uris_drops_invalid_entries() {
        let uris = vec![
            "cursor://anysphere.cursor-mcp/oauth/callback".to_string(),
            "https://www.cursor.com/agents/mcp/oauth/callback".to_string(),
            "https://chatgpt.com/connector/oauth/abc123".to_string(),
            "http://localhost.evil/callback".to_string(),
        ];
        assert_eq!(
            filter_valid_redirect_uris(&uris),
            vec![
                "cursor://anysphere.cursor-mcp/oauth/callback".to_string(),
                "https://chatgpt.com/connector/oauth/abc123".to_string(),
            ]
        );
    }

    #[test]
    fn loopback_ignores_port_per_rfc_8252() {
        // Registered with one port, requested with another — must match.
        let registered = vec!["http://127.0.0.1:12345/callback".to_string()];
        assert!(is_redirect_uri_allowed(
            &registered,
            "http://127.0.0.1:44307/callback"
        ));
        assert!(is_redirect_uri_allowed(
            &registered,
            "http://127.0.0.1:1/callback"
        ));

        let localhost = vec!["http://localhost:3000/callback".to_string()];
        assert!(is_redirect_uri_allowed(
            &localhost,
            "http://localhost:55555/callback"
        ));

        let ipv6 = vec!["http://[::1]:8080/callback".to_string()];
        assert!(is_redirect_uri_allowed(&ipv6, "http://[::1]:9999/callback"));
    }

    #[test]
    fn loopback_requires_matching_scheme_host_and_path() {
        let registered = vec!["http://127.0.0.1:8080/callback".to_string()];
        // Different path
        assert!(!is_redirect_uri_allowed(
            &registered,
            "http://127.0.0.1:8080/other"
        ));
        // Different host family — 127.0.0.1 and localhost are not interchangeable
        // (per RFC 8252, clients SHOULD NOT use `localhost`; treat as distinct).
        assert!(!is_redirect_uri_allowed(
            &registered,
            "http://localhost:8080/callback"
        ));
        // HTTPS vs HTTP
        assert!(!is_redirect_uri_allowed(
            &registered,
            "https://127.0.0.1:8080/callback"
        ));
    }

    #[test]
    fn non_loopback_requires_exact_match() {
        // HTTPS: exact match only (no port flex)
        let https = vec!["https://app.example.com/callback".to_string()];
        assert!(is_redirect_uri_allowed(
            &https,
            "https://app.example.com/callback"
        ));
        assert!(!is_redirect_uri_allowed(
            &https,
            "https://app.example.com:8443/callback"
        ));

        // Custom scheme: exact match only
        let custom = vec!["cursor://callback".to_string()];
        assert!(is_redirect_uri_allowed(&custom, "cursor://callback"));
        assert!(!is_redirect_uri_allowed(&custom, "cursor://other"));
    }

    #[test]
    fn unparseable_uris_fall_back_to_strict_equality() {
        let registered = vec!["not-a-url".to_string()];
        assert!(is_redirect_uri_allowed(&registered, "not-a-url"));
        assert!(!is_redirect_uri_allowed(&registered, "not-a-url-either"));
    }

    #[test]
    fn empty_registered_list_denies_everything() {
        assert!(!is_redirect_uri_allowed(
            &[],
            "http://127.0.0.1:8080/callback"
        ));
    }

    // Note: Integration tests for idempotent registration are better handled
    // in tests that use an actual database, since process_dcr_request now
    // persists directly to the database.

    #[test]
    fn os_handler_schemes_are_not_redirect_targets() {
        for uri in [
            "search-ms:query=x&crumb=location:\\\\evil\\share",
            "ms-officecmd:{}",
            "ms-word:ofe|u|https://evil.example/doc",
            "microsoft-edge:https://evil.example/cb",
            "x-safari-https://evil.example/cb",
            "smb://evil.example/share",
            "file:///etc/passwd",
            "itms-services://?action=download-manifest",
            "MS-SETTINGS:privacy",
        ] {
            assert!(
                !is_valid_registered_redirect_uri(uri),
                "{uri} must be refused"
            );
        }
        for uri in [
            "cursor://anysphere.cursor-mcp/oauth/callback",
            "vscode://vscode.github-authentication/did-authenticate",
            "claude://claude.ai/oauth/callback",
            "com.example.app:/oauth2redirect",
        ] {
            assert!(
                is_valid_registered_redirect_uri(uri),
                "{uri} must be accepted"
            );
        }
    }
}
