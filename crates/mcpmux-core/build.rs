//! Build script that generates branding constants from branding.toml
//!
//! This reads the workspace-level branding.toml and generates Rust constants
//! that are included at compile time.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // Re-run if branding.toml changes
    println!("cargo:rerun-if-changed=../../branding.toml");

    // Find branding.toml relative to this crate (2 levels up to workspace root)
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = Path::new(&manifest_dir).parent().unwrap().parent().unwrap();
    let branding_path = workspace_root.join("branding.toml");

    if !branding_path.exists() {
        // Generate defaults if branding.toml doesn't exist
        generate_defaults();
        return;
    }

    let content = fs::read_to_string(&branding_path).expect("Failed to read branding.toml");

    // Simple TOML parsing without external dependency
    let display_name = extract_toml_string(&content, "display_name").unwrap_or("MCP Mux");
    let identifier = extract_toml_string(&content, "identifier").unwrap_or("com.mcpmux.app");
    let deep_link_scheme = extract_toml_string(&content, "deep_link_scheme").unwrap_or("mcpmux");
    let domain = extract_toml_string(&content, "domain").unwrap_or("mcpmux.com");
    let api_domain = extract_toml_string(&content, "api_domain").unwrap_or("api.mcpmux.com");
    let keychain_service =
        extract_toml_string(&content, "keychain_service").unwrap_or("com.mcpmux.desktop");
    let log_prefix = extract_toml_string(&content, "log_prefix").unwrap_or("mcpmux");
    let mcp_config_key = extract_toml_string(&content, "mcp_config_key").unwrap_or("mcpmux");
    let github_org = extract_toml_string(&content, "github_org").unwrap_or("mcpmux");
    let npm_scope = extract_toml_string(&content, "npm_scope").unwrap_or("@mcpmux");

    // Generate Rust constants
    let out_dir = env::var("OUT_DIR").unwrap();
    let rust_path = Path::new(&out_dir).join("branding_generated.rs");

    let rust_code = format!(
        r#"// Auto-generated branding constants from branding.toml
// DO NOT EDIT - regenerate with `cargo build`

/// User-facing display name
pub const DISPLAY_NAME: &str = {display_name:?};

/// Reverse-domain app identifier
pub const IDENTIFIER: &str = {identifier:?};

/// Custom URL scheme for deep links
pub const DEEP_LINK_SCHEME: &str = {deep_link_scheme:?};

/// Primary domain
pub const DOMAIN: &str = {domain:?};

/// API subdomain
pub const API_DOMAIN: &str = {api_domain:?};

/// Keychain/credential manager service name
pub const KEYCHAIN_SERVICE: &str = {keychain_service:?};

/// Log file prefix
pub const LOG_PREFIX: &str = {log_prefix:?};

/// MCP server config key (for clients like VS Code, Claude Desktop)
pub const MCP_CONFIG_KEY: &str = {mcp_config_key:?};

/// GitHub organization
pub const GITHUB_ORG: &str = {github_org:?};

/// NPM scope
pub const NPM_SCOPE: &str = {npm_scope:?};
"#,
        display_name = display_name,
        identifier = identifier,
        deep_link_scheme = deep_link_scheme,
        domain = domain,
        api_domain = api_domain,
        keychain_service = keychain_service,
        log_prefix = log_prefix,
        mcp_config_key = mcp_config_key,
        github_org = github_org,
        npm_scope = npm_scope,
    );

    fs::write(&rust_path, rust_code).expect("Failed to write branding_generated.rs");
}

/// Generate default constants when branding.toml doesn't exist
fn generate_defaults() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let rust_path = Path::new(&out_dir).join("branding_generated.rs");

    let rust_code = r#"// Auto-generated branding constants (defaults - no branding.toml found)
// Create branding.toml in workspace root to customize

pub const DISPLAY_NAME: &str = "MCP Mux";
pub const IDENTIFIER: &str = "com.mcpmux.app";
pub const DEEP_LINK_SCHEME: &str = "mcpmux";
pub const DOMAIN: &str = "mcpmux.com";
pub const API_DOMAIN: &str = "api.mcpmux.com";
pub const KEYCHAIN_SERVICE: &str = "com.mcpmux.desktop";
pub const LOG_PREFIX: &str = "mcpmux";
pub const MCP_CONFIG_KEY: &str = "mcpmux";
pub const GITHUB_ORG: &str = "mcpmux";
pub const NPM_SCOPE: &str = "@mcpmux";
"#;

    fs::write(&rust_path, rust_code).expect("Failed to write branding_generated.rs");
}

/// Extract a string value from TOML content (simple parser, no dependencies)
fn extract_toml_string<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with(key) {
            if let Some(eq_pos) = line.find('=') {
                let value = line[eq_pos + 1..].trim();
                // Remove quotes
                if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                    return Some(&value[1..value.len() - 1]);
                }
            }
        }
    }
    None
}
