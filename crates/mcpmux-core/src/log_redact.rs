//! Formatting for log lines that would otherwise carry secrets.

/// Where a URL points, for logs: scheme, host and port, with `/…` standing
/// in for any path or query.
///
/// The rest is routinely secret: MCP server URLs get API keys substituted
/// into their path or query from inputs, OAuth authorization URLs carry
/// `state` and the PKCE challenge, redirects carry authorization codes, and
/// deep links carry consent request ids.
pub fn url_for_log(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return "[unparseable URL]".to_string();
    };
    let mut out = format!("{}://", parsed.scheme());
    if let Some(host) = parsed.host_str() {
        out.push_str(host);
    }
    if let Some(port) = parsed.port() {
        out.push_str(&format!(":{port}"));
    }
    let has_more = !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some();
    if has_more {
        out.push_str("/…");
    }
    out
}

/// The first characters of a bearer-like identifier (a consent request id,
/// session id, authorization code), enough to correlate log lines without
/// making the logged value usable.
pub fn id_for_log(id: &str) -> String {
    const SHOWN: usize = 8;
    match id.char_indices().nth(SHOWN) {
        Some((cut, _)) => format!("{}…", &id[..cut]),
        None => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_keep_only_where_they_point() {
        assert_eq!(
            url_for_log("http://127.0.0.1:43123/callback?code=abc&state=xyz"),
            "http://127.0.0.1:43123/…"
        );
        assert_eq!(
            url_for_log("https://actions.example.com/mcp/sk-secret/sse"),
            "https://actions.example.com/…"
        );
        assert_eq!(
            url_for_log("mcpmux://authorize?request_id=0123456789"),
            "mcpmux://authorize/…"
        );
        assert_eq!(
            url_for_log("https://user:pw@example.com/"),
            "https://example.com"
        );
        assert_eq!(url_for_log("not a url"), "[unparseable URL]");
    }

    #[test]
    fn ids_are_shortened() {
        assert_eq!(id_for_log("0123456789abcdef"), "01234567…");
        assert_eq!(id_for_log("short"), "short");
        assert_eq!(id_for_log("ééééééééé"), "éééééééé…");
    }
}
