//! `mcpmux-cli` operator CLI implementation.

pub mod args;
pub mod client;

use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::{bail, Result};
use mcpmux_control::Method;
use serde_json::{json, Value};

use args::{
    BaseDirsCommand, Cli, ClientsCommand, Command, ConfigCommand, DaemonCommand,
    FeatureSetsCommand, OutputMode, PortCommand, RegistryCommand, ServersCommand, SpacesCommand,
    WorkspaceCommand, WorkspacesCommand,
};
use client::ControlClient;

/// Run the CLI, returning the process exit code.
pub async fn run(cli: Cli) -> Result<ExitCode> {
    let mut client = ControlClient::connect(cli.data_dir.as_deref(), cli.socket.as_deref()).await?;

    match &cli.command {
        Command::Daemon {
            command: DaemonCommand::Status,
        } => {
            let value = client.call(Method::Status, json!({})).await?;
            emit(&cli.output, &value, render_status)?;
        }
        Command::Daemon {
            command: DaemonCommand::Port(sub),
        } => match sub {
            PortCommand::Get => {
                let value = client.call(Method::PortGet, json!({})).await?;
                emit(&cli.output, &value, render_port)?;
            }
            PortCommand::Set { port } => {
                let value = client
                    .call(Method::PortSet, json!({ "port": port }))
                    .await?;
                emit(&cli.output, &value, render_port)?;
            }
            PortCommand::Clear => {
                let value = client.call(Method::PortClear, json!({})).await?;
                emit(&cli.output, &value, render_port)?;
            }
        },
        Command::Daemon {
            command: DaemonCommand::Restart,
        } => {
            return daemon_restart(client, &cli.output).await;
        }
        Command::Health => {
            let value = client.call(Method::Health, json!({})).await?;
            emit(&cli.output, &value, render_health)?;
        }
        Command::Doctor => {
            let value = client.call(Method::Doctor, json!({})).await?;
            let healthy = emit_doctor(&cli.output, &value)?;
            if !healthy {
                return Ok(ExitCode::from(2));
            }
        }
        Command::Logs {
            server,
            space,
            limit,
            level,
            follow,
        } => {
            if *follow {
                run_logs_follow(&mut client, server, space.as_deref()).await?;
            } else {
                let value = client
                    .call(
                        Method::LogsList,
                        json!({
                            "server_id": server,
                            "space_id": space,
                            "limit": limit,
                            "level": level,
                        }),
                    )
                    .await?;
                emit(&cli.output, &value, render_logs)?;
            }
        }
        Command::Spaces { command } => run_spaces(&mut client, &cli, command).await?,
        Command::FeatureSets { command } => run_feature_sets(&mut client, &cli, command).await?,
        Command::Servers { command } => run_servers(&mut client, &cli, command).await?,
        Command::Registry { command } => run_registry(&mut client, &cli, command).await?,
        Command::Workspaces { command } => run_workspaces(&mut client, &cli, command).await?,
        Command::Clients { command } => run_clients(&mut client, &cli, command).await?,
        Command::Config { command } => run_config(&mut client, &cli, command).await?,
        Command::Workspace { command } => run_workspace(&mut client, &cli, command).await?,
    }

    Ok(ExitCode::SUCCESS)
}

async fn run_spaces(client: &mut ControlClient, cli: &Cli, command: &SpacesCommand) -> Result<()> {
    match command {
        SpacesCommand::List => {
            let value = client.call(Method::SpacesList, json!({})).await?;
            emit(&cli.output, &value, render_spaces)
        }
        SpacesCommand::Create { name, icon } => {
            let value = client
                .call(Method::SpacesCreate, json!({"name": name, "icon": icon}))
                .await?;
            emit(&cli.output, &value, render_space)
        }
        SpacesCommand::Delete { space_id } => {
            confirm(cli, &format!("delete space {space_id}"))?;
            let value = client
                .call(Method::SpacesDelete, json!({"space_id": space_id}))
                .await?;
            emit(&cli.output, &value, render_mutation)
        }
        SpacesCommand::SetDefault { space_id } => {
            let value = client
                .call(Method::SpacesSetDefault, json!({"space_id": space_id}))
                .await?;
            emit(&cli.output, &value, render_mutation)
        }
        SpacesCommand::BaseDirs { command } => match command {
            BaseDirsCommand::List { space_id } => {
                let value = client
                    .call(Method::BaseDirsList, json!({"space_id": space_id}))
                    .await?;
                emit(&cli.output, &value, render_base_dirs)
            }
            BaseDirsCommand::Add { space_id, path } => {
                let value = client
                    .call(
                        Method::BaseDirsAdd,
                        json!({"space_id": space_id, "path": path}),
                    )
                    .await?;
                emit(&cli.output, &value, render_base_dir)
            }
            BaseDirsCommand::Remove { id } => {
                confirm(cli, &format!("remove base dir {id}"))?;
                let value = client
                    .call(Method::BaseDirsRemove, json!({"id": id}))
                    .await?;
                emit(&cli.output, &value, render_mutation)
            }
        },
    }
}

