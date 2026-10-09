//! Linux user-service installation for mcpmuxd.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context};

use crate::args::Args;

const UNIT_NAME: &str = "mcpmux.service";

pub fn install(args: &Args) -> anyhow::Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("mcpmuxd service install is currently supported only on Linux");
    }
    if args.auth_disabled {
        bail!(
            "--auth-disabled is for a one-off local run and is never installed as a service; \
             install without it (create an API-key client for headless clients instead)"
        );
    }

    let executable =
        env::current_exe().context("could not determine the mcpmuxd executable path")?;
    let unit_path = user_unit_dir()?.join(UNIT_NAME);
    let unit = render_unit(&ServiceConfig::from_args(args, executable));

    fs::create_dir_all(
        unit_path
            .parent()
            .expect("a systemd unit path always has a parent directory"),
    )
    .with_context(|| format!("could not create {}", unit_path.display()))?;
    write_unit(&unit_path, &unit)?;

    systemctl(["daemon-reload"])?;
    systemctl(["enable", UNIT_NAME])?;
    systemctl(["restart", UNIT_NAME])?;

    println!("Installed and started {}", unit_path.display());
    println!("Check status: systemctl --user status {UNIT_NAME}");
    println!("Follow logs: journalctl --user -u {UNIT_NAME} -f");
    Ok(())
}

#[derive(Debug, Clone)]
struct ServiceConfig {
    executable: PathBuf,
    data_dir: PathBuf,
    port: Option<u16>,
    registry_url: Option<String>,
    key_provider: &'static str,
    log_dir: Option<PathBuf>,
    log_filter: String,
    public_base_url: Option<String>,
}

impl ServiceConfig {
    fn from_args(args: &Args, executable: PathBuf) -> Self {
        Self {
            executable,
            data_dir: args
                .data_dir
                .clone()
                .unwrap_or_else(mcpmux_runtime::default_data_dir),
            port: args.port,
            registry_url: args.registry_url.clone(),
            key_provider: args.key_provider.as_str(),
            log_dir: args.log_dir.clone(),
            log_filter: args.log_filter.clone(),
            public_base_url: args.public_base_url.clone(),
        }
    }
}

fn user_unit_dir() -> anyhow::Result<PathBuf> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("systemd/user"));
    }

    let home = env::var_os("HOME").context("HOME is required to install a systemd user service")?;
    Ok(PathBuf::from(home).join(".config/systemd/user"))
}

fn render_unit(config: &ServiceConfig) -> String {
    let mut command = vec![
        quote_systemd_arg(&config.executable),
        "--data-dir".to_string(),
        quote_systemd_arg(&config.data_dir),
        "--key-provider".to_string(),
        config.key_provider.to_string(),
        "--log-filter".to_string(),
        quote_systemd_arg(&config.log_filter),
    ];
    if let Some(port) = config.port {
        command.extend(["--port".to_string(), port.to_string()]);
    }
    if let Some(registry_url) = &config.registry_url {
        command.extend([
            "--registry-url".to_string(),
            quote_systemd_arg(registry_url),
        ]);
    }
    if let Some(log_dir) = &config.log_dir {
        command.extend(["--log-dir".to_string(), quote_systemd_arg(log_dir)]);
    }
    if let Some(public_base_url) = &config.public_base_url {
        command.extend([
            "--public-base-url".to_string(),
            quote_systemd_arg(public_base_url),
        ]);
    }
    format!(
        "[Unit]\nDescription=McpMux local MCP gateway\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nExecStart={}\nRestart=on-failure\nRestartSec=5s\n\n[Install]\nWantedBy=default.target\n",
        command.join(" ")
    )
}

fn quote_systemd_arg(value: impl AsRef<std::ffi::OsStr>) -> String {
    let mut escaped = String::from('"');
    for byte in value.as_ref().as_encoded_bytes() {
        if matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'/' | b'.' | b'_' | b'-' | b':' | b'=')
        {
            escaped.push(*byte as char);
        } else if *byte == b'%' {
            // systemd decodes `\x25` back to `%` before expanding specifiers;
            // `%%` is the only literal percent.
            escaped.push_str("%%");
        } else if *byte == b'$' {
            // Likewise `$$` is the only literal dollar (no variable expansion).
            escaped.push_str("$$");
        } else {
            escaped.push_str(&format!("\\x{byte:02x}"));
        }
    }
    escaped.push('"');
    escaped
}

fn write_unit(path: &Path, content: &str) -> anyhow::Result<()> {
    let temporary_path = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary_path, content)
        .with_context(|| format!("could not write {}", temporary_path.display()))?;
    fs::rename(&temporary_path, path)
        .with_context(|| format!("could not install {}", path.display()))
}

fn systemctl<const N: usize>(args: [&str; N]) -> anyhow::Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .context("could not execute systemctl --user")?;
    if !status.success() {
        bail!("systemctl --user exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{quote_systemd_arg, render_unit, ServiceConfig};
    use std::path::PathBuf;

    #[test]
    fn renders_a_restartable_user_service() {
        let unit = render_unit(&ServiceConfig {
            executable: PathBuf::from("/opt/mcpmux/bin/mcpmuxd"),
            data_dir: PathBuf::from("/var/lib/mcpmux data"),
            port: Some(45819),
            registry_url: Some("https://registry.example.test".to_string()),
            key_provider: "file",
            log_dir: Some(PathBuf::from("/var/log/mcpmux")),
            log_filter: "info,mcpmux_gateway=debug".to_string(),
            public_base_url: Some("https://mcp.example.test".to_string()),
        });

        assert!(unit.contains("ExecStart=\"/opt/mcpmux/bin/mcpmuxd\""));
        assert!(unit.contains("--data-dir \"/var/lib/mcpmux\\x20data\""));
        assert!(unit.contains("--key-provider file"));
        assert!(unit.contains("--port 45819"));
        assert!(unit.contains("--registry-url \"https://registry.example.test\""));
        assert!(unit.contains("Restart=on-failure"));
        assert!(!unit.contains("--auth-disabled"));
    }

    #[test]
    fn escapes_systemd_special_characters() {
        assert_eq!(quote_systemd_arg("a b%\"\\c"), "\"a\\x20b%%\\x22\\x5cc\"");
        assert_eq!(quote_systemd_arg("/data/%h/$HOME"), "\"/data/%%h/$$HOME\"");
    }
}
