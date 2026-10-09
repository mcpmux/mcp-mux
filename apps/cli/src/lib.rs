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
            return daemon_restart(client, cli.socket.as_deref(), &cli.output).await;
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
            include_secrets,
        } => {
            let value = client
                .call(
                    Method::ConfigExport,
                    json!({
                        "format": format,
                        "server_id": server,
                        "space_id": space,
                        "include_secrets": include_secrets,
                    }),
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
                    write_private_file(path, content.as_bytes())
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
            let file = daemon_side_path(file)?;
            let call = |dry_run: bool| {
                json!({
                    "file": file.to_string_lossy(),
                    "space_id": space,
                    "dry_run": dry_run,
                })
            };
            if !dry_run {
                // Show what the import adds, changes and removes, and what
                // each server runs, before anything is written. In JSON mode
                // it goes to stderr, keeping stdout for the result.
                let plan = client.call(Method::ConfigImport, call(true)).await?;
                if !cli.yes {
                    let lines = config_import_plan(&plan, "will import:");
                    match cli.output {
                        OutputMode::Human => lines.iter().for_each(|l| println!("{l}")),
                        OutputMode::Json => lines.iter().for_each(|l| eprintln!("{l}")),
                    }
                }
                confirm(cli, "import these servers (new ones start enabled)")?;
            }
            let value = client.call(Method::ConfigImport, call(*dry_run)).await?;
            emit(&cli.output, &value, render_config_import)
        }
        ConfigCommand::Validate { file } => {
            let file = daemon_side_path(file)?;
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

/// The daemon opens files itself, relative to its own working directory
/// (`/` under systemd), so send it an absolute path resolved against the
/// operator's current directory.
fn daemon_side_path(path: &std::path::Path) -> Result<std::path::PathBuf> {
    std::path::absolute(path).map_err(|e| anyhow::anyhow!("cannot resolve {}: {e}", path.display()))
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

/// Write a file only its owner can read: exports can hold server commands
/// and URLs worth keeping private.
fn write_private_file(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // `mode` only applies to a new file; tighten an existing one too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents)
}

/// Whether a destructive command may go ahead without asking (`--yes`), must
/// ask, or must be refused (no terminal to ask on).
fn may_skip_prompt(yes: bool, interactive: bool, action: &str) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !interactive {
        bail!("refusing to {action} without --yes (no interactive terminal)");
    }
    Ok(false)
}

fn confirm(cli: &Cli, action: &str) -> Result<()> {
    if may_skip_prompt(cli.yes, std::io::stdin().is_terminal(), action)? {
        return Ok(());
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

/// Lines listing the env and header values an import keeps out of the
/// Space file (input ids per server, never values).
fn stored_as_inputs_lines(value: &Value, what: &str) -> Vec<String> {
    let Some(stored) = value.get("stored_as_inputs").and_then(Value::as_object) else {
        return Vec::new();
    };
    if stored.is_empty() {
        return Vec::new();
    }
    let mut lines =
        vec![format!("env/header values {what} as server inputs, not in the Space file:")];
    for (key, ids) in stored {
        let ids: Vec<&str> = as_array(ids)
            .into_iter()
            .filter_map(Value::as_str)
            .collect();
        lines.push(format!("  {key}: {}", ids.join(", ")));
    }
    lines
}

/// The lines describing an import plan (a dry-run response) under `heading`.
fn config_import_plan(value: &Value, heading: &str) -> Vec<String> {
    let mut lines = vec![heading.to_string()];
    for (mark, key) in [("+", "added"), ("~", "updated"), ("-", "removed")] {
        for id in as_array(value.get(key).unwrap_or(&Value::Null)) {
            if let Some(id) = id.as_str() {
                lines.push(format!("  {mark} {id}"));
            }
        }
    }
    if let Some(launches) = value.get("launches").and_then(Value::as_object) {
        lines.push("servers in the file run:".to_string());
        for (key, launch) in launches {
            lines.push(format!("  {key}: {}", launch.as_str().unwrap_or_default()));
        }
    }
    lines.extend(stored_as_inputs_lines(value, "will be stored encrypted"));
    lines
}

fn render_config_import(value: &Value) -> Result<()> {
    let dry = value
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if dry {
        for line in config_import_plan(value, "dry run - would import:") {
            println!("{line}");
        }
        return Ok(());
    }
    println!(
        "imported: {} added, {} updated, {} removed",
        as_array(value.get("added").unwrap_or(&Value::Null)).len(),
        as_array(value.get("updated").unwrap_or(&Value::Null)).len(),
        as_array(value.get("removed").unwrap_or(&Value::Null)).len(),
    );
    for line in stored_as_inputs_lines(value, "stored encrypted") {
        println!("{line}");
    }
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
// `daemon restart` — stop the running daemon and re-exec it with the same
// arguments. Unix-only because we read /proc/<pid>/{exe,cmdline,environ}.
// Refuses to touch a systemd-supervised daemon. Caller must close the
// control socket before invoking this.
// =============================================================================

/// How long the old daemon may take to drain and exit after SIGTERM.
#[cfg(unix)]
const RESTART_EXIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// How long the replacement daemon may take to answer on its control socket.
#[cfg(unix)]
const RESTART_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(unix)]
async fn daemon_restart(
    mut client: ControlClient,
    socket_override: Option<&std::path::Path>,
    mode: &OutputMode,
) -> Result<ExitCode> {
    let status = client.call(Method::Status, json!({})).await?;
    let pid: u32 = str_at(&status, "pid")
        .parse()
        .map_err(|_| anyhow::anyhow!("daemon status did not return a numeric pid"))?;
    // Signal only the process actually serving the socket, and only when the
    // OS says which one that is.
    let Some(peer) = client.peer_pid() else {
        bail!("cannot tell which process serves the control socket; refusing to signal it");
    };
    if u32::try_from(peer).ok() != Some(pid) {
        bail!(
            "the daemon reported pid {pid}, but the socket is served by pid {peer}; \
             refusing to signal either"
        );
    }
    let data_dir = str_at(&status, "data_dir");

    let proc = std::path::PathBuf::from(format!("/proc/{pid}"));
    if !proc.exists() {
        bail!("daemon pid {pid} is not running");
    }

    // A daemon that systemd started lives in its own `<unit>.service`
    // cgroup. Killing it and spawning an unsupervised copy would lose
    // supervision, so send the operator to systemctl instead. (Environment
    // markers such as INVOCATION_ID are inherited by ordinary terminal
    // sessions, so they cannot tell the two cases apart.)
    if let Some(unit) = supervising_systemd_unit(pid) {
        let scope = if unit.user { "--user " } else { "" };
        bail!(
            "daemon pid {pid} is managed by systemd unit {}; restart it with \
             `systemctl {scope}restart {}`",
            unit.name,
            unit.name
        );
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

    // SAFETY: plain signal delivery to a pid we just read from the daemon.
    if unsafe { libc::kill(pid as i32, libc::SIGTERM) } != 0 {
        bail!(
            "cannot signal daemon pid {pid}: {}",
            std::io::Error::last_os_error()
        );
    }

    // The data-dir lock and the gateway port are released only when the old
    // process exits; the control socket disappears long before that.
    if !wait_for_exit(pid, RESTART_EXIT_TIMEOUT).await {
        bail!(
            "daemon pid {pid} did not exit within {}s of SIGTERM; not starting a replacement",
            RESTART_EXIT_TIMEOUT.as_secs()
        );
    }

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    mcpmux_gateway::pool::transport::configure_child_process_platform(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn {}: {e}", exe.display()))?;
    let new_pid = child
        .id()
        .ok_or_else(|| anyhow::anyhow!("replacement daemon exited immediately"))?;

    // Report success only once the replacement answers as itself.
    let socket_path = match socket_override {
        Some(path) => path.to_path_buf(),
        None => mcpmux_runtime::control_socket_path(std::path::Path::new(&data_dir)),
    };
    let deadline = tokio::time::Instant::now() + RESTART_READY_TIMEOUT;
    loop {
        if let Some(exit) = child.try_wait()? {
            bail!(
                "replacement daemon exited during startup ({exit}); run `{} {}` in a \
                 terminal to see why",
                exe.display(),
                args.join(" ")
            );
        }
        if let Ok(mut fresh) = ControlClient::connect(None, Some(&socket_path)).await {
            if let Ok(status) = fresh.call(Method::Status, json!({})).await {
                if str_at(&status, "pid") == new_pid.to_string() {
                    break;
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "replacement daemon (pid {new_pid}) did not answer on {} within {}s",
                socket_path.display(),
                RESTART_READY_TIMEOUT.as_secs()
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let summary = json!({
        "previous_pid": pid,
        "new_pid": new_pid,
        "binary": exe.display().to_string(),
        "args": args,
    });
    emit(mode, &summary, render_restart)?;
    Ok(ExitCode::SUCCESS)
}

/// The systemd service unit supervising a process.
#[cfg(unix)]
struct SystemdUnit {
    name: String,
    /// Started by the per-user manager (`systemctl --user`).
    user: bool,
}

/// The `.service` unit whose cgroup holds `pid`, unless this CLI runs in
/// that same cgroup (then the daemon was started by hand from this session,
/// e.g. inside a terminal multiplexer that itself runs as a service).
#[cfg(unix)]
fn supervising_systemd_unit(pid: u32) -> Option<SystemdUnit> {
    let daemon = unified_cgroup(&format!("/proc/{pid}/cgroup"))?;
    if unified_cgroup("/proc/self/cgroup").as_deref() == Some(daemon.as_str()) {
        return None;
    }
    let leaf = daemon.rsplit('/').next()?;
    leaf.ends_with(".service").then(|| SystemdUnit {
        name: leaf.to_string(),
        user: daemon.contains("/user@"),
    })
}

/// The cgroup v2 path (`0::<path>`) from a `/proc/<pid>/cgroup` file.
#[cfg(unix)]
fn unified_cgroup(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("0::").map(str::to_string))
}

/// Wait until `pid` has exited. A zombie (exited, not yet reaped by its
/// parent) counts as exited: it holds no locks, sockets or ports.
#[cfg(unix)]
async fn wait_for_exit(pid: u32, timeout: std::time::Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if !process_alive(pid) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // Format: `<pid> (<comm>) <state> ...`; comm may contain spaces/parens.
    let state = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.trim_start().chars().next());
    !matches!(state, Some('Z') | Some('X') | None)
}

#[cfg(not(unix))]
async fn daemon_restart(
    _client: ControlClient,
    _socket_override: Option<&std::path::Path>,
    _mode: &OutputMode,
) -> Result<ExitCode> {
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

#[cfg(test)]
mod tests {

    #[cfg(unix)]
    #[test]
    fn export_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("mcpmux-cli-out-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("space.json");
        std::fs::write(&existing, "old").unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o644)).unwrap();
        let fresh = dir.join("fresh.json");
        let _ = std::fs::remove_file(&fresh);
        for file in [&existing, &fresh] {
            write_private_file(file, b"{}").unwrap();
            assert_eq!(std::fs::read_to_string(file).unwrap(), "{}");
            let mode = std::fs::metadata(file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", file.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    #[test]
    fn destructive_commands_without_a_terminal_need_yes() {
        assert!(may_skip_prompt(true, false, "import").unwrap());
        assert!(!may_skip_prompt(false, true, "import").unwrap());
        let err = may_skip_prompt(false, false, "import").unwrap_err();
        assert!(err.to_string().contains("without --yes"), "{err}");
    }

    #[test]
    fn the_import_plan_names_what_runs() {
        let plan = serde_json::json!({
            "dry_run": true, "added": ["a"], "updated": [], "removed": ["b"],
            "launches": {"a": "npx -y pkg  [env: NODE_OPTIONS]"}
        });
        assert_eq!(
            config_import_plan(&plan, "will import:"),
            [
                "will import:",
                "  + a",
                "  - b",
                "servers in the file run:",
                "  a: npx -y pkg  [env: NODE_OPTIONS]",
            ]
        );
    }

    #[test]
    fn daemon_side_path_resolves_relative_paths_against_the_cli_cwd() {
        let resolved = daemon_side_path(std::path::Path::new("servers.json")).unwrap();
        assert!(resolved.is_absolute());
        assert_eq!(
            resolved,
            std::env::current_dir().unwrap().join("servers.json")
        );

        let absolute = std::env::temp_dir().join("servers.json");
        assert_eq!(daemon_side_path(&absolute).unwrap(), absolute);
    }
}