async fn run_feature_sets(
    client: &mut ControlClient,
    cli: &Cli,
    command: &FeatureSetsCommand,
) -> Result<()> {
    match command {
        FeatureSetsCommand::List { space } => {
            let value = client
                .call(Method::FeatureSetsList, json!({"space_id": space}))
                .await?;
            emit(&cli.output, &value, render_feature_sets)
        }
        FeatureSetsCommand::Get { id } => {
            let value = client
                .call(Method::FeatureSetsGet, json!({"id": id}))
                .await?;
            emit(&cli.output, &value, render_feature_set_detail)
        }
        FeatureSetsCommand::Create {
            name,
            space,
            description,
            icon,
        } => {
            let value = client
                .call(
                    Method::FeatureSetsCreate,
                    json!({
                        "space_id": space,
                        "name": name,
                        "description": description,
                        "icon": icon,
                    }),
                )
                .await?;
            emit(&cli.output, &value, render_feature_set)
        }
        FeatureSetsCommand::Update {
            id,
            name,
            description,
            icon,
        } => {
            let value = client
                .call(
                    Method::FeatureSetsUpdate,
                    json!({
                        "id": id,
                        "name": name,
                        "description": description,
                        "icon": icon,
                    }),
                )
                .await?;
            emit(&cli.output, &value, render_feature_set)
        }
        FeatureSetsCommand::Delete { id } => {
            confirm(cli, &format!("delete feature set {id}"))?;
            let value = client
                .call(Method::FeatureSetsDelete, json!({"id": id}))
                .await?;
            emit(&cli.output, &value, render_mutation)
        }
        FeatureSetsCommand::Include {
            id,
            server,
            space,
            feature_type,
            name,
        } => {
            let value = client
                .call(
                    Method::FeatureSetsAddMember,
                    json!({
                        "feature_set_id": id,
                        "server_id": server,
                        "space_id": space,
                        "feature_type": feature_type,
                        "name": name,
                    }),
                )
                .await?;
            emit(&cli.output, &value, render_mutation)
        }
        FeatureSetsCommand::Remove {
            id,
            feature,
            server,
            space,
        } => {
            let value = client
                .call(
                    Method::FeatureSetsRemoveMember,
                    json!({
                        "feature_set_id": id,
                        "feature_id": feature,
                        "server_id": server,
                        "space_id": space,
                        "by_name": server.is_some(),
                    }),
                )
                .await?;
            emit(&cli.output, &value, render_mutation)
        }
    }
}

