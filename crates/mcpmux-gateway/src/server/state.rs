//! Gateway state management
//!
//! Manages gateway-level state including:
//! - Client sessions and access keys
//! - OAuth tokens and pending authorizations
//! - JWT signing secrets
//! - Database connections

use std::collections::HashMap;
use std::sync::Arc;
use subtle::ConstantTimeEq;
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
    /// Consent requests waiting for the user (request_id -> request). Kept
    /// apart from `authorization_codes` so a request_id can never be redeemed
    /// at the token endpoint.
    pending_consents: HashMap<String, PendingAuthorization>,
    /// Authorization codes issued after the user approved (code -> request).
    authorization_codes: HashMap<String, PendingAuthorization>,
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
}

/// How long an issued authorization code can be redeemed, in seconds.
pub const AUTHORIZATION_CODE_TTL_SECS: i64 = 600;

/// Upper bound on consent requests waiting for the user at once.
const MAX_PENDING_CONSENTS: usize = 256;

/// Why a consent request could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentLookupError {
    /// No consent request with that id (never existed, or already answered).
    NotFound,
    /// The consent request is past its expiry.
    Expired,
    /// The consent token does not match the one issued for the request.
    TokenMismatch,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
            pending_consents: HashMap::new(),
            authorization_codes: HashMap::new(),
            jwt_signing_secret: None,
            db: None,
            inbound_client_repository: None,
            client_metadata_service: None,
            domain_event_tx,
            auth_disabled: false,
        }
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
    pub fn set_network_bind(&mut self, network_bind: bool) {
        self.network_bind = network_bind;
    }

    /// Whether inbound MCP auth is disabled — connections may be accepted
    /// without a Bearer token. See [`Self::auth_disabled`] field docs.
    pub fn auth_disabled(&self) -> bool {
        self.auth_disabled
    }

    /// Enable/disable system-wide inbound auth. Called at startup (seed from
    /// settings) and live from the desktop toggle.
    pub fn set_auth_disabled(&mut self, disabled: bool) {
        if self.auth_disabled != disabled {
            info!(
                "[State] Inbound auth {}",
                if disabled { "DISABLED" } else { "enabled" }
            );
        }
        self.auth_disabled = disabled;
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

    /// Store a consent request until the user approves or denies it in the
    /// desktop app. Expired requests and codes are dropped first.
    pub fn store_pending_consent(&mut self, request_id: &str, request: PendingAuthorization) {
        self.prune_expired_oauth_entries();
        // Bound memory: when full, the request closest to expiry makes room.
        if self.pending_consents.len() >= MAX_PENDING_CONSENTS {
            if let Some(oldest) = self
                .pending_consents
                .iter()
                .min_by_key(|(_, r)| r.expires_at)
                .map(|(id, _)| id.clone())
            {
                self.pending_consents.remove(&oldest);
            }
        }
        self.pending_consents
            .insert(request_id.to_string(), request);
    }

    /// Look up a consent request without consuming it (for showing the
    /// consent dialog). An expired request is removed and reported as such.
    pub fn lookup_pending_consent(
        &mut self,
        request_id: &str,
    ) -> Result<PendingAuthorization, ConsentLookupError> {
        let request = self
            .pending_consents
            .get(request_id)
            .ok_or(ConsentLookupError::NotFound)?;
        if request.expires_at < unix_now() {
            self.pending_consents.remove(request_id);
            return Err(ConsentLookupError::Expired);
        }
        Ok(request.clone())
    }

    /// Atomically check the consent token and remove the consent request, so
    /// it can be answered exactly once. A wrong token leaves the request in
    /// place.
    pub fn take_pending_consent(
        &mut self,
        request_id: &str,
        consent_token: &str,
    ) -> Result<PendingAuthorization, ConsentLookupError> {
        let request = self
            .pending_consents
            .get(request_id)
            .ok_or(ConsentLookupError::NotFound)?;
        let token_matches = request.consent_token.as_deref().is_some_and(|expected| {
            bool::from(expected.as_bytes().ct_eq(consent_token.as_bytes()))
        });
        if !token_matches {
            return Err(ConsentLookupError::TokenMismatch);
        }
        let request = self
            .pending_consents
            .remove(request_id)
            .ok_or(ConsentLookupError::NotFound)?;
        if request.expires_at < unix_now() {
            return Err(ConsentLookupError::Expired);
        }
        Ok(request)
    }

    /// Remove a consent request without a consent token. Only for the
    /// test-mode HTTP approval endpoint, which has no access to the token.
    #[cfg(feature = "e2e")]
    pub fn take_pending_consent_without_token(
        &mut self,
        request_id: &str,
    ) -> Result<PendingAuthorization, ConsentLookupError> {
        let request = self
            .pending_consents
            .remove(request_id)
            .ok_or(ConsentLookupError::NotFound)?;
        if request.expires_at < unix_now() {
            return Err(ConsentLookupError::Expired);
        }
        Ok(request)
    }

    /// Issue a one-time authorization code for an approved consent request.
    pub fn issue_authorization_code(&mut self, approved: &PendingAuthorization) -> String {
        self.prune_expired_oauth_entries();
        let code = format!("mc_{}", Uuid::new_v4().simple());
        self.authorization_codes.insert(
            code.clone(),
            PendingAuthorization {
                expires_at: unix_now() + AUTHORIZATION_CODE_TTL_SECS,
                consent_token: None,
                ..approved.clone()
            },
        );
        code
    }

    /// Consume an authorization code (one-time use). Returns `None` for an
    /// unknown or expired code; consent request ids are never accepted.
    pub fn consume_authorization_code(&mut self, code: &str) -> Option<PendingAuthorization> {
        let entry = self.authorization_codes.remove(code)?;
        if entry.expires_at < unix_now() {
            debug!("[State] Rejected expired authorization code");
            return None;
        }
        Some(entry)
    }

    fn prune_expired_oauth_entries(&mut self) {
        let now = unix_now();
        self.pending_consents.retain(|_, r| r.expires_at >= now);
        self.authorization_codes.retain(|_, r| r.expires_at >= now);
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

    fn consent_request(expires_at: i64) -> PendingAuthorization {
        PendingAuthorization {
            client_id: "mcp_client".to_string(),
            client_name: Some("Client".to_string()),
            redirect_uri: "http://127.0.0.1:8765/callback".to_string(),
            scope: None,
            state: Some("st".to_string()),
            code_challenge: Some("c".repeat(43)),
            code_challenge_method: Some("S256".to_string()),
            expires_at,
            consent_token: Some("consent-token".to_string()),
        }
    }

    #[test]
    fn consent_request_id_is_not_an_authorization_code() {
        let mut state = GatewayState::default();
        state.store_pending_consent("req-1", consent_request(unix_now() + 300));

        assert!(state.consume_authorization_code("req-1").is_none());
        // The consent request itself is still there for the desktop app.
        assert!(state.lookup_pending_consent("req-1").is_ok());
    }

    #[test]
    fn approved_consent_yields_a_single_use_code() {
        let mut state = GatewayState::default();
        state.store_pending_consent("req-1", consent_request(unix_now() + 300));

        let approved = state
            .take_pending_consent("req-1", "consent-token")
            .expect("token matches");
        let code = state.issue_authorization_code(&approved);

        let redeemed = state
            .consume_authorization_code(&code)
            .expect("code redeems");
        assert_eq!(redeemed.client_id, "mcp_client");
        assert!(redeemed.consent_token.is_none());
        assert!(
            state.consume_authorization_code(&code).is_none(),
            "codes are single-use"
        );
        assert_eq!(
            state.lookup_pending_consent("req-1").err(),
            Some(ConsentLookupError::NotFound),
            "an answered consent request is gone"
        );
    }

    #[test]
    fn wrong_consent_token_leaves_the_request_in_place() {
        let mut state = GatewayState::default();
        state.store_pending_consent("req-1", consent_request(unix_now() + 300));

        assert_eq!(
            state.take_pending_consent("req-1", "forged").err(),
            Some(ConsentLookupError::TokenMismatch)
        );
        assert!(state.take_pending_consent("req-1", "consent-token").is_ok());
    }

    #[test]
    fn expired_consent_requests_are_rejected() {
        let mut state = GatewayState::default();
        state
            .pending_consents
            .insert("old".to_string(), consent_request(unix_now() - 1));

        assert_eq!(
            state.lookup_pending_consent("old").err(),
            Some(ConsentLookupError::Expired)
        );
        assert_eq!(
            state.lookup_pending_consent("old").err(),
            Some(ConsentLookupError::NotFound),
            "an expired request is dropped on lookup"
        );
    }

    #[test]
    fn expired_authorization_codes_are_rejected() {
        let mut state = GatewayState::default();
        let mut entry = consent_request(unix_now() - 1);
        entry.consent_token = None;
        state
            .authorization_codes
            .insert("mc_old".to_string(), entry);

        assert!(state.consume_authorization_code("mc_old").is_none());
    }

    #[test]
    fn pending_consents_are_bounded() {
        let mut state = GatewayState::default();
        let now = unix_now();
        for i in 0..MAX_PENDING_CONSENTS {
            state.store_pending_consent(&format!("req-{i}"), consent_request(now + 300 + i as i64));
        }
        state.store_pending_consent("newest", consent_request(now + 10_000));

        assert_eq!(state.pending_consents.len(), MAX_PENDING_CONSENTS);
        assert!(state.pending_consents.contains_key("newest"));
        assert!(
            !state.pending_consents.contains_key("req-0"),
            "the request closest to expiry made room"
        );
    }

    #[test]
    fn storing_prunes_expired_entries() {
        let mut state = GatewayState::default();
        state
            .pending_consents
            .insert("old".to_string(), consent_request(unix_now() - 1));
        state
            .authorization_codes
            .insert("mc_old".to_string(), consent_request(unix_now() - 1));

        state.store_pending_consent("new", consent_request(unix_now() + 300));

        assert!(!state.pending_consents.contains_key("old"));
        assert!(!state.authorization_codes.contains_key("mc_old"));
        assert!(state.pending_consents.contains_key("new"));
    }

    #[test]
    fn auth_disabled_defaults_off_and_toggles() {
        let mut state = GatewayState::default();
        // Secure default: auth is required (not disabled).
        assert!(!state.auth_disabled());
        state.set_auth_disabled(true);
        assert!(state.auth_disabled());
        state.set_auth_disabled(false);
        assert!(!state.auth_disabled());
    }
}
