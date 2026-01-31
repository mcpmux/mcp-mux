//! Centralized branding constants
//!
//! All product naming comes from this module.
//! Generated from branding.toml at build time.
//!
//! # Usage
//!
//! ```rust
//! use mcpmux_core::branding;
//!
//! // Access constants directly
//! println!("App: {}", branding::DISPLAY_NAME);
//!
//! // Use helper functions
//! let prefix = branding::deep_link_prefix(); // "mcpmux://"
//! let url = branding::api_url("/v1/servers");
//! ```

// Include generated constants from build.rs
include!(concat!(env!("OUT_DIR"), "/branding_generated.rs"));

/// Get the deep link URL prefix (e.g., "mcpmux://")
pub fn deep_link_prefix() -> String {
    format!("{}://", DEEP_LINK_SCHEME)
}

/// Check if a URL is a deep link for this app
pub fn is_deep_link(url: &str) -> bool {
    url.starts_with(&format!("{}://", DEEP_LINK_SCHEME))
}

/// Get the full API URL for a path
///
/// # Example
/// ```ignore
/// let url = branding::api_url("/v1/servers");
/// // Returns: "https://api.mcpmux.com/v1/servers"
/// ```
pub fn api_url(path: &str) -> String {
    format!(
        "https://{}/{}",
        API_DOMAIN,
        path.trim_start_matches('/')
    )
}

/// Get the schema URL for a schema name
///
/// # Example
/// ```ignore
/// let url = branding::schema_url("server-definition.json");
/// // Returns: "https://mcpmux.com/schemas/server-definition.json"
/// ```
pub fn schema_url(schema_name: &str) -> String {
    format!("https://{}/schemas/{}", DOMAIN, schema_name)
}

/// Get the window title with suffix
///
/// # Example
/// ```ignore
/// let title = branding::window_title("Settings");
/// // Returns: "MCP Mux — Settings"
/// ```
pub fn window_title(suffix: &str) -> String {
    format!("{} — {}", DISPLAY_NAME, suffix)
}

/// Get the OAuth callback URI using loopback interface redirect
///
/// Per RFC 8252 Section 7.3, loopback interface redirection uses:
/// `http://127.0.0.1:{port}/oauth2redirect`
///
/// This is the most compatible method for native app OAuth as:
/// - Enterprise security systems don't block loopback addresses
/// - No custom URL scheme registration required
/// - Works universally across all OAuth providers
///
/// The port is dynamic and assigned at runtime. This function returns
/// the base path pattern; the full URL with port is constructed during
/// the OAuth flow.
///
/// Note: We use 127.0.0.1 (not localhost) per RFC 8252 recommendation
/// to avoid DNS resolution issues and firewall interference.
///
/// # Example
/// ```ignore
/// let path = branding::oauth_callback_path();
/// // Returns: "/oauth2redirect"
/// // Full URL example: "http://127.0.0.1:9876/oauth2redirect"
/// ```
pub fn oauth_callback_path() -> &'static str {
    "/oauth2redirect"
}

/// Get the OAuth client name for outbound DCR registration
///
/// This is a simple, user-friendly name shown during OAuth consent.
/// Uses a single consistent name (not per-server) to keep DCR registrations clean.
///
/// # Example
/// ```ignore
/// let name = branding::outbound_oauth_client_name();
/// // Returns: "MCP Mux"
/// ```
pub fn outbound_oauth_client_name() -> &'static str {
    DISPLAY_NAME
}

/// Build the OAuth client name for DCR with optional space name
///
/// When a space name is provided, returns "MCP Mux (Space Name)" to help users
/// identify which space a registration belongs to when viewing authorized apps.
///
/// # Example
/// ```ignore
/// let name = branding::outbound_oauth_client_name_for_space(Some("Work"));
/// // Returns: "MCP Mux (Work)"
///
/// let name = branding::outbound_oauth_client_name_for_space(None);
/// // Returns: "MCP Mux"
/// ```
pub fn outbound_oauth_client_name_for_space(space_name: Option<&str>) -> String {
    match space_name {
        Some(name) if !name.is_empty() => format!("{} ({})", DISPLAY_NAME, name),
        _ => DISPLAY_NAME.to_string(),
    }
}

/// Default preferred port for OAuth callbacks (adjacent to gateway port)
/// 
/// Uses a high port number to avoid conflicts:
/// - Adjacent to gateway port (45818) for easy identification
/// - Well above common service ports (0-10000)
/// - Falls back to dynamic port if unavailable, then persists that port
pub const DEFAULT_OAUTH_CALLBACK_PORT: u16 = 45819;

/// Build a complete OAuth callback URI for loopback redirect
///
/// # Arguments
/// * `port` - The ephemeral port the callback server is listening on
///
/// # Example
/// ```ignore
/// let uri = branding::oauth_callback_uri_with_port(9876);
/// // Returns: "http://127.0.0.1:9876/oauth2redirect"
/// ```
pub fn oauth_callback_uri_with_port(port: u16) -> String {
    format!("http://127.0.0.1:{}{}", port, oauth_callback_path())
}