async fn run_servers(
    client: &mut ControlClient,
    cli: &Cli,
    command: &ServersCommand,
) -> Result<()> {
    match command {
        ServersCommand::List { space } => {
            let value = client
                .call(Method::ServersList, json!({"space_id": space}))
                .await?;
            emit(&cli.output, &value, render_servers)
        }
        ServersCommand::Inspect { server_id } => {
            let value = client
                .call(Method::ServersInspect, json!({"server_id": server_id}))
                .await?;
            emit(&cli.output, &value, render_servers_inspect)
        }
        ServersCommand::Features {
            server_id,
            space,
            feature_type,
        } => {
            let value = client
                .call(
                    Method::ServersFeatures,
                    json!({
                        "server_id": server_id,
                        "space_id": space,
                        "feature_type": feature_type,
                    }),
                )
                .await?;
            emit(&cli.output, &value, render_server_features)
        }
        ServersCommand::Add { server_id, space } => {
            let value = client
                .call(
                    Method::ServersAdd,
                    json!({"server_id": server_id, "space_id": space, "inputs": {}}),
                )
                .await?;
            emit(&cli.output, &value, render_installed_ref)
        }
        ServersCommand::Configure {
            server_id,
            space,
            file,
        } => {
            let raw = std::fs::read_to_string(file)
                .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", file.display()))?;
            let parsed: Value = serde_json::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", file.display()))?;
            let value = client
                .call(
                    Method::ServersConfigure,
                    json!({
                        "server_id": server_id,
                        "space_id": space,
                        "inputs": parsed.get("inputs"),
                        "env": parsed.get("env"),
                        "args": parsed.get("args"),
                        "headers": parsed.get("headers"),
                    }),
                )
                .await?;
            emit(&cli.output, &value, render_installed_ref)
        }
        ServersCommand::Enable { server_id, space } => {
            let value = client
                .call(
                    Method::ServersEnable,
                    json!({"server_id": server_id, "space_id": space}),
                )
                .await?;
            emit(&cli.output, &value, render_servers_status)
        }
        ServersCommand::Disable { server_id, space } => {
            let value = client
                .call(
                    Method::ServersDisable,
                    json!({"server_id": server_id, "space_id": space}),
                )
                .await?;
            emit(&cli.output, &value, render_servers_status)
        }
        ServersCommand::Remove { server_id, space } => {
            confirm(cli, &format!("uninstall server {server_id}"))?;
            let value = client
                .call(
                    Method::ServersRemove,
                    json!({"server_id": server_id, "space_id": space}),
                )
                .await?;
            emit(&cli.output, &value, render_mutation)
        }
        ServersCommand::Auth { server_id, space } => {
            let value = client
                .call(
                    Method::ServersAuthenticate,
                    json!({"server_id": server_id, "space_id": space}),
                )
                .await?;
            emit(&cli.output, &value, render_auth_url)
        }
    }
}

async fn run_registry(
    client: &mut ControlClient,
    cli: &Cli,
    command: &RegistryCommand,
) -> Result<()> {
    match command {
        RegistryCommand::List {
            query,
            category,
            refresh,
        } => {
            let value = client
                .call(
                    Method::RegistryList,
                    json!({"query": query, "category": category, "refresh": refresh}),
                )
                .await?;
            emit(&cli.output, &value, render_registry)
        }
        RegistryCommand::Search {
            query,
            category,
            refresh,
        } => {
            let value = client
                .call(
                    Method::RegistrySearch,
                    json!({"query": query, "category": category, "refresh": refresh}),
                )
                .await?;
            emit(&cli.output, &value, render_registry)
        }
    }
}

async fn run_workspaces(
    client: &mut ControlClient,
    cli: &Cli,
    command: &WorkspacesCommand,
) -> Result<()> {
    match command {
        WorkspacesCommand::List { space } => {
            let value = client
                .call(Method::WorkspacesList, json!({"space_id": space}))
                .await?;
            emit(&cli.output, &value, render_bindings)
        }
        WorkspacesCommand::Bind {
            path,
            space,
            feature_sets,
            binding_type,
        } => {
            let value = client
                .call(
                    Method::WorkspacesBind,
                    json!({
                        "path": path,
                        "space_id": space,
                        "feature_set_ids": feature_sets,
                        "binding_type": binding_type,
                    }),
                )
                .await?;
            emit(&cli.output, &value, render_binding)
        }
        WorkspacesCommand::Unbind { id } => {
            confirm(cli, &format!("remove binding {id}"))?;
            let value = client
                .call(Method::WorkspacesUnbind, json!({"id": id}))
                .await?;
            emit(&cli.output, &value, render_mutation)
        }
    }
}

async fn run_clients(
    client: &mut ControlClient,
    cli: &Cli,
    command: &ClientsCommand,
) -> Result<()> {
    match command {
        ClientsCommand::List => {
            let value = client.call(Method::ClientsList, json!({})).await?;
            emit(&cli.output, &value, render_clients)
        }
        ClientsCommand::Create { name, client_type } => {
            let value = client
                .call(
                    Method::ClientsCreate,
                    json!({"name": name, "client_type": client_type}),
                )
                .await?;
            emit(&cli.output, &value, render_client_created)
        }
        ClientsCommand::Delete { id } => {
            confirm(cli, &format!("delete client {id}"))?;
            let value = client
                .call(Method::ClientsDelete, json!({"id": id}))
                .await?;
            emit(&cli.output, &value, render_mutation)
        }
    }
}

