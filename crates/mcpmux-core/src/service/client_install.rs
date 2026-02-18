//! Client IDE install helpers.
//!
//! Deep link URI generators for VS Code and Cursor one-click MCP server install.

/// Generate the VS Code deep link URI for one-click MCP install.
pub fn vscode_deep_link(gateway_url: &str) -> String {
    let config = serde_json::json!({
        "name": "mcpmux",
        "type": "http",
        "url": format!("{}/mcp", gateway_url)
    });
    let config_str = config.to_string();
    let encoded = urlencoding::encode(&config_str);
    format!("vscode:mcp/install?{}", encoded)
}

/// Generate the Cursor deep link URI for one-click MCP install.
pub fn cursor_deep_link(gateway_url: &str) -> String {
    use base64::Engine;

    let config = serde_json::json!({
        "url": format!("{}/mcp", gateway_url)
    });
    let encoded_config = base64::engine::general_purpose::STANDARD.encode(config.to_string());
    format!(
        "cursor://anysphere.cursor-deeplink/mcp/install?name=McpMux&config={}",
        encoded_config
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vscode_deep_link() {
        let link = vscode_deep_link("http://localhost:45818");
        assert!(link.starts_with("vscode:mcp/install?"));
        assert!(link.contains("mcpmux"));
        assert!(link.contains("localhost"));
    }

    #[test]
    fn test_cursor_deep_link() {
        let link = cursor_deep_link("http://localhost:45818");
        assert!(link.starts_with("cursor://anysphere.cursor-deeplink/mcp/install?"));
        assert!(link.contains("name=McpMux"));
        assert!(link.contains("config="));
    }
}
