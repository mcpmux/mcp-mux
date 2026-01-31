//! Application Services - Orchestration layer with event emission
//!
//! Application services sit between the presentation layer (Tauri commands, HTTP handlers)
//! and the domain layer (repositories, domain services). They:
//!
//! 1. **Orchestrate** business operations across multiple repositories
//! 2. **Emit events** after successful operations via the event bus
//! 3. **Validate** inputs and enforce business rules
//! 4. **Provide** a clean API for the presentation layer
//!
//! # Architecture
//!
//! ```text
//! Presentation Layer (Tauri Commands)
//!         │
//!         ▼
//! ┌─────────────────────────────────────┐
//! │      Application Services           │
//! │  ┌─────────────────────────────┐   │
//! │  │ SpaceAppService             │   │
//! │  │ ServerAppService            │   │
//! │  │ PermissionAppService        │   │
//! │  │ ClientAppService            │   │
//! │  └─────────────┬───────────────┘   │
//! │                │                    │
//! │                ▼                    │
//! │         ┌──────────┐               │
//! │         │Event Bus │               │
//! │         └──────────┘               │
//! └─────────────────────────────────────┘
//!         │
//!         ▼
//! Domain Layer (Repositories)
//! ```
//!
//! # Usage
//!
//! ```ignore
//! let event_bus = EventBus::new();
//! let space_service = SpaceAppService::new(space_repo, fs_repo, event_bus.sender());
//!
//! // All operations automatically emit events
//! let space = space_service.create("Work", None).await?;
//! // -> Emits SpaceCreated event
//! ```

mod space;
mod server;
mod permission;
mod client;
mod user_space_sync;

pub use space::SpaceAppService;
pub use server::ServerAppService;
pub use permission::PermissionAppService;
pub use client::ClientAppService;
pub use user_space_sync::{UserSpaceSyncService, SyncResult};

use std::sync::Arc;
use crate::event_bus::EventBus;
use crate::repository::*;

/// Builder for creating all application services with shared dependencies
pub struct ApplicationServicesBuilder {
    event_bus: Option<Arc<EventBus>>,
    space_repo: Option<Arc<dyn SpaceRepository>>,
    installed_server_repo: Option<Arc<dyn InstalledServerRepository>>,
    feature_set_repo: Option<Arc<dyn FeatureSetRepository>>,
    server_feature_repo: Option<Arc<dyn ServerFeatureRepository>>,
    client_repo: Option<Arc<dyn InboundMcpClientRepository>>,
    credential_repo: Option<Arc<dyn CredentialRepository>>,
}

impl ApplicationServicesBuilder {
    pub fn new() -> Self {
        Self {
            event_bus: None,
            space_repo: None,
            installed_server_repo: None,
            feature_set_repo: None,
            server_feature_repo: None,
            client_repo: None,
            credential_repo: None,
        }
    }

    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn with_space_repo(mut self, repo: Arc<dyn SpaceRepository>) -> Self {
        self.space_repo = Some(repo);
        self
    }

    pub fn with_installed_server_repo(mut self, repo: Arc<dyn InstalledServerRepository>) -> Self {
        self.installed_server_repo = Some(repo);
        self
    }

    pub fn with_feature_set_repo(mut self, repo: Arc<dyn FeatureSetRepository>) -> Self {
        self.feature_set_repo = Some(repo);
        self
    }

    pub fn with_server_feature_repo(mut self, repo: Arc<dyn ServerFeatureRepository>) -> Self {
        self.server_feature_repo = Some(repo);
        self
    }

    pub fn with_client_repo(mut self, repo: Arc<dyn InboundMcpClientRepository>) -> Self {
        self.client_repo = Some(repo);
        self
    }

    pub fn with_credential_repo(mut self, repo: Arc<dyn CredentialRepository>) -> Self {
        self.credential_repo = Some(repo);
        self
    }

    /// Build all application services
    pub fn build(self) -> anyhow::Result<ApplicationServices> {
        let event_bus = self.event_bus.ok_or_else(|| anyhow::anyhow!("Event bus required"))?;
        let sender = event_bus.sender();

        Ok(ApplicationServices {
            event_bus,
            space: self.space_repo.map(|r| {
                SpaceAppService::new(r, self.feature_set_repo.clone(), sender.clone())
            }),
            server: self.installed_server_repo.map(|r| {
                ServerAppService::new(
                    r,
                    self.feature_set_repo.clone(),
                    self.server_feature_repo.clone(),
                    self.credential_repo.clone(),
                    sender.clone(),
                )
            }),
            permission: self.feature_set_repo.map(|r| {
                PermissionAppService::new(r, self.client_repo.clone(), sender.clone())
            }),
            client: self.client_repo.map(|r| {
                ClientAppService::new(r, sender.clone())
            }),
        })
    }
}

impl Default for ApplicationServicesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Container for all application services
pub struct ApplicationServices {
    /// Shared event bus
    pub event_bus: Arc<EventBus>,
    /// Space management
    pub space: Option<SpaceAppService>,
    /// Server installation and management
    pub server: Option<ServerAppService>,
    /// Feature sets and grants
    pub permission: Option<PermissionAppService>,
    /// Client management
    pub client: Option<ClientAppService>,
}

impl ApplicationServices {
    /// Get space service (panics if not configured)
    pub fn space(&self) -> &SpaceAppService {
        self.space.as_ref().expect("SpaceAppService not configured")
    }

    /// Get server service (panics if not configured)
    pub fn server(&self) -> &ServerAppService {
        self.server.as_ref().expect("ServerAppService not configured")
    }

    /// Get permission service (panics if not configured)
    pub fn permission(&self) -> &PermissionAppService {
        self.permission.as_ref().expect("PermissionAppService not configured")
    }

    /// Get client service (panics if not configured)
    pub fn client(&self) -> &ClientAppService {
        self.client.as_ref().expect("ClientAppService not configured")
    }

    /// Subscribe to events from all services
    pub fn subscribe(&self) -> crate::event_bus::EventReceiver {
        self.event_bus.subscribe()
    }
}

