//! `mcpmux serve` — headless McpMux gateway.
//!
//! Runs the exact same `mcpmux-gateway` used by the desktop app, wired to
//! SQLite-backed storage, with no Tauri shell. Intended for self-host and
//! container deployments. Configuration comes from a TOML file and/or
//! environment variables (see `config.rs`); secrets (the master encryption
//! key) can be injected via `MCPMUX_MASTER_KEY` for stateless key management.

mod config;
#[cfg(feature = "embed-ui")]
mod webui;

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::Mutex;
use tracing::{info, warn};

use config::Config;
use mcpmux_core::{
    AppSettingsRepository, CredentialRepository, FeatureSetRepository, InstalledServerRepository,
    LogConfig, OutboundOAuthRepository, ServerDiscoveryService,
    ServerFeatureRepository as CoreServerFeatureRepository, ServerLogManager,
};
use mcpmux_gateway::{DependenciesBuilder, GatewayConfig, GatewayServer};
use mcpmux_storage::{
    Database, EnvKeyProvider, FieldEncryptor, MasterKeyProvider, SqliteAppSettingsRepository,
    SqliteCredentialRepository, SqliteFeatureSetRepository, SqliteInstalledServerRepository,
    SqliteOutboundOAuthRepository, SqliteServerFeatureRepository,
};

#[tokio::main]
async fn main() -> Result<()> {
    // `mcpmux serve [--config PATH]` (serve is the only/default subcommand).
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return Ok(());
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("mcpmux {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let config_path = arg_value(&args, "--config")
        .or_else(|| std::env::var("MCPMUX_CONFIG").ok())
        .map(std::path::PathBuf::from);

    let config = Config::load(config_path).context("loading configuration")?;

    init_tracing(&config.log);
    config.validate()?; // no unauthenticated network bind

    info!(
        "[serve] Starting McpMux gateway {} on {}:{} (data_dir={:?})",
        env!("CARGO_PKG_VERSION"),
        config.host,
        config.port,
        config.data_dir
    );
    if config.is_network_bind() {
        info!("[serve] Network bind — inbound auth required, Host allowlist enforced");
    }

    run(config).await
}

async fn run(config: Config) -> Result<()> {
    std::fs::create_dir_all(&config.data_dir)
        .with_context(|| format!("creating data dir {:?}", config.data_dir))?;

    // --- Master key: env-injected (stateless) or on-disk/keychain (persistent).
    let master_key = if let Ok(hex) = std::env::var("MCPMUX_MASTER_KEY") {
        info!("[serve] Using master key from MCPMUX_MASTER_KEY");
        EnvKeyProvider::from_hex(&hex)?.get_or_create_key()?
    } else {
        info!("[serve] Using on-disk/keychain master key provider");
        mcpmux_storage::create_key_provider(&config.data_dir)?.get_or_create_key()?
    };
    let encryptor = Arc::new(FieldEncryptor::new(&master_key)?);

    // --- Database + repositories (mirrors the desktop AppState wiring).
    let db_path = config.data_dir.join("mcpmux.db");
    let db = Arc::new(Mutex::new(
        Database::open(&db_path).with_context(|| format!("opening database {db_path:?}"))?,
    ));

    let installed_server_repo: Arc<dyn InstalledServerRepository> = Arc::new(
        SqliteInstalledServerRepository::new(db.clone(), encryptor.clone()),
    );
    let credential_repo: Arc<dyn CredentialRepository> = Arc::new(SqliteCredentialRepository::new(
        db.clone(),
        encryptor.clone(),
    ));
    let backend_oauth_repo: Arc<dyn OutboundOAuthRepository> =
        Arc::new(SqliteOutboundOAuthRepository::new(db.clone()));
    let feature_set_repo: Arc<dyn FeatureSetRepository> =
        Arc::new(SqliteFeatureSetRepository::new(db.clone()));
    let server_feature_repo: Arc<dyn CoreServerFeatureRepository> =
        Arc::new(SqliteServerFeatureRepository::new(db.clone()));
    let settings_repo: Arc<dyn AppSettingsRepository> =
        Arc::new(SqliteAppSettingsRepository::new(db.clone()));

    // --- Server discovery + log manager.
    let spaces_dir = config.data_dir.join("spaces");
    std::fs::create_dir_all(&spaces_dir)?;
    let registry_url = std::env::var("MCPMUX_REGISTRY_URL")
        .unwrap_or_else(|_| "https://api.mcpmux.com".to_string());
    let server_discovery = Arc::new(
        ServerDiscoveryService::new(config.data_dir.clone(), spaces_dir.clone())
            .with_registry_api(registry_url),
    );
    let server_log_manager = Arc::new(ServerLogManager::new(LogConfig {
        base_dir: config.data_dir.join("logs"),
        max_file_size: 10 * 1024 * 1024,
        max_files: 30,
        compress: true,
    }));

    // --- JWT signing secret (headless-friendly provider).
    let jwt_secret = match mcpmux_storage::create_jwt_secret_provider(&config.data_dir) {
        Ok(p) => match p.get_or_create_secret() {
            Ok(s) => Some(s),
            Err(e) => {
                warn!("[serve] JWT secret unavailable ({e}); token signing disabled");
                None
            }
        },
        Err(e) => {
            warn!("[serve] JWT secret provider error ({e}); token signing disabled");
            None
        }
    };

    // --- Assemble gateway dependencies (same builder the desktop uses).
    let mut builder = DependenciesBuilder::new()
        .with_installed_server_repo(installed_server_repo)
        .with_credential_repo(credential_repo)
        .with_backend_oauth_repo(backend_oauth_repo)
        .with_feature_repo(server_feature_repo)
        .with_feature_set_repo(feature_set_repo)
        .with_server_discovery(server_discovery)
        .with_log_manager(server_log_manager)
        .with_database(db.clone())
        .with_state_dir(config.data_dir.clone())
        .with_settings_repo(settings_repo);
    if let Some(secret) = jwt_secret {
        builder = builder.with_jwt_secret(secret);
    }
    let dependencies = builder.build().map_err(|e| anyhow::anyhow!(e))?;

    let gateway_config = GatewayConfig {
        host: config.host.clone(),
        port: config.port,
        public_base_url: config.public_base_url.clone(),
        enable_cors: true,
        additional_allowed_hosts: config.additional_allowed_hosts.clone(),
        allow_any_host: config.allow_any_host,
    };

    let server = GatewayServer::new(gateway_config, dependencies);

    // Seed the inbound-auth toggle. On a network bind the engine rejects
    // disabling auth — validate() already caught that, so this only applies on
    // loopback.
    {
        let gw_state = server.state();
        if config.auth_disabled {
            gw_state
                .write()
                .await
                .set_auth_disabled(true)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            warn!("[serve] Inbound authentication DISABLED (loopback convenience)");
        }
    }

    // Admin API token: operator-supplied (MCPMUX_ADMIN_TOKEN) or generated. The
    // management router is bearer-gated; on a network bind it's the only gate,
    // so a generated token is 256 bits.
    let admin_token = Arc::new(
        std::env::var("MCPMUX_ADMIN_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(generate_admin_token),
    );
    let extra_router = mcpmux_gateway::server::management::management_router(
        server.app_state(),
        admin_token.clone(),
    );
    // Embedded web admin (the desktop React app served headless at /app).
    #[cfg(feature = "embed-ui")]
    let extra_router = {
        info!("[serve] Web admin mounted at /app");
        extra_router.merge(webui::router())
    };
    // Print the token once so the operator can reach the admin API + sign in.
    info!(
        "[serve] Admin API mounted at /admin/api (token: {})",
        admin_token
    );

    info!(
        "[serve] Ready. MCP endpoint: http://{}:{}/mcp  ·  health: /health  ·  admin: /admin/api",
        if config.is_network_bind() {
            config.host.as_str()
        } else {
            "localhost"
        },
        config.port
    );

    // Serve until SIGTERM / Ctrl-C — drains in-flight requests and releases the
    // port cleanly (important for container restarts). The management router is
    // merged into the gateway's own router.
    server
        .run_with_shutdown_and_router(extra_router, shutdown_signal())
        .await
        .context("gateway server error")
}

/// Resolve when the process receives a termination signal.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sig) = signal(SignalKind::terminate()) {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("[serve] Ctrl-C received, shutting down"),
        _ = terminate => info!("[serve] SIGTERM received, shutting down"),
    }
}

