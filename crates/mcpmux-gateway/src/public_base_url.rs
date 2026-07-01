//! Validate operator-supplied public gateway URLs.
//!
//! The actual base-URL resolution for OAuth metadata lives in
//! [`crate::server::handlers::effective_base_url`]; this module only
//! normalizes the raw string an operator provides via the Tauri command or
//! the admin HTTP API before it's persisted to `gateway.public_base_url`.

/// Normalize and validate an operator-supplied public gateway URL.
///
/// Mirrors the desktop app's `normalize_public_base_url` so a value set via
/// either the Tauri command or the admin HTTP API behaves identically, since
/// both persist to the same `gateway.public_base_url` setting.
///
/// Returns an empty string when `url` is blank (clears the setting).
pub fn normalize_public_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    let parsed = url::Url::parse(trimmed).map_err(|e| format!("Invalid URL: {e}"))?;
    if parsed.scheme() != "https" {
        return Err("Public gateway URL must use https".into());
    }
    let Some(host) = parsed.host_str() else {
        return Err("Public gateway URL must include a hostname".into());
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Public gateway URL must not include credentials".into());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("Public gateway URL must not include a query string or fragment".into());
    }
    if parsed.path() != "/" && !parsed.path().is_empty() {
        return Err(
            "Public gateway URL must be an origin only, for example https://mcp.example.com"
                .into(),
        );
    }

    Ok(match parsed.port() {
        Some(port) => format!("https://{host}:{port}"),
        None => format!("https://{host}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_https_origin() {
        assert_eq!(
            normalize_public_url("https://mcp.example.com").unwrap(),
            "https://mcp.example.com"
        );
    }

    #[test]
    fn normalize_preserves_port() {
        assert_eq!(
            normalize_public_url("https://mcp.example.com:8443").unwrap(),
            "https://mcp.example.com:8443"
        );
    }

    #[test]
    fn normalize_rejects_http() {
        assert!(normalize_public_url("http://mcp.example.com").is_err());
    }

    #[test]
    fn normalize_rejects_path() {
        assert!(normalize_public_url("https://mcp.example.com/mcp").is_err());
    }

    #[test]
    fn normalize_clears_on_blank() {
        assert_eq!(normalize_public_url("   ").unwrap(), "");
    }
}
