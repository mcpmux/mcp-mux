//! Gateway state management
//!
//! Manages gateway-level state including:
//! - Client sessions and access keys
//! - OAuth tokens and pending authorizations
//! - JWT signing secrets
//! - Database connections

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::handlers::PendingAuthorization;
use crate::services::ClientMetadataService;
use mcpmux_core::DomainEvent;
use mcpmux_storage::{Database, InboundClientRepository, JWT_SECRET_SIZE};
use tokio::sync::broadcast;

/// Client session in the gateway
#[derive(Debug, Clone)]
pub struct ClientSession {
    /// Session ID
    pub id: Uuid,
    /// Client ID (from McpMux)
    pub client_id: Uuid,
    /// Access key used
    pub access_key: String,
    /// Currently active space
    pub space_id: Uuid,
    /// Connected backend servers
    pub connected_backends: Vec<String>,
    /// Session start time
    pub started_at: chrono::DateTime<chrono::Utc>,
}

/// Gateway server state
///
/// Note: Server connections are managed by PoolService, not here.
/// This state is for gateway-level concerns only.
pub struct GatewayState {
    /// Base URL for this gateway (e.g., "http://localhost:3100")
    pub base_url: String,
    /// Active client sessions
    pub sessions: HashMap<Uuid, ClientSession>,
    /// Access key to client ID mapping
    pub access_keys: HashMap<String, Uuid>,
    /// OAuth tokens per server (in-memory cache)
    pub oauth_tokens: HashMap<String, super::super::oauth::OAuthToken>,
    /// Pending authorization codes (code -> PendingAuthorization)
    pub pending_authorizations: HashMap<String, PendingAuthorization>,
    /// Set of client_ids that have been issued tokens (for "active" status)
    pub clients_with_tokens: std::collections::HashSet<String>,
    /// JWT signing secret (for issuing access tokens)
    pub jwt_signing_secret: Option<Zeroizing<[u8; JWT_SECRET_SIZE]>>,
    /// Database connection (for persistent OAuth storage)
    db: Option<Arc<Mutex<Database>>>,
    /// Inbound client repository (OAuth + MCP client unified storage)
    inbound_client_repository: Option<InboundClientRepository>,
    /// Client metadata service (CIMD + DCR resolution)
    client_metadata_service: Option<Arc<ClientMetadataService>>,
    /// Unified event broadcaster (UI subscribes to receive all domain events)
    domain_event_tx: broadcast::Sender<DomainEvent>,
}

impl GatewayState {
    /// Create new gateway state with provided event sender
    pub fn new(domain_event_tx: broadcast::Sender<DomainEvent>) -> Self {
        Self {
            base_url: "http://localhost:3100".to_string(), // Default
            sessions: HashMap::new(),
            access_keys: HashMap::new(),
            oauth_tokens: HashMap::new(),
            pending_authorizations: HashMap::new(),
            clients_with_tokens: std::collections::HashSet::new(),
            jwt_signing_secret: None,
            db: None,
            inbound_client_repository: None,
            client_metadata_service: None,
            domain_event_tx,
        }
    }

    /// Set the base URL
    pub fn set_base_url(&mut self, base_url: String) {
        info!("[State] Base URL configured: {}", base_url);
        self.base_url = base_url;
    }

    /// Subscribe to domain events (new unified channel)
    pub fn subscribe_domain_events(&self) -> broadcast::Receiver<DomainEvent> {
        self.domain_event_tx.subscribe()
    }

    /// Get a clone of the domain event sender
    pub fn domain_event_sender(&self) -> broadcast::Sender<DomainEvent> {
        self.domain_event_tx.clone()
    }

    /// Emit a domain event (new unified emission point)
    pub fn emit_domain_event(&self, event: DomainEvent) {
        if let Err(e) = self.domain_event_tx.send(event) {
            debug!("[State] No domain event subscribers: {}", e);
        }
    }

    /// Set the database connection and create OAuth repository
    pub fn set_database(&mut self, db: Arc<Mutex<Database>>) {
        info!("[State] Database connection configured for OAuth persistence");
        self.inbound_client_repository = Some(InboundClientRepository::new(db.clone()));
        self.db = Some(db);
    }

