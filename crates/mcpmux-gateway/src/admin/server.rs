//! Admin HTTP server lifecycle (bind, serve, graceful shutdown).

use axum::Router;
use mcpmux_core::ApplicationServices;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::info;

use super::bridge_context::AdminBridgeCtx;
use super::config::AdminConfig;
use super::event_hub::AdminEventHub;
use super::middleware::new_csrf_token_store;
use super::middleware::CfAccessValidator;
use super::router::{build_admin_router, AdminState};

/// Running admin server handle for graceful shutdown.
pub struct AdminServerHandle {
    pub task: tokio::task::JoinHandle<anyhow::Result<()>>,
    shutdown: CancellationToken,
}

impl AdminServerHandle {
    /// Signal the admin server to stop accepting new connections.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }
}

/// Web admin HTTP server (static SPA + `/api/v1/*`).
pub struct AdminServer {
    config: AdminConfig,
    router: Router,
    bind_addr: SocketAddr,
}

impl AdminServer {
    /// Build the admin server without binding.
    pub async fn new(
        config: AdminConfig,
        services: Arc<ApplicationServices>,
        bridge: Arc<AdminBridgeCtx>,
        event_hub: Arc<AdminEventHub>,
        gateway_running: Arc<AtomicBool>,
        frontend_dist: PathBuf,
        cf_validator: Option<Arc<CfAccessValidator>>,
    ) -> anyhow::Result<Self> {
        let bind_addr: SocketAddr = config
            .bind_addr()
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid admin bind address: {e}"))?;

        let router = build_admin_router(AdminState {
            services,
            config: config.clone(),
            gateway_running,
            frontend_dist,
            cf_validator,
            bridge,
            event_hub,
            csrf_token: new_csrf_token_store(),
        });

        Ok(Self {
            config,
            router,
            bind_addr,
        })
    }

    /// Load CF Access validator from team domain when trust is enabled.
    pub async fn build_cf_validator(
        config: &AdminConfig,
    ) -> anyhow::Result<Option<Arc<CfAccessValidator>>> {
        if !config.trust_cf_access {
            return Ok(None);
        }
        if let Some(ref validator) = config.cf_validator_override {
            return Ok(Some(validator.clone()));
        }
        let team = config.cf_team_domain.as_deref().ok_or_else(|| {
            anyhow::anyhow!("admin_trust_cf_access requires gateway.admin_cf_team_domain")
        })?;
        let validator =
            CfAccessValidator::from_team_domain(team, config.cf_access_audience.clone()).await?;
        Ok(Some(Arc::new(validator)))
    }

    /// Bind and serve until shutdown is cancelled.
    pub async fn run_with_shutdown(self, shutdown: CancellationToken) -> anyhow::Result<()> {
        let listener = TcpListener::bind(self.bind_addr).await?;
        info!(
            "[Admin] Listening on {} (cf_access={})",
            self.bind_addr, self.config.trust_cf_access
        );

        axum::serve(listener, self.router)
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
                info!("[Admin] Graceful shutdown");
            })
            .await?;

        Ok(())
    }

    /// Start the admin server in the background.
    pub fn spawn(self) -> AdminServerHandle {
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move { self.run_with_shutdown(token).await });
        AdminServerHandle { task, shutdown }
    }
}
