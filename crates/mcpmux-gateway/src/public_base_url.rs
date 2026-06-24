//! Resolve the OAuth / metadata base URL for inbound gateway requests.
//!
//! Local clients keep `http://localhost:{port}`; tunnel traffic uses the
//! configured public URL when Cloudflare Access or forwarded-host signals match.

use axum::http::HeaderMap;

/// Header Cloudflare Access adds to origin requests after successful auth.
const CF_ACCESS_JWT_HEADER: &str = "cf-access-jwt-assertion";

/// Normalize and validate an operator-supplied public gateway URL.
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

    Ok(format!("https://{host}"))
}

/// Pick the base URL for OAuth metadata and WWW-Authenticate on this request.
pub fn resolve_request_base_url(
    headers: &HeaderMap,
    local_base_url: &str,
    configured_public_url: Option<&str>,
) -> String {
    let Some(public) = configured_public_url.filter(|value| !value.is_empty()) else {
        return local_base_url.to_string();
    };

    let Some(public_host) = url_host(public) else {
        return local_base_url.to_string();
    };

    // cloudflared + CF Access inject this on every request that passed the edge policy.
    // Service tokens and browser sessions both get it; loopback clients never do.
    if header_value(headers, CF_ACCESS_JWT_HEADER).is_some() {
        return public.to_string();
    }

    if let Some(forwarded_host) = header_value(headers, "x-forwarded-host") {
        let host = forwarded_host.split(',').next().unwrap_or("").trim();
        if host_matches_public(host, &public_host) {
            let proto = header_value(headers, "x-forwarded-proto")
                .and_then(|value| value.split(',').next())
                .unwrap_or("https")
                .trim();
            return format!("{proto}://{host}");
        }
    }

    // Bypassed CF Access paths reach the origin without JWT or forwarded-host
    // headers, but cloudflared still sets Host to the public hostname.
    if let Some(host) = header_value(headers, "host") {
        let host = host.split(',').next().unwrap_or("").trim();
        if host_matches_public(host, &public_host) {
            return public.to_string();
        }
    }

    local_base_url.to_string()
}

/// Host values accepted by rmcp Streamable HTTP `Host` validation.
///
/// Loopback defaults prevent DNS rebinding; the configured public hostname is
/// added when tunnel traffic arrives with an external `Host` header.
pub fn streamable_http_allowed_hosts(
    local_port: u16,
    configured_public_url: Option<&str>,
) -> Vec<String> {
    let mut hosts = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
        format!("localhost:{local_port}"),
        format!("127.0.0.1:{local_port}"),
    ];

    let Some(public) = configured_public_url.filter(|value| !value.is_empty()) else {
        return hosts;
    };

    let Ok(parsed) = url::Url::parse(public) else {
        return hosts;
    };
    let Some(host) = parsed.host_str() else {
        return hosts;
    };

    hosts.push(host.to_string());
    if let Some(port) = parsed.port() {
        hosts.push(format!("{host}:{port}"));
    }

    hosts
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn url_host(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_ascii_lowercase))
}

fn host_matches_public(forwarded_host: &str, public_host: &str) -> bool {
    forwarded_host
        .split(':')
        .next()
        .unwrap_or(forwarded_host)
        .eq_ignore_ascii_case(public_host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn normalize_accepts_https_origin() {
        assert_eq!(
            normalize_public_url("https://mcp.example.com/mcp").unwrap(),
            "https://mcp.example.com"
        );
    }

    #[test]
    fn normalize_rejects_http() {
        assert!(normalize_public_url("http://mcp.example.com").is_err());
    }

    #[test]
    fn resolve_uses_local_without_public_url() {
        let headers = HeaderMap::new();
        assert_eq!(
            resolve_request_base_url(&headers, "http://localhost:45818", None),
            "http://localhost:45818"
        );
    }

    #[test]
    fn resolve_uses_public_url_when_forwarded_host_matches() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-host", "mcp.example.com".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        assert_eq!(
            resolve_request_base_url(
                &headers,
                "http://localhost:45818",
                Some("https://mcp.example.com")
            ),
            "https://mcp.example.com"
        );
    }

    #[test]
    fn resolve_uses_public_url_when_cf_access_jwt_present() {
        let mut headers = HeaderMap::new();
        headers.insert(CF_ACCESS_JWT_HEADER, "jwt".parse().unwrap());
        assert_eq!(
            resolve_request_base_url(
                &headers,
                "http://localhost:45818",
                Some("https://mcp.example.com")
            ),
            "https://mcp.example.com"
        );
    }

    #[test]
    fn resolve_keeps_local_when_forwarded_host_differs() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-host", "other.example.com".parse().unwrap());
        assert_eq!(
            resolve_request_base_url(
                &headers,
                "http://localhost:45818",
                Some("https://mcp.example.com")
            ),
            "http://localhost:45818"
        );
    }

    #[test]
    fn resolve_uses_public_url_when_host_header_matches() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "mcp.example.com".parse().unwrap());
        assert_eq!(
            resolve_request_base_url(
                &headers,
                "http://localhost:45818",
                Some("https://mcp.example.com")
            ),
            "https://mcp.example.com"
        );
    }

    #[test]
    fn streamable_allowed_hosts_includes_public_hostname() {
        let hosts = streamable_http_allowed_hosts(45818, Some("https://mcp.example.com"));
        assert!(hosts.contains(&"mcp.example.com".to_string()));
        assert!(hosts.contains(&"127.0.0.1:45818".to_string()));
    }
}
