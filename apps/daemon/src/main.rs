//! `mcpmuxd` — McpMux headless daemon.
//!
//! Linux-first Phase 1 release. Binds the Streamable HTTP gateway on
//! `127.0.0.1:45818` by default, logs to journald (stdout), and exits
//! cleanly on SIGTERM / SIGINT. See `docs/manual/headless-cli-roadmap.md`
//! for the full plan.

use std::process::ExitCode;

use clap::Parser;
#[cfg(unix)]
use mcpmux_gateway::GatewayConfig;
#[cfg(unix)]
use mcpmux_runtime::{
    init_tracing, shutdown_gateway_runtime, wait_for_health, wait_for_shutdown, HealthCheckConfig,
    HealthStatus, KeyProviderPolicy, LogSink, RuntimeBuilder, TracingConfig,
};
#[cfg(unix)]
use std::io::IsTerminal;
#[cfg(unix)]
use tokio::time::Duration;
use tracing::error;
#[cfg(unix)]
use tracing::{info, warn};
#[cfg(unix)]
use tracing_appender::non_blocking::WorkerGuard;

mod args;
#[cfg(unix)]
mod control;
#[cfg(unix)]
mod service;
use args::Args;
#[cfg(unix)]
use args::{Command, KeyProviderArg, ServiceCommand};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> ExitCode {
    let args = Args::parse();

    if let Err(e) = run(args).await {
        error!(error = %e, "[mcpmuxd] exited with error");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Flag combinations the daemon refuses to start with.
#[cfg_attr(not(unix), allow(dead_code))]
fn check_args(args: &Args) -> anyhow::Result<()> {
    if args.auth_disabled && args.public_base_url.is_some() {
        anyhow::bail!(
            "--auth-disabled can't be combined with --public-base-url: anyone who reaches \
             the public URL would get tool access without signing in"
        );
    }
    Ok(())
}

#[cfg(unix)]
async fn run(args: Args) -> anyhow::Result<()> {
    let _log_guard = init_logging(&args);
    check_args(&args)?;

    if matches!(
        &args.command,
        Some(Command::Service {
            command: ServiceCommand::Install
        })
    ) {
        return service::install(&args);
    }

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "mcpmuxd {} starting",
        mcpmux_core::branding::DISPLAY_NAME,
    );

    let mut runtime_builder = RuntimeBuilder::new()
        .with_data_dir(
            args.data_dir
                .clone()
                .unwrap_or_else(mcpmux_runtime::default_data_dir),
        )
        .with_registry_url(
            args.registry_url
                .clone()
                .unwrap_or_else(|| mcpmux_runtime::DEFAULT_REGISTRY_URL.to_string()),
        )
        .with_key_provider_policy(map_key_policy(args.key_provider));
    if let Some(log_dir) = args.log_dir.clone() {
        runtime_builder = runtime_builder.with_log_dir(log_dir);
    }
    let runtime = runtime_builder.build().await.map_err(|e| {
        error!(error = %e, "[mcpmuxd] runtime init failed");
        anyhow::anyhow!(e.to_string())
    })?;

    info!(
        data_dir = %runtime.data_dir.display(),
        keys_dir = %runtime.keys_dir.display(),
        logs_dir = %runtime.logs_dir.display(),
        "mcpmuxd runtime ready"
    );

    let bind_host = "127.0.0.1";
    let preferred_port = match args.port {
        Some(port) => port,
        None => runtime
            .gateway_port_service
            .load_persisted_port()
            .await
            .unwrap_or(mcpmux_core::DEFAULT_GATEWAY_PORT),
    };
    let gw_config = GatewayConfig {
        host: bind_host.to_string(),
        port: preferred_port,
        public_base_url: args.public_base_url.clone(),
        enable_cors: true,
    };

    let server = runtime.build_gateway_server(gw_config.clone()).await?;
    let gateway_state = server.state();
    // Capture the pool/feature/server-manager handles before `spawn` consumes
    // the server; the control socket drives the same live instances.
    let control_pool_service = server.pool_service();
    let shutdown_pool_service = control_pool_service.clone();
    let control_feature_service = server.feature_service();
    let control_server_manager = server.server_manager();

    // Install the signal listener before publishing the HTTP listener. This
    // closes the startup window where systemd could observe `/health` and
    // send SIGTERM before `wait_for_shutdown` had subscribed to it.
    let shutdown_task = tokio::spawn(wait_for_shutdown());
    tokio::task::yield_now().await;

    // `--auth-disabled` applies to this run only. It is never persisted:
    // a one-off local test must not leave a later service start (possibly
    // behind --public-base-url) unauthenticated.
    if args.auth_disabled {
        warn!("[mcpmuxd] inbound MCP auth is DISABLED for this run (--auth-disabled)");
        gateway_state.write().await.set_auth_disabled(true);
    }

    // Bridge gateway events into the shared EventBus. Phase 3 forwards
    // these to the control socket; the daemon itself does not need them.
    let _event_bridge =
        mcpmux_runtime::spawn_event_bridge(runtime.event_bus.clone(), gateway_state.clone()).await;

    let mut handle = server.spawn();

    // Fail on our own bind result: a /health 200 alone could come from
    // another process (e.g. the desktop app) already owning this port.
    if let Err(e) = handle.wait_until_bound().await {
        error!(error = %e, "[mcpmuxd] gateway failed to bind");
        shutdown_gateway_runtime(Some(handle), Some(shutdown_pool_service)).await;
        shutdown_task.abort();
        return Err(anyhow::anyhow!(
            "gateway failed to bind {bind_host}:{preferred_port}: {e} \
             (is the McpMux desktop app or another mcpmuxd already using this port?)"
        ));
    }

    // Post-spawn /health probe. systemd-journal readers see "ready" as
    // soon as this returns Ok.
    let probe_url = format!("http://{}:{}/health", bind_host, preferred_port);
    let health_cfg = HealthCheckConfig::new(probe_url.clone(), Duration::from_secs(2));
    match wait_for_health(&health_cfg).await {
        HealthStatus::Ok {
            version,
            round_trip,
        } => {
            info!(
                url = %probe_url,
                version = version.as_deref().unwrap_or("unknown"),
                rtt_ms = round_trip.as_millis() as u64,
                "mcpmuxd ready"
            );
        }
        HealthStatus::Unreachable {
            attempts,
            last_error,
        } => {
            error!(
                attempts,
                last_error, "[mcpmuxd] /health did not return 200 within the probe window"
            );
            shutdown_gateway_runtime(Some(handle), Some(shutdown_pool_service)).await;
            shutdown_task.abort();
            return Err(anyhow::anyhow!("health probe failed"));
        }
        HealthStatus::InvalidUrl(e) => {
            warn!(error = %e, "[mcpmuxd] invalid health probe URL");
        }
    }

    // Start the local control socket now that the gateway is confirmed up.
    // The CLI refuses to run without it, so a bind failure (e.g. another
    // daemon on the same data dir) is fatal rather than silent.
    let gateway_events =
        mcpmux_core::EventSender::from_broadcast(gateway_state.read().await.domain_event_sender());
    let control_state = std::sync::Arc::new(control::ControlState {
        runtime: runtime.clone(),
        gateway_state: gateway_state.clone(),
        gateway_events,
        pool_service: control_pool_service,
        feature_service: control_feature_service,
        server_manager: control_server_manager,
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION"),
        gateway_origin: format!("http://{}:{}", bind_host, preferred_port),
        port: preferred_port,
    });
    let control_server = match control::ControlServer::spawn(control_state).await {
        Ok(server) => server,
        Err(e) => {
            error!(error = %e, "[mcpmuxd] control socket failed to start");
            shutdown_gateway_runtime(Some(handle), Some(shutdown_pool_service)).await;
            shutdown_task.abort();
            return Err(anyhow::anyhow!(e.to_string()));
        }
    };
    info!(
        path = %control_server.socket_path().display(),
        "mcpmuxd control socket ready"
    );

    let reason = shutdown_task.await.unwrap_or("signal-handler-cancelled");
    info!(
        reason,
        "[mcpmuxd] shutdown signal received, draining gateway"
    );

    drop(control_server);
    shutdown_gateway_runtime(Some(handle), Some(shutdown_pool_service)).await;

    info!("[mcpmuxd] stopped cleanly");
    Ok(())
}

#[cfg(not(unix))]
async fn run(_args: Args) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "mcpmuxd is not available on Windows yet because its secure local control transport requires Unix sockets; use the McpMux desktop app"
    ))
}