fn init_tracing(filter: &str) {
    use tracing_subscriber::{fmt, EnvFilter};
    let env_filter = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(env_filter).with_target(false).init();
}

/// Generate a strong (256-bit) admin API token when the operator doesn't
/// supply one via `MCPMUX_ADMIN_TOKEN`.
fn generate_admin_token() -> String {
    format!(
        "mcpadmin_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Read the value following `flag` in argv (`--config PATH`).
fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn print_help() {
    println!(
        "mcpmux {} — headless McpMux gateway\n\
\n\
USAGE:\n\
    mcpmux [--config PATH]\n\
\n\
OPTIONS:\n\
    --config PATH   Path to a TOML config file (or set MCPMUX_CONFIG)\n\
    -h, --help      Print this help\n\
    -V, --version   Print version\n\
\n\
ENVIRONMENT (override the config file):\n\
    MCPMUX_DATA_DIR         Data directory (db, keys, logs, spaces)\n\
    MCPMUX_HOST             Bind host (default 127.0.0.1)\n\
    MCPMUX_PORT             Bind port (default {})\n\
    MCPMUX_PUBLIC_BASE_URL  External origin advertised in OAuth metadata\n\
    MCPMUX_AUTH_DISABLED    Disable inbound auth (loopback only)\n\
    MCPMUX_ALLOWED_HOSTS    Comma-separated extra Host-header values\n\
    MCPMUX_ALLOW_ANY_HOST   Accept any Host on a network bind (not recommended)\n\
    MCPMUX_MASTER_KEY       Hex master key (stateless; else on-disk/keychain)\n\
    MCPMUX_LOG / RUST_LOG   Log filter (default info)\n",
        env!("CARGO_PKG_VERSION"),
        mcpmux_core::DEFAULT_GATEWAY_PORT
    );
}