async fn run_config(client: &mut ControlClient, cli: &Cli, command: &ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Export {
            format,
            server,
            space,
        } => {
            let value = client
                .call(
                    Method::ConfigExport,
                    json!({"format": format, "server_id": server, "space_id": space}),
                )
                .await?;
            emit(&cli.output, &value, render_config_export)
        }
        ConfigCommand::ExportSpace { space, out } => {
            let value = client
                .call(Method::ConfigExportSpace, json!({"space_id": space}))
                .await?;
            match out {
                Some(path) => {
                    let content = value.get("content").and_then(Value::as_str).unwrap_or("");
                    std::fs::write(path, content)
                        .map_err(|e| anyhow::anyhow!("cannot write {}: {e}", path.display()))?;
                    if cli.output == OutputMode::Human {
                        println!(
                            "wrote {} server(s) to {}",
                            value
                                .get("server_count")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                            path.display()
                        );
                    } else {
                        println!("{}", serde_json::to_string_pretty(&value)?);
                    }
                    Ok(())
                }
                None => emit(&cli.output, &value, render_config_export_space),
            }
        }
        ConfigCommand::Import {
            file,
            space,
            dry_run,
        } => {
            let value = client
                .call(
                    Method::ConfigImport,
                    json!({
                        "file": file.to_string_lossy(),
                        "space_id": space,
                        "dry_run": dry_run,
                    }),
                )
                .await?;
            emit(&cli.output, &value, render_config_import)
        }
        ConfigCommand::Validate { file } => {
            let value = client
                .call(
                    Method::ConfigValidate,
                    json!({"file": file.to_string_lossy()}),
                )
                .await?;
            emit(&cli.output, &value, render_config_validate)
        }
    }
}

async fn run_workspace(
    client: &mut ControlClient,
    cli: &Cli,
    command: &WorkspaceCommand,
) -> Result<()> {
    match command {
        WorkspaceCommand::Config {
            path,
            client: target,
        } => {
            let value = client
                .call(
                    Method::WorkspaceConfig,
                    json!({"path": path, "client": target}),
                )
                .await?;
            emit(&cli.output, &value, render_workspace_config)
        }
    }
}

async fn run_logs_follow(
    client: &mut ControlClient,
    server: &str,
    space: Option<&str>,
) -> Result<()> {
    // Print a bounded initial snapshot, then stream domain events.
    let initial = client
        .call(
            Method::LogsList,
            json!({"server_id": server, "space_id": space, "limit": 50}),
        )
        .await?;
    render_logs(&initial)?;

    eprintln!("following daemon events (Ctrl-C to stop)...");
    client
        .subscribe(|envelope| {
            let event = serde_json::to_string(&envelope.event).unwrap_or_default();
            println!("{event}");
        })
        .await
}

// =============================================================================
// Output
// =============================================================================

fn emit(mode: &OutputMode, value: &Value, human: fn(&Value) -> Result<()>) -> Result<()> {
    match mode {
        OutputMode::Json => {
            println!("{}", serde_json::to_string_pretty(value)?);
            Ok(())
        }
        OutputMode::Human => human(value),
    }
}

fn confirm(cli: &Cli, action: &str) -> Result<()> {
    if cli.yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        bail!("refusing to {action} without --yes (no interactive terminal)");
    }
    eprint!("About to {action}. Continue? [y/N] ");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        bail!("aborted");
    }
    Ok(())
}

fn render_status(value: &Value) -> Result<()> {
    println!(
        "daemon {} (pid {})  gateway {}  data {}",
        str_at(value, "version"),
        str_at(value, "pid"),
        str_at(value, "gateway_url"),
        str_at(value, "data_dir"),
    );
    println!(
        "servers: {} enabled, {} connected   auth_disabled={}",
        str_at(value, "enabled_servers"),
        str_at(value, "connected_servers"),
        str_at(value, "auth_disabled"),
    );
    Ok(())
}