    /// Get the inbound client repository (for persistent storage)
    pub fn inbound_client_repository(&self) -> Option<&InboundClientRepository> {
        self.inbound_client_repository.as_ref()
    }

    /// Set the client metadata service
    pub fn set_client_metadata_service(&mut self, service: Arc<ClientMetadataService>) {
        info!("[State] Client metadata service configured (CIMD + DCR resolution)");
        self.client_metadata_service = Some(service);
    }

    /// Get the client metadata service
    pub fn client_metadata_service(&self) -> Option<&ClientMetadataService> {
        self.client_metadata_service.as_ref().map(|s| s.as_ref())
    }

    /// Check if database is connected
    pub fn has_database(&self) -> bool {
        self.db.is_some()
    }

    /// Set the JWT signing secret
    pub fn set_jwt_secret(&mut self, secret: Zeroizing<[u8; JWT_SECRET_SIZE]>) {
        info!("[State] JWT signing secret configured");
        self.jwt_signing_secret = Some(secret);
    }

    /// Get the JWT signing secret
    pub fn get_jwt_secret(&self) -> Option<&[u8; JWT_SECRET_SIZE]> {
        self.jwt_signing_secret.as_deref()
    }

    /// Check if JWT signing is available
    pub fn has_jwt_secret(&self) -> bool {
        self.jwt_signing_secret.is_some()
    }

    /// Store a pending authorization (for code -> token exchange)
    pub fn store_pending_authorization(&mut self, code: &str, auth: PendingAuthorization) {
        debug!(
            "[State] Storing pending authorization for code: {}...",
            &code[..code.len().min(16)]
        );
        self.pending_authorizations.insert(code.to_string(), auth);
    }

    /// Consume a pending authorization (one-time use)
    pub fn consume_pending_authorization(&mut self, code: &str) -> Option<PendingAuthorization> {
        let result = self.pending_authorizations.remove(code);
        if result.is_some() {
            debug!(
                "[State] Consumed pending authorization for code: {}...",
                &code[..code.len().min(16)]
            );
        }
        result
    }

    /// Register an access key for a client
    pub fn register_access_key(&mut self, access_key: String, client_id: Uuid) {
        info!("[State] Registered access key for client: {}", client_id);
        self.access_keys.insert(access_key, client_id);
    }

    /// Validate an access key and return the client ID
    pub fn validate_access_key(&self, access_key: &str) -> Option<Uuid> {
        let result = self.access_keys.get(access_key).copied();
        if result.is_some() {
            debug!("[State] Access key validated");
        } else {
            debug!("[State] Access key validation failed");
        }
        result
    }

    /// Create a new session
    pub fn create_session(
        &mut self,
        client_id: Uuid,
        access_key: String,
        space_id: Uuid,
    ) -> ClientSession {
        let session = ClientSession {
            id: Uuid::new_v4(),
            client_id,
            access_key,
            space_id,
            connected_backends: vec![],
            started_at: chrono::Utc::now(),
        };
        info!(
            "[State] Created session: {} for client: {} in space: {}",
            session.id, client_id, space_id
        );
        self.sessions.insert(session.id, session.clone());
        session
    }

    /// Get a session by ID
    pub fn get_session(&self, session_id: &Uuid) -> Option<&ClientSession> {
        self.sessions.get(session_id)
    }

    /// Remove a session
    pub fn remove_session(&mut self, session_id: &Uuid) -> Option<ClientSession> {
        if let Some(session) = self.sessions.remove(session_id) {
            info!(
                "[State] Removed session: {} (client: {}, duration: {}s)",
                session.id,
                session.client_id,
                (chrono::Utc::now() - session.started_at).num_seconds()
            );
            Some(session)
        } else {
            None
        }
    }

    /// Store an OAuth token for a server
    pub fn store_oauth_token(&mut self, server_id: String, token: super::super::oauth::OAuthToken) {
        let expires_info = match &token.expires_at {
            Some(dt) => format!("expires at {}", dt.format("%Y-%m-%d %H:%M:%S UTC")),
            None => "no expiry".to_string(),
        };
        info!(
            "[State] Stored OAuth token for server: {} ({})",
            server_id, expires_info
        );
        self.oauth_tokens.insert(server_id, token);
    }

