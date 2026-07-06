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
use tracing::{debug, info, warn};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::handlers::PendingAuthorization;
use crate::services::ClientMetadataService;
use mcpmux_core::DomainEvent;
use mcpmux_storage::{Database, InboundClientRepository, JWT_SECRET_SIZE};
use tokio::sync::broadcast;

/// Rejected auth/network state transition: inbound authentication can never
/// be disabled while the gateway is bound to a non-loopback address. Callers
/// surface [`std::fmt::Display`] to the user and leave auth enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthNetworkConflict;

impl std::fmt::Display for AuthNetworkConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "authentication is required while the gateway is exposed on the network — \
             turn off network access first"
        )
    }
}

impl std::error::Error for AuthNetworkConflict {}

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
    /// Configured public base URL (e.g. an https tunnel origin). When set it is
    /// advertised verbatim in OAuth/MCP metadata; when None the advertised base
    /// is the request Host (on a network bind) or `base_url` (loopback).
    pub public_base_url: Option<String>,
    /// True when the gateway is bound to a non-loopback address. Lets the
    /// metadata handlers advertise the host a remote client actually used
    /// instead of `localhost`, without changing local-only behavior.
    pub network_bind: bool,
    /// Active client sessions
    pub sessions: HashMap<Uuid, ClientSession>,
    /// Access key to client ID mapping
    pub access_keys: HashMap<String, Uuid>,
    /// OAuth tokens per server (in-memory cache)
    pub oauth_tokens: HashMap<String, super::super::oauth::OAuthToken>,
    /// Pending authorization codes (code -> PendingAuthorization)
    pub pending_authorizations: HashMap<String, PendingAuthorization>,
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
    /// When true, inbound MCP connections are accepted WITHOUT a Bearer token
    /// (localhost-only convenience). Default false (auth required). Seeded from
    /// the `gateway.auth_disabled` app setting at startup and flipped live by
    /// the desktop toggle. A valid token is still honored when present.
    auth_disabled: bool,
    /// Outstanding device-pairing tokens. Minted from the desktop, consumed at
    /// `POST /pair/claim`. Shared (Arc) so the Tauri mint command and the HTTP
    /// claim handler see the same set.
    pairing_tokens: super::pairing::PairingTokenStore,
}

impl GatewayState {
    /// Create new gateway state with provided event sender
    pub fn new(domain_event_tx: broadcast::Sender<DomainEvent>) -> Self {
        Self {
            base_url: "http://localhost:3100".to_string(), // Default
            public_base_url: None,
            network_bind: false,
            sessions: HashMap::new(),
            access_keys: HashMap::new(),
            oauth_tokens: HashMap::new(),
            pending_authorizations: HashMap::new(),
            jwt_signing_secret: None,
            db: None,
            inbound_client_repository: None,
            client_metadata_service: None,
            domain_event_tx,
            auth_disabled: false,
            pairing_tokens: super::pairing::PairingTokenStore::new(),
        }
    }

    /// Shared pairing-token store (mint on desktop, consume at /pair/claim).
    pub fn pairing_tokens(&self) -> super::pairing::PairingTokenStore {
        self.pairing_tokens.clone()
    }

    /// Set the base URL
    pub fn set_base_url(&mut self, base_url: String) {
        info!("[State] Base URL configured: {}", base_url);
        self.base_url = base_url;
    }

    /// Set the configured public base URL (None = local-only / host-derived).
    pub fn set_public_base_url(&mut self, public_base_url: Option<String>) {
        self.public_base_url = public_base_url;
    }

    /// Record whether the gateway is bound to a non-loopback address.
    ///
    /// Invariant: a network-bound gateway always requires inbound auth. If
    /// auth was disabled (loopback convenience) when the bind flips to
    /// network, auth is force-re-enabled here — the engine never allows the
    /// unauthenticated-network combination regardless of what callers or
    /// persisted settings say.
    pub fn set_network_bind(&mut self, network_bind: bool) {
        self.network_bind = network_bind;
        if network_bind && self.auth_disabled {
            warn!(
                "[State] Network bind with inbound auth disabled — force-re-enabling auth \
                 (unauthenticated network exposure is never allowed)"
            );
            self.auth_disabled = false;
        }
    }

    /// Whether inbound MCP auth is disabled — connections may be accepted
    /// without a Bearer token. See [`Self::auth_disabled`] field docs.
    pub fn auth_disabled(&self) -> bool {
        self.auth_disabled
    }

    /// Enable/disable system-wide inbound auth. Called at startup (seed from
    /// settings) and live from the desktop toggle.
    ///
    /// Disabling is rejected while the gateway is bound to a non-loopback
    /// address ([`AuthNetworkConflict`]) — the loopback-only convenience must
    /// never become unauthenticated network exposure. Enabling always
    /// succeeds.
    pub fn set_auth_disabled(&mut self, disabled: bool) -> Result<(), AuthNetworkConflict> {
        if disabled && self.network_bind {
            warn!("[State] Refusing to disable inbound auth: gateway is network-bound");
            return Err(AuthNetworkConflict);
        }
        if self.auth_disabled != disabled {
            info!(
                "[State] Inbound auth {}",
                if disabled { "DISABLED" } else { "enabled" }
            );
        }
        self.auth_disabled = disabled;
        Ok(())
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

    /// Clone the client metadata service handle out of the state.
    ///
    /// Use this (and drop the state guard) before calling
    /// `resolve_client()`: CIMD client ids resolve via an outbound HTTP
    /// fetch (10 s timeout), and `GatewayState`'s write-preferring RwLock
    /// is taken by `oauth_middleware` on every MCP request — holding a
    /// read guard across the fetch can stall all MCP traffic behind one
    /// queued writer.
    pub fn client_metadata_service_arc(&self) -> Option<Arc<ClientMetadataService>> {
        self.client_metadata_service.clone()
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

    #[test]
    fn auth_disabled_defaults_off_and_toggles() {
        let mut state = GatewayState::default();
        // Secure default: auth is required (not disabled).
        assert!(!state.auth_disabled());
        state.set_auth_disabled(true).expect("loopback: allowed");
        assert!(state.auth_disabled());
        state.set_auth_disabled(false).expect("enabling always ok");
        assert!(!state.auth_disabled());
    }

    #[test]
    fn disabling_auth_rejected_on_network_bind() {
        let mut state = GatewayState::default();
        state.set_network_bind(true);
        // The unauthenticated-network combination must be unrepresentable.
        assert_eq!(state.set_auth_disabled(true), Err(AuthNetworkConflict));
        assert!(!state.auth_disabled(), "auth must remain enabled");
        // Re-enabling (a no-op here) still succeeds on a network bind.
        state.set_auth_disabled(false).expect("enabling always ok");
    }

    #[test]
    fn network_bind_force_reenables_disabled_auth() {
        let mut state = GatewayState::default();
        state.set_auth_disabled(true).expect("loopback: allowed");
        assert!(state.auth_disabled());
        // Flipping to a network bind heals the combination in the engine —
        // callers and persisted settings cannot express it either way.
        state.set_network_bind(true);
        assert!(!state.auth_disabled(), "network bind must force auth on");
    }
}
