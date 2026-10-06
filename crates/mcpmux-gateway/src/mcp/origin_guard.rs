//! Block web pages from reaching the MCP endpoint.
//!
//! A browser is a local program that runs code from any website, so a page
//! the user opens can make the browser send requests to `localhost:45818`.
//! Browsers attach an `Origin` header to every cross-site request (CORS
//! preflights and "no-cors" POSTs included); native MCP clients (Claude Code,
//! Cursor, VS Code, Claude Desktop) send none. Rejecting `/mcp` requests whose
//! `Origin` is anything other than this machine keeps websites out without
//! touching real clients. The MCP Streamable HTTP spec requires servers to
//! validate `Origin` for exactly this reason.
//!
//! Together with rmcp's `Host` allowlist (which stops DNS rebinding) this is
//! what makes running without an access key safe on a loopback-only gateway.

use url::{Host, Url};

/// Whether a request carrying this `Origin` header may use `/mcp`.
///
/// Allowed: pages served from this machine (`localhost`, `127.0.0.0/8`,
/// `[::1]` on any port, e.g. the MCP Inspector), the desktop app's own webview
/// origins, and the configured public base URL (a tunnel the user set up).
/// Everything else is blocked, including the opaque `null` origin that
/// sandboxed iframes and `file://` pages send.
pub fn is_allowed_origin(origin: &str, public_base_url: Option<&str>) -> bool {
    let Ok(url) = Url::parse(origin.trim()) else {
        return false;
    };

    match url.scheme() {
        "http" | "https" => {}
        // Tauri webview origin on macOS/Linux.
        "tauri" => return url.host_str() == Some("localhost"),
        _ => return false,
    }

    let loopback = match url.host() {
        Some(Host::Domain(domain)) => {
            let domain = domain.to_ascii_lowercase();
            // `tauri.localhost` is the Tauri webview origin on Windows.
            domain == "localhost" || domain == "tauri.localhost"
        }
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if loopback {
        return true;
    }

    public_base_url
        .and_then(|base| Url::parse(base.trim()).ok())
        .is_some_and(|base| base.origin() == url.origin())
}

#[cfg(test)]
mod tests {
    use super::is_allowed_origin;

    #[test]
    fn allows_pages_served_from_this_machine() {
        for origin in [
            "http://localhost",
            "http://localhost:6274",
            "https://LOCALHOST:8443",
            "http://127.0.0.1:5173",
            "http://127.1.2.3",
            "http://[::1]:3000",
        ] {
            assert!(
                is_allowed_origin(origin, None),
                "{origin} should be allowed"
            );
        }
    }

    #[test]
    fn allows_the_desktop_webview() {
        assert!(is_allowed_origin("tauri://localhost", None));
        assert!(is_allowed_origin("http://tauri.localhost", None));
        assert!(is_allowed_origin("https://tauri.localhost", None));
    }

    #[test]
    fn blocks_websites_and_opaque_origins() {
        for origin in [
            "https://evil.example",
            "http://192.168.1.20:45818",
            "http://localhost.evil.example",
            "http://127.0.0.1.nip.io",
            "tauri://evil.example",
            "chrome-extension://abcdef",
            "file://",
            "null",
            "",
            "not a url",
        ] {
            assert!(
                !is_allowed_origin(origin, None),
                "{origin} should be blocked"
            );
        }
    }

    #[test]
    fn allows_only_the_exact_public_base_url_origin() {
        let public = Some("https://mux.example.com/");
        assert!(is_allowed_origin("https://mux.example.com", public));
        assert!(!is_allowed_origin("http://mux.example.com", public));
        assert!(!is_allowed_origin("https://mux.example.com:8443", public));
        assert!(!is_allowed_origin("https://evil.example", public));
    }
}
