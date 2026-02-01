//! Transport configuration resolution
//!
//! Handles building the actual runtime transport configuration from
//! the static registry definition and user-specific installation settings.

use super::ResolvedTransport;
use mcpmux_core::{InstalledServer, TransportConfig as RegistryConfig};
use std::collections::HashMap;
use std::path::Path;

const MCP_STATE_DIR_ENV: &str = "MCP_STATE_DIR";

/// Build transport config from registry transport and installed server
pub fn build_transport_config(
    registry_transport: &RegistryConfig,
    installed: &InstalledServer,
    base_state_dir: Option<&Path>,
) -> ResolvedTransport {
    tracing::debug!(
        "[TransportResolution] Building config for {}/{} with {} input values",
        installed.space_id,
        installed.server_id,
        installed.input_values.len()
    );

    match registry_transport {
        RegistryConfig::Stdio {
            command, args, env, ..
        } => {
            let resolved_command = resolve_placeholders(command, &installed.input_values);
            let mut resolved_args: Vec<String> = args
                .iter()
                .map(|arg| resolve_placeholders(arg, &installed.input_values))
                .collect();

            // Append user's extra args
            resolved_args.extend(installed.args_append.clone());

            // Build env from registry + input values + env_overrides
            let mut resolved_env = HashMap::new();

            // 1. Start with registry env
            for (k, v) in env {
                let resolved_value = resolve_placeholders(v, &installed.input_values);
                tracing::debug!(
                    "[TransportResolution] Registry env: {}={} → {}",
                    k,
                    v,
                    resolved_value
                );
                resolved_env.insert(k.clone(), resolved_value);
            }

            // 2. Add input values directly as env vars
            tracing::debug!(
                "[TransportResolution] Adding {} input values as direct env vars",
                installed.input_values.len()
            );
            resolved_env.extend(installed.input_values.clone());

            // 3. Apply user's env overrides
            resolved_env.extend(installed.env_overrides.clone());

            // 4. Inject MCP_STATE_DIR if not already set
            apply_state_dir_env(&mut resolved_env, base_state_dir, installed);

            tracing::debug!(
                "[TransportResolution] Final env has {} variables",
                resolved_env.len()
            );

            ResolvedTransport::Stdio {
                command: resolved_command,
                args: resolved_args,
                env: resolved_env,
            }
        }
        RegistryConfig::Http { url, headers, .. } => {
            let resolved_url = resolve_placeholders(url, &installed.input_values);

            // Resolve headers from registry
            let mut resolved_headers: HashMap<String, String> = headers
                .iter()
                .map(|(k, v)| (k.clone(), resolve_placeholders(v, &installed.input_values)))
                .collect();

            // Add user's extra headers
            resolved_headers.extend(installed.extra_headers.clone());

            ResolvedTransport::Http {
                url: resolved_url,
                headers: resolved_headers,
            }
        }
    }
}

fn apply_state_dir_env(
    resolved_env: &mut HashMap<String, String>,
    base_state_dir: Option<&Path>,
    installed: &InstalledServer,
) {
    if resolved_env.contains_key(MCP_STATE_DIR_ENV) {
        return;
    }

    let Some(base_state_dir) = base_state_dir else {
        return;
    };

    let state_dir = base_state_dir
        .join("stdio")
        .join(&installed.space_id)
        .join(&installed.server_id);

    resolved_env.insert(
        MCP_STATE_DIR_ENV.to_string(),
        state_dir.to_string_lossy().to_string(),
    );
}

/// Resolve placeholders like ${input:INPUT_NAME} in a string
fn resolve_placeholders(template: &str, input_values: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in input_values {
        result = result.replace(&format!("${{input:{}}}", key), value);
    }
    result
}