fn render_port(value: &Value) -> Result<()> {
    let persisted = match value.get("persisted_port") {
        Some(Value::Number(n)) => format!("{}", n),
        _ => "<unset>".to_string(),
    };
    println!(
        "persisted_port: {}  active_port: {}  default_port: {}",
        persisted,
        str_at(value, "active_port"),
        str_at(value, "default_port"),
    );
    Ok(())
}

/// Print a doctor report and return whether it is healthy.
fn emit_doctor(mode: &OutputMode, value: &Value) -> Result<bool> {
    let healthy = value
        .get("healthy")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match mode {
        OutputMode::Json => {
            println!("{}", serde_json::to_string_pretty(value)?);
        }
        OutputMode::Human => {
            for check in as_array(value.get("checks").unwrap_or(&Value::Null)) {
                let marker = match str_at(check, "status").as_str() {
                    "ok" => "ok  ",
                    "warn" => "warn",
                    "fail" => "FAIL",
                    _ => "skip",
                };
                println!(
                    "[{marker}] {}: {}",
                    str_at(check, "id"),
                    str_at(check, "message")
                );
                let hint = str_at(check, "hint");
                if !hint.is_empty() {
                    println!("       hint: {hint}");
                }
            }
            println!("overall: {}", if healthy { "healthy" } else { "unhealthy" });
        }
    }
    Ok(healthy)
}

fn render_health(value: &Value) -> Result<()> {
    println!(
        "{} (version {}, pid {})",
        str_at(value, "status"),
        str_at(value, "version"),
        str_at(value, "pid"),
    );
    Ok(())
}

fn render_logs(value: &Value) -> Result<()> {
    for entry in as_array(value) {
        println!(
            "{} [{}] {}: {}",
            str_at(entry, "timestamp"),
            str_at(entry, "level"),
            str_at(entry, "source"),
            str_at(entry, "message"),
        );
    }
    Ok(())
}

fn render_spaces(value: &Value) -> Result<()> {
    for space in as_array(value) {
        let default = if space.get("is_default").and_then(Value::as_bool) == Some(true) {
            " (default)"
        } else {
            ""
        };
        println!(
            "{}  {}{}",
            str_at(space, "id"),
            str_at(space, "name"),
            default
        );
    }
    Ok(())
}

fn render_space(value: &Value) -> Result<()> {
    println!("{}  {}", str_at(value, "id"), str_at(value, "name"));
    Ok(())
}

fn render_base_dirs(value: &Value) -> Result<()> {
    for dir in as_array(value) {
        println!("{}  {}", str_at(dir, "id"), str_at(dir, "path"));
    }
    Ok(())
}

fn render_base_dir(value: &Value) -> Result<()> {
    println!("{}  {}", str_at(value, "id"), str_at(value, "path"));
    Ok(())
}

fn render_feature_sets(value: &Value) -> Result<()> {
    for set in as_array(value) {
        println!(
            "{}  {}  [space {}]",
            str_at(set, "id"),
            str_at(set, "name"),
            str_at(set, "space_id"),
        );
    }
    Ok(())
}

fn render_feature_set(value: &Value) -> Result<()> {
    println!("{}  {}", str_at(value, "id"), str_at(value, "name"));
    Ok(())
}

fn render_feature_set_detail(value: &Value) -> Result<()> {
    println!("{}  {}", str_at(value, "id"), str_at(value, "name"));
    let members = value.get("members").map(as_array).unwrap_or_default();
    println!("members: {}", members.len());
    for member in members {
        println!(
            "  {} {} {}",
            str_at(member, "mode"),
            str_at(member, "member_type"),
            str_at(member, "member_id"),
        );
    }
    Ok(())
}

fn render_servers(value: &Value) -> Result<()> {
    for server in as_array(value) {
        println!(
            "{}  {}  enabled={} status={} space={}",
            str_at(server, "server_id"),
            str_at(server, "name"),
            str_at(server, "enabled"),
            str_at(server, "status"),
            str_at(server, "space_id"),
        );
    }
    Ok(())
}

