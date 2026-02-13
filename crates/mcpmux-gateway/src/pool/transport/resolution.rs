//! Transport configuration resolution
//!
//! Handles building the actual runtime transport configuration from
//! the static registry definition and user-specific installation settings.

use super::ResolvedTransport;
use mcpmux_core::{InstalledServer, TransportConfig as RegistryConfig};
use std::collections::HashMap;
use std::path::Path;

const MCP_STATE_DIR_ENV: &str = "MCP_STATE_DIR";

/// Build a merged input_values map that includes defaults for any inputs
/// not explicitly provided by the user.
fn merge_input_defaults(
    registry_transport: &RegistryConfig,
    user_values: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut merged = user_values.clone();
    let metadata = registry_transport.metadata();
    for input in &metadata.inputs {
        if !merged.contains_key(&input.id) {
            if let Some(ref default_val) = input.default {
                tracing::debug!(
                    "[TransportResolution] Using default for input '{}': '{}'",
                    input.id,
                    default_val
                );
                merged.insert(input.id.clone(), default_val.clone());
            }
        }
    }
    merged
}

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

    // Merge user-provided values with defaults from input definitions
    let effective_values = merge_input_defaults(registry_transport, &installed.input_values);

    match registry_transport {
        RegistryConfig::Stdio {
            command, args, env, ..
        } => {
            let resolved_command = resolve_placeholders(command, &effective_values);
            let mut resolved_args: Vec<String> = args
                .iter()
                .map(|arg| resolve_placeholders(arg, &effective_values))
                .collect();

            // Append user's extra args
            resolved_args.extend(installed.args_append.clone());

            // Build env from registry + input values + env_overrides
            let mut resolved_env = HashMap::new();

            // 1. Start with registry env
            for (k, v) in env {
                let resolved_value = resolve_placeholders(v, &effective_values);
                tracing::debug!(
                    "[TransportResolution] Registry env: {}={} → {}",
                    k,
                    v,
                    resolved_value
                );
                resolved_env.insert(k.clone(), resolved_value);
            }

            // 2. Add input values (user-provided + defaults) directly as env vars
            tracing::debug!(
                "[TransportResolution] Adding {} input values as direct env vars",
                effective_values.len()
            );
            resolved_env.extend(effective_values.clone());

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
            let resolved_url = resolve_placeholders(url, &effective_values);

            // Resolve headers from registry
            let mut resolved_headers: HashMap<String, String> = headers
                .iter()
                .map(|(k, v)| (k.clone(), resolve_placeholders(v, &effective_values)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use mcpmux_core::{InputDefinition, TransportMetadata};

    fn make_installed(input_values: HashMap<String, String>) -> InstalledServer {
        InstalledServer::new("test-space", "test-server").with_inputs(input_values)
    }

    fn make_input(id: &str, default: Option<&str>) -> InputDefinition {
        InputDefinition {
            id: id.to_string(),
            label: id.to_string(),
            r#type: "text".to_string(),
            required: default.is_none(),
            secret: false,
            description: None,
            default: default.map(|s| s.to_string()),
            placeholder: None,
            obtain_url: None,
            obtain_instructions: None,
        }
    }

    #[test]
    fn test_default_used_when_user_provides_no_value() {
        let transport = RegistryConfig::Stdio {
            command: "node".to_string(),
            args: vec!["server.js".to_string()],
            env: HashMap::from([("LOG_LEVEL".to_string(), "${input:LOG_LEVEL}".to_string())]),
            metadata: TransportMetadata {
                inputs: vec![make_input("LOG_LEVEL", Some("info"))],
            },
        };

        let installed = make_installed(HashMap::new()); // No user values

        let resolved = build_transport_config(&transport, &installed, None);

        match resolved {
            ResolvedTransport::Stdio { env, .. } => {
                // Default should be used for placeholder resolution
                assert_eq!(env.get("LOG_LEVEL"), Some(&"info".to_string()));
            }
            _ => panic!("Expected Stdio transport"),
        }
    }

    #[test]
    fn test_user_value_overrides_default() {
        let transport = RegistryConfig::Stdio {
            command: "node".to_string(),
            args: vec![],
            env: HashMap::from([("LOG_LEVEL".to_string(), "${input:LOG_LEVEL}".to_string())]),
            metadata: TransportMetadata {
                inputs: vec![make_input("LOG_LEVEL", Some("info"))],
            },
        };

        let installed = make_installed(HashMap::from([(
            "LOG_LEVEL".to_string(),
            "debug".to_string(),
        )]));

        let resolved = build_transport_config(&transport, &installed, None);

        match resolved {
            ResolvedTransport::Stdio { env, .. } => {
                // User value should win over default
                assert_eq!(env.get("LOG_LEVEL"), Some(&"debug".to_string()));
            }
            _ => panic!("Expected Stdio transport"),
        }
    }

    #[test]
    fn test_default_resolves_in_args() {
        let transport = RegistryConfig::Stdio {
            command: "node".to_string(),
            args: vec!["--port".to_string(), "${input:PORT}".to_string()],
            env: HashMap::new(),
            metadata: TransportMetadata {
                inputs: vec![make_input("PORT", Some("8080"))],
            },
        };

        let installed = make_installed(HashMap::new());

        let resolved = build_transport_config(&transport, &installed, None);

        match resolved {
            ResolvedTransport::Stdio { args, .. } => {
                assert_eq!(args[0], "--port");
                assert_eq!(args[1], "8080");
            }
            _ => panic!("Expected Stdio transport"),
        }
    }

    #[test]
    fn test_default_resolves_in_command() {
        let transport = RegistryConfig::Stdio {
            command: "${input:BINARY_PATH}".to_string(),
            args: vec![],
            env: HashMap::new(),
            metadata: TransportMetadata {
                inputs: vec![make_input("BINARY_PATH", Some("/usr/local/bin/mcp"))],
            },
        };

        let installed = make_installed(HashMap::new());

        let resolved = build_transport_config(&transport, &installed, None);

        match resolved {
            ResolvedTransport::Stdio { command, .. } => {
                assert_eq!(command, "/usr/local/bin/mcp");
            }
            _ => panic!("Expected Stdio transport"),
        }
    }

    #[test]
    fn test_default_resolves_in_http_url() {
        let transport = RegistryConfig::Http {
            url: "https://api.example.com/${input:API_VERSION}/mcp".to_string(),
            headers: HashMap::new(),
            metadata: TransportMetadata {
                inputs: vec![make_input("API_VERSION", Some("v2"))],
            },
        };

        let installed = make_installed(HashMap::new());

        let resolved = build_transport_config(&transport, &installed, None);

        match resolved {
            ResolvedTransport::Http { url, .. } => {
                assert_eq!(url, "https://api.example.com/v2/mcp");
            }
            _ => panic!("Expected Http transport"),
        }
    }

    #[test]
    fn test_default_resolves_in_http_headers() {
        let transport = RegistryConfig::Http {
            url: "https://api.example.com/mcp".to_string(),
            headers: HashMap::from([("X-Api-Key".to_string(), "${input:API_KEY}".to_string())]),
            metadata: TransportMetadata {
                inputs: vec![make_input("API_KEY", Some("default-key"))],
            },
        };

        let installed = make_installed(HashMap::new());

        let resolved = build_transport_config(&transport, &installed, None);

        match resolved {
            ResolvedTransport::Http { headers, .. } => {
                assert_eq!(headers.get("X-Api-Key"), Some(&"default-key".to_string()));
            }
            _ => panic!("Expected Http transport"),
        }
    }

    #[test]
    fn test_multiple_defaults_some_overridden() {
        let transport = RegistryConfig::Stdio {
            command: "node".to_string(),
            args: vec![],
            env: HashMap::from([
                ("LOG_LEVEL".to_string(), "${input:LOG_LEVEL}".to_string()),
                ("PORT".to_string(), "${input:PORT}".to_string()),
                ("API_KEY".to_string(), "${input:API_KEY}".to_string()),
            ]),
            metadata: TransportMetadata {
                inputs: vec![
                    make_input("LOG_LEVEL", Some("info")),
                    make_input("PORT", Some("3000")),
                    make_input("API_KEY", None), // No default
                ],
            },
        };

        // User provides PORT and API_KEY, but not LOG_LEVEL
        let installed = make_installed(HashMap::from([
            ("PORT".to_string(), "9090".to_string()),
            ("API_KEY".to_string(), "secret123".to_string()),
        ]));

        let resolved = build_transport_config(&transport, &installed, None);

        match resolved {
            ResolvedTransport::Stdio { env, .. } => {
                // LOG_LEVEL: default used
                assert_eq!(env.get("LOG_LEVEL"), Some(&"info".to_string()));
                // PORT: user value wins
                assert_eq!(env.get("PORT"), Some(&"9090".to_string()));
                // API_KEY: user value used
                assert_eq!(env.get("API_KEY"), Some(&"secret123".to_string()));
            }
            _ => panic!("Expected Stdio transport"),
        }
    }

    #[test]
    fn test_no_default_leaves_placeholder_unresolved() {
        let transport = RegistryConfig::Stdio {
            command: "node".to_string(),
            args: vec![],
            env: HashMap::from([("API_KEY".to_string(), "${input:API_KEY}".to_string())]),
            metadata: TransportMetadata {
                inputs: vec![make_input("API_KEY", None)],
            },
        };

        let installed = make_installed(HashMap::new());

        let resolved = build_transport_config(&transport, &installed, None);

        match resolved {
            ResolvedTransport::Stdio { env, .. } => {
                // Without user value or default, placeholder stays unresolved in the env template
                assert_eq!(env.get("API_KEY"), Some(&"${input:API_KEY}".to_string()));
            }
            _ => panic!("Expected Stdio transport"),
        }
    }

    #[test]
    fn test_merge_input_defaults_only_fills_missing() {
        let transport = RegistryConfig::Stdio {
            command: "node".to_string(),
            args: vec![],
            env: HashMap::new(),
            metadata: TransportMetadata {
                inputs: vec![
                    make_input("A", Some("default_a")),
                    make_input("B", Some("default_b")),
                ],
            },
        };

        let user_values = HashMap::from([("A".to_string(), "user_a".to_string())]);

        let merged = merge_input_defaults(&transport, &user_values);

        assert_eq!(merged.get("A"), Some(&"user_a".to_string()));
        assert_eq!(merged.get("B"), Some(&"default_b".to_string()));
    }
}