#[cfg(unix)]
fn init_logging(args: &Args) -> Option<WorkerGuard> {
    let sink = match &args.log_dir {
        Some(dir) => LogSink::DailyRolling {
            dir: dir.clone(),
            prefix: mcpmux_core::branding::LOG_PREFIX.to_string(),
        },
        None => LogSink::StdoutOnly,
    };
    let console_ansi = args.log_dir.is_none() && std::io::stdout().is_terminal();
    let cfg = TracingConfig {
        sink,
        filter: args.log_filter.clone(),
        console_ansi,
    };
    init_tracing(&cfg)
}

#[cfg(unix)]
fn map_key_policy(arg: KeyProviderArg) -> KeyProviderPolicy {
    match arg {
        KeyProviderArg::Auto => KeyProviderPolicy::Auto,
        KeyProviderArg::Keychain => KeyProviderPolicy::Keychain,
        KeyProviderArg::File => KeyProviderPolicy::File,
    }
}

#[cfg(test)]
mod tests {
    use super::{check_args, Args};
    use clap::Parser;

    #[test]
    fn auth_cannot_be_disabled_behind_a_public_url() {
        let args = Args::parse_from([
            "mcpmuxd",
            "--auth-disabled",
            "--public-base-url",
            "https://mcp.example.com",
        ]);
        assert!(check_args(&args).is_err());

        assert!(check_args(&Args::parse_from(["mcpmuxd", "--auth-disabled"])).is_ok());
        assert!(check_args(&Args::parse_from([
            "mcpmuxd",
            "--public-base-url",
            "https://mcp.example.com"
        ]))
        .is_ok());
    }
}