    /// Get an OAuth token for a server
    pub fn get_oauth_token(&self, server_id: &str) -> Option<&super::super::oauth::OAuthToken> {
        self.oauth_tokens.get(server_id)
    }
}

impl Default for GatewayState {
    fn default() -> Self {
        let (domain_event_tx, _) = broadcast::channel(256);
        Self::new(domain_event_tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::OAuthToken;

    fn make_state() -> GatewayState {
        GatewayState::default()
    }

    fn test_pending_auth(client_id: &str) -> PendingAuthorization {
        PendingAuthorization {
            client_id: client_id.to_string(),
            client_name: Some("Test Client".to_string()),
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scope: Some("read".to_string()),
            state: Some("state123".to_string()),
            code_challenge: Some("challenge".to_string()),
            code_challenge_method: Some("S256".to_string()),
            expires_at: chrono::Utc::now().timestamp() + 300,
            consent_token: None,
        }
    }

    #[test]
    fn store_consume_pending_auth_one_time() {
        let mut state = make_state();
        let auth = test_pending_auth("client-1");

        state.store_pending_authorization("code-abc", auth);

        let consumed = state.consume_pending_authorization("code-abc");
        assert!(consumed.is_some());
        assert_eq!(consumed.unwrap().client_id, "client-1");

        // Second consume returns None (one-time use)
        let second = state.consume_pending_authorization("code-abc");
        assert!(second.is_none());
    }

    #[test]
    fn consume_nonexistent_returns_none() {
        let mut state = make_state();
        let result = state.consume_pending_authorization("unknown-code");
        assert!(result.is_none());
    }

    #[test]
    fn register_validate_access_key() {
        let mut state = make_state();
        let client_id = Uuid::new_v4();

        state.register_access_key("mcp_test_key".to_string(), client_id);

        let validated = state.validate_access_key("mcp_test_key");
        assert_eq!(validated, Some(client_id));
    }

    #[test]
    fn validate_unknown_key_returns_none() {
        let state = make_state();
        assert!(state.validate_access_key("unknown_key").is_none());
    }

    #[test]
    fn register_same_key_overwrites() {
        let mut state = make_state();
        let client1 = Uuid::new_v4();
        let client2 = Uuid::new_v4();

        state.register_access_key("shared_key".to_string(), client1);
        state.register_access_key("shared_key".to_string(), client2);

        assert_eq!(state.validate_access_key("shared_key"), Some(client2));
    }

    #[test]
    fn create_get_remove_session() {
        let mut state = make_state();
        let client_id = Uuid::new_v4();
        let space_id = Uuid::new_v4();

        let session = state.create_session(client_id, "key".to_string(), space_id);
        let session_id = session.id;

        // Get session
        let found = state.get_session(&session_id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().client_id, client_id);

        // Remove session
        let removed = state.remove_session(&session_id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().space_id, space_id);

        // Gone after removal
        assert!(state.get_session(&session_id).is_none());
    }

    #[test]
    fn remove_nonexistent_session_none() {
        let mut state = make_state();
        assert!(state.remove_session(&Uuid::new_v4()).is_none());
    }

    #[test]
    fn jwt_secret_none_before_set() {
        let state = make_state();
        assert!(state.get_jwt_secret().is_none());
        assert!(!state.has_jwt_secret());
    }

    #[test]
    fn set_get_jwt_secret() {
        let mut state = make_state();
        let secret = Zeroizing::new([42u8; JWT_SECRET_SIZE]);
        state.set_jwt_secret(secret);

        assert!(state.has_jwt_secret());
        let retrieved = state.get_jwt_secret().unwrap();
        assert_eq!(retrieved[0], 42);
    }

    #[test]
    fn store_get_oauth_token() {
        let mut state = make_state();
        let token = OAuthToken {
            access_token: "access-123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_at: None,
            scope: None,
            id_token: None,
        };

        state.store_oauth_token("server-1".to_string(), token);

        let found = state.get_oauth_token("server-1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().access_token, "access-123");

        // Non-existent server
        assert!(state.get_oauth_token("server-2").is_none());
    }
}