fn render_servers_inspect(value: &Value) -> Result<()> {
    for server in as_array(value) {
        println!(
            "{}  {}  space={}  enabled={}  transport={}",
            str_at(server, "server_id"),
            str_at(server, "name"),
            str_at(server, "space_id"),
            str_at(server, "enabled"),
            str_at(server, "transport"),
        );
        println!(
            "  configured inputs: {}",
            join_keys(server, "configured_inputs")
        );
        println!("  env overrides: {}", join_keys(server, "env_overrides"));
        println!("  extra headers: {}", join_keys(server, "extra_headers"));
    }
    Ok(())
}

fn render_server_features(value: &Value) -> Result<()> {
    for feature in as_array(value) {
        println!(
            "{}  {}  {}  available={}",
            str_at(feature, "feature_type"),
            str_at(feature, "feature_name"),
            str_at(feature, "id"),
            str_at(feature, "available"),
        );
    }
    Ok(())
}

fn render_installed_ref(value: &Value) -> Result<()> {
    println!(
        "{}  {}  enabled={}",
        str_at(value, "server_id"),
        str_at(value, "id"),
        str_at(value, "enabled"),
    );
    Ok(())
}

fn render_servers_status(value: &Value) -> Result<()> {
    println!(
        "{}  enabled={} status={}",
        str_at(value, "server_id"),
        str_at(value, "enabled"),
        str_at(value, "status"),
    );
    Ok(())
}

fn render_auth_url(value: &Value) -> Result<()> {
    println!("Open this URL to authorize {}:", str_at(value, "server_id"));
    println!("{}", str_at(value, "authorization_url"));
    Ok(())
}

fn render_bindings(value: &Value) -> Result<()> {
    for binding in as_array(value) {
        let feature_sets = binding
            .get("feature_set_ids")
            .map(as_array)
            .unwrap_or_default()
            .into_iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{}  {}  type={} space={} feature_sets=[{}]",
            str_at(binding, "id"),
            str_at(binding, "workspace_root"),
            str_at(binding, "binding_type"),
            str_at(binding, "space_id"),
            feature_sets,
        );
    }
    Ok(())
}

fn render_binding(value: &Value) -> Result<()> {
    println!(
        "{}  {}  space={}",
        str_at(value, "id"),
        str_at(value, "workspace_root"),
        str_at(value, "space_id"),
    );
    Ok(())
}

fn render_clients(value: &Value) -> Result<()> {
    for client in as_array(value) {
        let locked = if str_at(client, "locked_space_id").is_empty() {
            String::new()
        } else {
            format!(" locked_to={}", str_at(client, "locked_space_id"))
        };
        println!(
            "{}  {}  type={} approved={}{}",
            str_at(client, "id"),
            str_at(client, "name"),
            str_at(client, "type"),
            str_at(client, "approved"),
            locked,
        );
    }
    Ok(())
}

fn render_client_created(value: &Value) -> Result<()> {
    println!(
        "{}  {}  type={}",
        str_at(value, "id"),
        str_at(value, "name"),
        str_at(value, "type")
    );
    if let Some(key) = value.get("api_key").and_then(Value::as_str) {
        println!("API key (shown once): {key}");
    }
    Ok(())
}

fn render_config_export(value: &Value) -> Result<()> {
    print!("{}", str_at(value, "content"));
    if !str_at(value, "content").ends_with('\n') {
        println!();
    }
    Ok(())
}

fn render_config_export_space(value: &Value) -> Result<()> {
    print!("{}", str_at(value, "content"));
    if !str_at(value, "content").ends_with('\n') {
        println!();
    }
    Ok(())
}

fn render_config_import(value: &Value) -> Result<()> {
    let dry = value
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if dry {
        println!("dry run - would import:");
        for id in as_array(value.get("added").unwrap_or(&Value::Null)) {
            if let Some(id) = id.as_str() {
                println!("  + {id}");
            }
        }
        return Ok(());
    }
    println!(
        "imported: {} added, {} updated, {} removed",
        as_array(value.get("added").unwrap_or(&Value::Null)).len(),
        as_array(value.get("updated").unwrap_or(&Value::Null)).len(),
        as_array(value.get("removed").unwrap_or(&Value::Null)).len(),
    );
    let backup = str_at(value, "backup");
    if !backup.is_empty() {
        println!("backup: {backup}");
    }
    Ok(())
}

