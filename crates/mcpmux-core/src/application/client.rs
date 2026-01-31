//! Client Application Service
//!
//! Manages inbound MCP clients with automatic event emission.

use std::sync::Arc;
use anyhow::{anyhow, Result};
use tracing::info;
use uuid::Uuid;

use crate::domain::{DomainEvent, Client};
use crate::event_bus::EventSender;
use crate::repository::InboundMcpClientRepository;

/// Application service for client management
pub struct ClientAppService {
    client_repo: Arc<dyn InboundMcpClientRepository>,
    event_sender: EventSender,
}

impl ClientAppService {
    pub fn new(
        client_repo: Arc<dyn InboundMcpClientRepository>,
        event_sender: EventSender,
    ) -> Self {
        Self {
            client_repo,
            event_sender,
        }
    }

    /// List all clients
    pub async fn list(&self) -> Result<Vec<Client>> {
        self.client_repo.list().await
    }

    /// Get a client by ID
    pub async fn get(&self, id: Uuid) -> Result<Option<Client>> {
        self.client_repo.get(&id).await
    }

    /// Get a client by access key
    pub async fn get_by_access_key(&self, key: &str) -> Result<Option<Client>> {
        self.client_repo.get_by_access_key(key).await
    }

    /// Create a new client
    ///
    /// Emits: `ClientRegistered`
    pub async fn create(&self, name: &str, client_type: &str) -> Result<Client> {
        let mut client = Client::new(name, client_type);
        client.generate_access_key();

        self.client_repo.create(&client).await?;

        info!(
            client_id = %client.id,
            name = name,
            "[ClientAppService] Created client"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::ClientRegistered {
            client_id: client.id.to_string(),
            client_name: client.name.clone(),
            registration_type: Some("api_key".to_string()),
        });

        Ok(client)
    }

    /// Register OAuth client (called during OAuth registration flow)
    ///
    /// Emits: `ClientRegistered`
    pub async fn register_oauth_client(
        &self,
        client_id: &str,
        name: &str,
        client_type: &str,
    ) -> Result<Client> {
        // Parse client_id to UUID (OAuth client ID is UUID based)
        let id = Uuid::parse_str(client_id)
            .map_err(|e| anyhow!("Invalid client ID: {}", e))?;

        let mut client = Client::new(name, client_type);
        // Override auto-generated ID with OAuth client ID
        client.id = id;

        self.client_repo.create(&client).await?;

        info!(
            client_id = %client.id,
            name = name,
            "[ClientAppService] Registered OAuth client"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::ClientRegistered {
            client_id: client.id.to_string(),
            client_name: client.name.clone(),
            registration_type: Some("oauth".to_string()),
        });

        Ok(client)
    }

    /// Update a client
    ///
    /// Emits: `ClientUpdated`
    pub async fn update(
        &self,
        id: Uuid,
        name: Option<String>,
        client_type: Option<String>,
    ) -> Result<Client> {
        let mut client = self.client_repo.get(&id).await?
            .ok_or_else(|| anyhow!("Client not found"))?;

        if let Some(name) = name {
            client.name = name;
        }
        if let Some(ct) = client_type {
            client.client_type = ct;
        }
        client.updated_at = chrono::Utc::now();

        self.client_repo.update(&client).await?;

        info!(
            client_id = %client.id,
            "[ClientAppService] Updated client"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::ClientUpdated {
            client_id: client.id.to_string(),
        });

        Ok(client)
    }

    /// Delete a client
    ///
    /// Emits: `ClientDeleted`
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        self.client_repo.delete(&id).await?;

        info!(
            client_id = %id,
            "[ClientAppService] Deleted client"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::ClientDeleted {
            client_id: id.to_string(),
        });

        Ok(())
    }

    /// Record token issuance (OAuth flow completed)
    ///
    /// Emits: `ClientTokenIssued`
    pub fn record_token_issued(&self, client_id: &str) {
        info!(
            client_id = client_id,
            "[ClientAppService] Token issued to client"
        );

        self.event_sender.emit(DomainEvent::ClientTokenIssued {
            client_id: client_id.to_string(),
        });
    }

    /// Get the event sender (for external components that need to emit client events)
    pub fn event_sender(&self) -> &EventSender {
        &self.event_sender
    }
}