/// Check if a URL is a loopback OAuth callback for this app
///
/// Matches both IPv4 (127.0.0.1) and IPv6 ([::1]) loopback addresses
/// with the expected callback path.
///
/// # Example
/// ```ignore
/// assert!(branding::is_oauth_callback("http://127.0.0.1:9876/oauth2redirect?code=123"));
/// assert!(branding::is_oauth_callback("http://[::1]:9876/oauth2redirect?code=123"));
/// assert!(!branding::is_oauth_callback("https://example.com"));
/// ```
pub fn is_oauth_callback(url: &str) -> bool {
    let path = oauth_callback_path();
    // Match IPv4 loopback
    if url.starts_with("http://127.0.0.1:") && url.contains(path) {
        return true;
    }
    // Match IPv6 loopback
    if url.starts_with("http://[::1]:") && url.contains(path) {
        return true;
    }
    false
}

// =============================================================================
// Gateway Port Service (re-exports)
// =============================================================================

pub use crate::service::{
    GatewayPortService,
    PortAllocationError,
    PortResolution,
    DEFAULT_GATEWAY_PORT,
    is_port_available,
    allocate_dynamic_port,
};

// =============================================================================
// App Settings (re-exports)
// =============================================================================

pub use crate::service::{AppSettingsService, keys as settings_keys};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants_populated() {
        assert!(!DISPLAY_NAME.is_empty());
        assert!(IDENTIFIER.starts_with("com."));
        assert!(!DEEP_LINK_SCHEME.is_empty());
        assert!(!DOMAIN.is_empty());
        assert!(!API_DOMAIN.is_empty());
        assert!(!KEYCHAIN_SERVICE.is_empty());
        assert!(!LOG_PREFIX.is_empty());
    }

    #[test]
    fn test_deep_link_prefix() {
        let prefix = deep_link_prefix();
        assert!(prefix.ends_with("://"));
        assert_eq!(prefix, format!("{}://", DEEP_LINK_SCHEME));
    }

    #[test]
    fn test_is_deep_link() {
        let scheme = DEEP_LINK_SCHEME;
        assert!(is_deep_link(&format!("{}://authorize", scheme)));
        assert!(is_deep_link(&format!("{}://callback?code=123", scheme)));
        assert!(!is_deep_link("https://example.com"));
        assert!(!is_deep_link("http://localhost:3000"));
    }

    #[test]
    fn test_api_url() {
        let url = api_url("/v1/servers");
        assert!(url.starts_with("https://"));
        assert!(url.contains(API_DOMAIN));
        assert!(url.ends_with("/v1/servers"));

        // Test without leading slash
        let url2 = api_url("v1/servers");
        assert_eq!(url, url2);
    }

    #[test]
    fn test_schema_url() {
        let url = schema_url("server-definition.json");
        assert!(url.starts_with("https://"));
        assert!(url.contains(DOMAIN));
        assert!(url.contains("/schemas/"));
        assert!(url.ends_with("server-definition.json"));
    }

    #[test]
    fn test_window_title() {
        let title = window_title("Settings");
        assert!(title.contains(DISPLAY_NAME));
        assert!(title.contains("Settings"));
    }

    #[test]
    fn test_oauth_callback_path() {
        let path = oauth_callback_path();
        assert!(path.starts_with('/'));
        assert_eq!(path, "/oauth2redirect");
    }

    #[test]
    fn test_oauth_callback_uri_with_port() {
        let uri = oauth_callback_uri_with_port(9876);
        assert!(uri.starts_with("http://127.0.0.1:"));
        assert!(uri.contains(":9876"));
        assert!(uri.ends_with("/oauth2redirect"));
        assert_eq!(uri, "http://127.0.0.1:9876/oauth2redirect");
    }

    #[test]
    fn test_is_oauth_callback() {
        // IPv4 loopback should match
        assert!(is_oauth_callback("http://127.0.0.1:9876/oauth2redirect"));
        assert!(is_oauth_callback("http://127.0.0.1:9876/oauth2redirect?code=123&state=abc"));
        assert!(is_oauth_callback("http://127.0.0.1:51234/oauth2redirect?error=access_denied"));
        
        // IPv6 loopback should match
        assert!(is_oauth_callback("http://[::1]:9876/oauth2redirect"));
        assert!(is_oauth_callback("http://[::1]:9876/oauth2redirect?code=123"));
        
        // Non-matching formats
        assert!(!is_oauth_callback("https://127.0.0.1:9876/oauth2redirect")); // https not http
        assert!(!is_oauth_callback("http://localhost:9876/oauth2redirect")); // localhost not IP
        assert!(!is_oauth_callback("http://127.0.0.1:9876/callback")); // wrong path
        assert!(!is_oauth_callback("https://example.com/oauth2redirect"));
        assert!(!is_oauth_callback("mcpmux://callback/oauth")); // old scheme format
    }

    #[test]
    fn test_default_gateway_port_reexport() {
        assert_eq!(DEFAULT_GATEWAY_PORT, 45818);
    }
}