fn render_config_validate(value: &Value) -> Result<()> {
    let valid = value.get("valid").and_then(Value::as_bool).unwrap_or(false);
    println!("valid: {valid}");
    for server in as_array(value.get("servers").unwrap_or(&Value::Null)) {
        if let Some(id) = server.as_str() {
            println!("  {id}");
        }
    }
    for warning in as_array(value.get("warnings").unwrap_or(&Value::Null)) {
        if let Some(w) = warning.as_str() {
            println!("warning: {w}");
        }
    }
    Ok(())
}

fn render_workspace_config(value: &Value) -> Result<()> {
    print!("{}", str_at(value, "content"));
    if !str_at(value, "content").ends_with('\n') {
        println!();
    }
    Ok(())
}

fn render_registry(value: &Value) -> Result<()> {
    for server in as_array(value) {
        let installed = if server.get("installed").and_then(Value::as_bool) == Some(true) {
            " [installed]"
        } else {
            ""
        };
        let publisher = str_at(server, "publisher");
        let by = if publisher.is_empty() {
            String::new()
        } else {
            format!(" by {publisher}")
        };
        println!(
            "{}  {}{}  ({}, auth={}){}",
            str_at(server, "id"),
            str_at(server, "name"),
            by,
            str_at(server, "transport"),
            str_at(server, "auth"),
            installed,
        );
    }
    Ok(())
}

fn render_mutation(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

// =============================================================================
// `daemon restart` — kill the running daemon and re-exec it with the same
// arguments. Unix-only because we read /proc/<pid>/{exe,cmdline}. Caller must
// close the control socket before invoking this.
// =============================================================================

#[cfg(unix)]
async fn daemon_restart(mut client: ControlClient, mode: &OutputMode) -> Result<ExitCode> {
    let status = client.call(Method::Status, json!({})).await?;
    let pid: u32 = str_at(&status, "pid")
        .parse()
        .map_err(|_| anyhow::anyhow!("daemon status did not return a numeric pid"))?;
    let data_dir = str_at(&status, "data_dir");

    let proc = std::path::PathBuf::from(format!("/proc/{pid}"));
    if !proc.exists() {
        bail!("daemon pid {pid} is not running");
    }

    let exe = std::fs::read_link(proc.join("exe"))
        .map_err(|e| anyhow::anyhow!("cannot resolve /proc/{pid}/exe: {e}"))?;
    let cmdline = std::fs::read(proc.join("cmdline"))
        .map_err(|e| anyhow::anyhow!("cannot read /proc/{pid}/cmdline: {e}"))?;
    let raw_args: Vec<String> = cmdline
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    let args: Vec<String> = raw_args.iter().skip(1).cloned().collect();

    drop(client);

    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }

    let socket_path = client_socket_path(&data_dir);
    for _ in 0..50 {
        if !socket_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if socket_path.exists() {
        bail!(
            "daemon did not release {} within 5s; refusing to spawn a replacement \
             (another process may own the socket)",
            socket_path.display()
        );
    }

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn {}: {e}", exe.display()))?;

    let summary = json!({
        "previous_pid": pid,
        "new_pid": child.id(),
        "binary": exe.display().to_string(),
        "args": args,
    });
    emit(mode, &summary, render_restart)?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(unix))]
async fn daemon_restart(_client: ControlClient, _mode: &OutputMode) -> Result<ExitCode> {
    bail!(
        "daemon restart is only supported on unix; use the supervisor (systemd, launchd) instead"
    );
}

#[cfg(unix)]
fn render_restart(value: &Value) -> Result<()> {
    println!(
        "restarted: previous_pid={} new_pid={} binary={}",
        str_at(value, "previous_pid"),
        str_at(value, "new_pid"),
        str_at(value, "binary"),
    );
    Ok(())
}

#[cfg(unix)]
fn client_socket_path(data_dir: &str) -> std::path::PathBuf {
    use std::path::PathBuf;
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("mcpmux").join("control.sock");
        }
    }
    PathBuf::from(data_dir).join("control.sock")
}

fn as_array(value: &Value) -> Vec<&Value> {
    value
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn str_at(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn join_keys(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default()
}
