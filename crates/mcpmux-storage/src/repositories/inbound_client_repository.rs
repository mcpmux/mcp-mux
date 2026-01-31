//! Inbound Client Repository for persistent storage of OAuth clients, codes, and tokens.
//!
//! This module provides database-backed storage for INBOUND clients (apps connecting TO McpMux):
//! - Registered clients (via CIMD, DCR, or pre-registration)
//! - Authorization codes (temporary, for PKCE flow)
//! - Access and refresh tokens
//!
//! Supports three MCP registration approaches per MCP spec 2025-11-25:
//! 1. Client ID Metadata Documents (CIMD) - client_id is a URL
//! 2. Dynamic Client Registration (DCR) - server generates client_id
//! 3. Pre-registration - server pre-configures client_id

use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::Database;

/// Client registration type (per MCP spec 2025-11-25)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegistrationType {
    /// Client ID Metadata Document - client_id is a URL
    Cimd,
    /// Dynamic Client Registration - server generates client_id
    Dcr,
    /// Pre-registered - server pre-configures client_id
    Preregistered,
}

impl RegistrationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RegistrationType::Cimd => "cimd",
            RegistrationType::Dcr => "dcr",
            RegistrationType::Preregistered => "preregistered",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "cimd" => Some(RegistrationType::Cimd),
            "dcr" => Some(RegistrationType::Dcr),
            "preregistered" => Some(RegistrationType::Preregistered),
            _ => None,
        }
    }
}

/// Inbound client (unified OAuth + MCP model)
/// 
/// Represents both the OAuth registration and MCP client configuration
/// in a unified model, supporting all three MCP registration approaches.
/// 
/// ## Client Identification
/// Per RFC 7591, clients self-identify via metadata they provide:
/// - `logo_uri`: Client's logo (use this for display)
/// - `software_id`: Unique identifier (e.g., "com.cursor.app")
/// - `client_name`: Human-readable name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundClient {
    pub client_id: String,
    pub registration_type: RegistrationType,
    pub client_name: String,
    pub client_alias: Option<String>,  // User-friendly override name
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
    pub token_endpoint_auth_method: String,
    pub scope: Option<String>,
    
    // Approval status - true if user has explicitly approved this client
    pub approved: bool,
    
    // RFC 7591 Client Metadata (use these for client identification)
    pub logo_uri: Option<String>,  // URL for client's logo
    pub client_uri: Option<String>,  // URL of client's homepage
    pub software_id: Option<String>,  // Unique software identifier (e.g., "com.cursor.app")
    pub software_version: Option<String>,  // Version of the client software
    
    // CIMD-specific fields (only used for registration_type=Cimd)
    pub metadata_url: Option<String>,  // URL where metadata was fetched
    pub metadata_cached_at: Option<String>,  // When we last fetched
    pub metadata_cache_ttl: Option<i64>,  // Cache duration in seconds
    
    // MCP client preferences
    pub connection_mode: String,  // 'follow_active', 'locked', 'ask_on_change'
    pub locked_space_id: Option<String>,
    pub last_seen: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Authorization code (pending exchange)
#[derive(Debug, Clone)]
pub struct AuthorizationCode {
    pub code: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub expires_at: String,
    pub created_at: String,
}

/// Token type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Access,
    Refresh,
}

impl TokenType {
    fn as_str(&self) -> &'static str {
        match self {
            TokenType::Access => "access",
            TokenType::Refresh => "refresh",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "access" => Some(TokenType::Access),
            "refresh" => Some(TokenType::Refresh),
            _ => None,
        }
    }
}

/// Stored token record
#[derive(Debug, Clone)]
pub struct TokenRecord {
    pub id: String,
    pub client_id: String,
    pub token_type: TokenType,
    pub token_hash: String,
    pub scope: Option<String>,
    pub expires_at: Option<String>,
    pub revoked: bool,
    pub created_at: String,
    pub parent_token_id: Option<String>,
}

/// OAuth Repository with database persistence
pub struct InboundClientRepository {
    db: Arc<Mutex<Database>>,
}

impl InboundClientRepository {
    /// Create a new inbound client repository with a database
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    // =========================================================================
    // Private Helper: Row Mapping (DRY)
    // =========================================================================

    /// Map a SQL row to InboundClient
    /// 
    /// Expects columns in this exact order (as returned by our queries):
    /// 0: client_id, 1: registration_type, 2: client_name, 3: client_alias,
    /// 4: logo_uri, 5: client_uri, 6: software_id, 7: software_version,
    /// 8: redirect_uris, 9: grant_types, 10: response_types, 11: token_endpoint_auth_method, 12: scope,
    /// 13: metadata_url, 14: metadata_cached_at, 15: metadata_cache_ttl,
    /// 16: connection_mode, 17: locked_space_id, 18: last_seen, 19: created_at, 20: updated_at, 21: approved
    fn map_row_to_client(row: &rusqlite::Row) -> rusqlite::Result<InboundClient> {
        let registration_type_str: String = row.get(1)?;
        let redirect_uris_json: Option<String> = row.get(8)?;
        let grant_types_json: Option<String> = row.get(9)?;
        let response_types_json: Option<String> = row.get(10)?;
        let approved_int: i32 = row.get::<_, Option<i32>>(21)?.unwrap_or(0);
        
        Ok(InboundClient {
            client_id: row.get(0)?,
            registration_type: RegistrationType::from_str(&registration_type_str)
                .unwrap_or(RegistrationType::Dcr),
            client_name: row.get(2)?,
            client_alias: row.get(3)?,
            logo_uri: row.get(4)?,
            client_uri: row.get(5)?,
            software_id: row.get(6)?,
            software_version: row.get(7)?,
            redirect_uris: redirect_uris_json
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default(),
            grant_types: grant_types_json
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default(),
            response_types: response_types_json
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default(),
            token_endpoint_auth_method: row
                .get::<_, Option<String>>(11)?
                .unwrap_or_else(|| "none".to_string()),
            scope: row.get(12)?,
            metadata_url: row.get(13)?,
            metadata_cached_at: row.get(14)?,
            metadata_cache_ttl: row.get(15)?,
            connection_mode: row
                .get::<_, Option<String>>(16)?
                .unwrap_or_else(|| "follow_active".to_string()),
            locked_space_id: row.get(17)?,
            last_seen: row.get(18)?,
            created_at: row.get(19)?,
            updated_at: row.get(20)?,
            approved: approved_int != 0,
        })
    }

    /// Standard column selection for InboundClient queries
    const CLIENT_COLUMNS: &'static str = 
        "client_id, registration_type, client_name, client_alias,
         logo_uri, client_uri, software_id, software_version,
         redirect_uris, grant_types, response_types, token_endpoint_auth_method, scope,
         metadata_url, metadata_cached_at, metadata_cache_ttl,
         connection_mode, locked_space_id, last_seen, created_at, updated_at, approved";

    // =========================================================================
    // Client Operations (unified inbound_clients table)
    // =========================================================================

    /// Register or update an inbound client (supports CIMD, DCR, pre-registered)
    pub async fn save_client(&self, client: &InboundClient) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "INSERT INTO inbound_clients (
                client_id, registration_type, client_name, client_alias,
                logo_uri, client_uri, software_id, software_version,
                redirect_uris, grant_types, response_types, token_endpoint_auth_method, scope,
                metadata_url, metadata_cached_at, metadata_cache_ttl,
                connection_mode, locked_space_id,
                last_seen, created_at, updated_at, approved
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
             ON CONFLICT(client_id) DO UPDATE SET
                registration_type = ?2, client_name = ?3, client_alias = ?4,
                logo_uri = ?5, client_uri = ?6, software_id = ?7, software_version = ?8,
                redirect_uris = ?9, grant_types = ?10, response_types = ?11,
                token_endpoint_auth_method = ?12, scope = ?13,
                metadata_url = ?14, metadata_cached_at = ?15, metadata_cache_ttl = ?16,
                connection_mode = ?17, locked_space_id = ?18,
                last_seen = ?19, updated_at = ?21, approved = ?22",
            params![
                client.client_id,
                client.registration_type.as_str(),
                client.client_name,
                client.client_alias,
                client.logo_uri,
                client.client_uri,
                client.software_id,
                client.software_version,
                serde_json::to_string(&client.redirect_uris)?,
                serde_json::to_string(&client.grant_types)?,
                serde_json::to_string(&client.response_types)?,
                client.token_endpoint_auth_method,
                client.scope,
                client.metadata_url,
                client.metadata_cached_at,
                client.metadata_cache_ttl,
                client.connection_mode,
                client.locked_space_id,
                client.last_seen,
                client.created_at,
                client.updated_at,
                client.approved as i32,
            ],
        )?;
        debug!("[OAuth] Saved client: {} ({})", client.client_name, client.client_id);
        Ok(())
    }

    /// Get a client by ID
    pub async fn get_client(&self, client_id: &str) -> Result<Option<InboundClient>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM inbound_clients WHERE client_id = ?1",
            Self::CLIENT_COLUMNS
        ))?;

        let result = stmt.query_row(params![client_id], Self::map_row_to_client);

        match result {
            Ok(client) => Ok(Some(client)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Find client by name (for idempotent DCR)
    /// 
    /// Allows a client to register with different redirect_uris
    pub async fn find_client_by_name(&self, name: &str) -> Result<Option<InboundClient>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM inbound_clients WHERE client_name = ?1",
            Self::CLIENT_COLUMNS
        ))?;

        let result = stmt.query_row(params![name], Self::map_row_to_client);

        match result {
            Ok(client) => Ok(Some(client)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Validate redirect URI for a client
    pub async fn validate_redirect_uri(&self, client_id: &str, redirect_uri: &str) -> Result<bool> {
        if let Some(client) = self.get_client(client_id).await? {
            Ok(client.redirect_uris.iter().any(|uri| uri == redirect_uri))
        } else {
            Ok(false)
        }
    }

    /// List all registered OAuth clients
    pub async fn list_clients(&self) -> Result<Vec<InboundClient>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM inbound_clients ORDER BY created_at DESC",
            Self::CLIENT_COLUMNS
        ))?;

        let clients = stmt.query_map([], Self::map_row_to_client)?;

        let result: Vec<InboundClient> = clients.collect::<Result<_, _>>()?;
        debug!("[OAuth] Listed {} clients", result.len());
        Ok(result)
    }

    /// Update a client's last_seen timestamp
    pub async fn update_client_last_seen(&self, client_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        conn.execute(
            "UPDATE inbound_clients SET last_seen = ?1, updated_at = ?1 WHERE client_id = ?2",
            params![now, client_id],
        )?;
        debug!("[OAuth] Updated last_seen for client: {}", client_id);
        Ok(())
    }
    
    /// Mark a client as approved by the user
    /// 
    /// This is called when user explicitly approves the OAuth consent.
    /// Only approved clients get silent re-authentication.
    pub async fn approve_client(&self, client_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        conn.execute(
            "UPDATE inbound_clients SET approved = 1, updated_at = ?1 WHERE client_id = ?2",
            params![now, client_id],
        )?;
        debug!("[OAuth] Approved client: {}", client_id);
        Ok(())
    }
    
    /// Check if a client has been approved by the user
    pub async fn is_client_approved(&self, client_id: &str) -> Result<bool> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let approved: i32 = conn.query_row(
            "SELECT approved FROM inbound_clients WHERE client_id = ?1",
            params![client_id],
            |row| row.get(0),
        ).unwrap_or(0);
        Ok(approved != 0)
    }
    
    /// Merge new redirect URIs with existing ones for a client
    /// Avoids duplicates and preserves existing URIs
    pub async fn merge_redirect_uris(&self, client_id: &str, new_uris: Vec<String>) -> Result<Vec<String>> {
        // Get existing client
        let existing_client = self.get_client(client_id).await?;
        let mut merged_uris = existing_client
            .map(|c| c.redirect_uris)
            .unwrap_or_default();
        
        // Add new URIs (avoid duplicates)
        for uri in new_uris {
            if !merged_uris.contains(&uri) {
                merged_uris.push(uri);
            }
        }
        
        // Update in database
        let db = self.db.lock().await;
        let conn = db.connection();
        let uris_json = serde_json::to_string(&merged_uris)?;
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        conn.execute(
            "UPDATE inbound_clients SET redirect_uris = ?1, updated_at = ?2 WHERE client_id = ?3",
            params![uris_json, now, client_id],
        )?;
        
        debug!("[OAuth] Merged redirect_uris for client: {} -> {:?}", client_id, merged_uris);
        Ok(merged_uris)
    }

    /// Update client configuration settings
    pub async fn update_client_settings(
        &self,
        client_id: &str,
        client_alias: Option<String>,
        connection_mode: Option<String>,
        locked_space_id: Option<Option<String>>,  // None = don't change, Some(None) = clear, Some(Some(x)) = set
    ) -> Result<Option<InboundClient>> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        
        // Update timestamp
        {
            let db = self.db.lock().await;
            let conn = db.connection();
            conn.execute(
                "UPDATE inbound_clients SET updated_at = ?1 WHERE client_id = ?2",
                params![now, client_id],
            )?;
        }
        
        // Update alias if provided
        if let Some(alias) = &client_alias {
            let db = self.db.lock().await;
            let conn = db.connection();
            conn.execute(
                "UPDATE inbound_clients SET client_alias = ?1 WHERE client_id = ?2",
                params![alias, client_id],
            )?;
        }
        
        // Update connection mode if provided
        if let Some(mode) = &connection_mode {
            let db = self.db.lock().await;
            let conn = db.connection();
            conn.execute(
                "UPDATE inbound_clients SET connection_mode = ?1 WHERE client_id = ?2",
                params![mode, client_id],
            )?;
        }
        
        // Update locked_space_id if provided
        if let Some(space_id) = &locked_space_id {
            let db = self.db.lock().await;
            let conn = db.connection();
            conn.execute(
                "UPDATE inbound_clients SET locked_space_id = ?1 WHERE client_id = ?2",
                params![space_id, client_id],
            )?;
        }
        
        debug!("[OAuth] Updated settings for client: {}", client_id);
        
        // Return updated client
        self.get_client(client_id).await
    }

    /// Delete a client and all associated tokens
    pub async fn delete_client(&self, client_id: &str) -> Result<bool> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        // Tokens and codes will be deleted via CASCADE
        let rows = conn.execute(
            "DELETE FROM inbound_clients WHERE client_id = ?1",
            params![client_id],
        )?;
        
        if rows > 0 {
            info!("[OAuth] Deleted client: {}", client_id);
            Ok(true)
        } else {
            debug!("[OAuth] Client not found for deletion: {}", client_id);
            Ok(false)
        }
    }

    // =========================================================================
    // Authorization Code Operations
    // =========================================================================

    /// Save an authorization code
    pub async fn save_authorization_code(&self, code: &AuthorizationCode) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "INSERT INTO oauth_authorization_codes 
                (code, client_id, redirect_uri, scope, code_challenge, code_challenge_method, expires_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                code.code,
                code.client_id,
                code.redirect_uri,
                code.scope,
                code.code_challenge,
                code.code_challenge_method,
                code.expires_at,
                code.created_at,
            ],
        )?;
        debug!("[OAuth] Saved authorization code for client: {}", code.client_id);
        Ok(())
    }

    /// Get and consume an authorization code (one-time use)
    pub async fn consume_authorization_code(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        // Get the code
        let mut stmt = conn.prepare(
            "SELECT code, client_id, redirect_uri, scope, code_challenge, code_challenge_method, expires_at, created_at
             FROM oauth_authorization_codes WHERE code = ?1"
        )?;

        let result = stmt.query_row(params![code], |row| {
            Ok(AuthorizationCode {
                code: row.get(0)?,
                client_id: row.get(1)?,
                redirect_uri: row.get(2)?,
                scope: row.get(3)?,
                code_challenge: row.get(4)?,
                code_challenge_method: row.get(5)?,
                expires_at: row.get(6)?,
                created_at: row.get(7)?,
            })
        });

        match result {
            Ok(auth_code) => {
                // Delete the code (one-time use)
                conn.execute("DELETE FROM oauth_authorization_codes WHERE code = ?1", params![code])?;
                debug!("[OAuth] Consumed authorization code for client: {}", auth_code.client_id);
                Ok(Some(auth_code))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Clean up expired authorization codes
    pub async fn cleanup_expired_codes(&self) -> Result<usize> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let deleted = conn.execute(
            "DELETE FROM oauth_authorization_codes WHERE expires_at < datetime('now')",
            [],
        )?;
        if deleted > 0 {
            info!("[OAuth] Cleaned up {} expired authorization codes", deleted);
        }
        Ok(deleted)
    }

    // =========================================================================
    // Token Operations
    // =========================================================================

    /// Hash a token for storage (we never store raw tokens)
    pub fn hash_token(token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Save a token record
    pub async fn save_token(&self, record: &TokenRecord) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "INSERT INTO oauth_tokens (id, client_id, token_type, token_hash, scope, expires_at, revoked, created_at, parent_token_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.id,
                record.client_id,
                record.token_type.as_str(),
                record.token_hash,
                record.scope,
                record.expires_at,
                record.revoked as i32,
                record.created_at,
                record.parent_token_id,
            ],
        )?;
        debug!("[OAuth] Saved {} token for client: {}", record.token_type.as_str(), record.client_id);
        Ok(())
    }

    /// Find a token by its hash
    pub async fn find_token_by_hash(&self, token_hash: &str) -> Result<Option<TokenRecord>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, client_id, token_type, token_hash, scope, expires_at, revoked, created_at, parent_token_id
             FROM oauth_tokens WHERE token_hash = ?1"
        )?;

        let result = stmt.query_row(params![token_hash], |row| {
            let token_type_str: String = row.get(2)?;
            let revoked: i32 = row.get(6)?;
            
            Ok(TokenRecord {
                id: row.get(0)?,
                client_id: row.get(1)?,
                token_type: TokenType::from_str(&token_type_str).unwrap_or(TokenType::Access),
                token_hash: row.get(3)?,
                scope: row.get(4)?,
                expires_at: row.get(5)?,
                revoked: revoked != 0,
                created_at: row.get(7)?,
                parent_token_id: row.get(8)?,
            })
        });

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Validate a token (check hash, expiration, revocation)
    pub async fn validate_token(&self, token: &str) -> Result<Option<TokenRecord>> {
        let hash = Self::hash_token(token);
        
        if let Some(record) = self.find_token_by_hash(&hash).await? {
            // Check if revoked
            if record.revoked {
                debug!("[OAuth] Token rejected: revoked");
                return Ok(None);
            }

            // Check if expired
            if let Some(expires_at) = &record.expires_at {
                let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                if expires_at < &now {
                    debug!("[OAuth] Token rejected: expired");
                    return Ok(None);
                }
            }

            Ok(Some(record))
        } else {
            debug!("[OAuth] Token not found in database");
            Ok(None)
        }
    }

    /// Revoke a token (and all child tokens)
    pub async fn revoke_token(&self, token_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        // Revoke the token itself
        conn.execute(
            "UPDATE oauth_tokens SET revoked = 1 WHERE id = ?1",
            params![token_id],
        )?;

        // Revoke all child tokens (e.g., access tokens from this refresh token)
        conn.execute(
            "UPDATE oauth_tokens SET revoked = 1 WHERE parent_token_id = ?1",
            params![token_id],
        )?;

        info!("[OAuth] Revoked token: {}", token_id);
        Ok(())
    }

    /// Revoke all tokens for a client
    pub async fn revoke_client_tokens(&self, client_id: &str) -> Result<usize> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let count = conn.execute(
            "UPDATE oauth_tokens SET revoked = 1 WHERE client_id = ?1",
            params![client_id],
        )?;
        info!("[OAuth] Revoked {} tokens for client: {}", count, client_id);
        Ok(count)
    }

    /// Clean up expired tokens
    pub async fn cleanup_expired_tokens(&self) -> Result<usize> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let deleted = conn.execute(
            "DELETE FROM oauth_tokens WHERE expires_at < datetime('now') AND expires_at IS NOT NULL",
            [],
        )?;
        if deleted > 0 {
            info!("[OAuth] Cleaned up {} expired tokens", deleted);
        }
        Ok(deleted)
    }

    // =========================================================================
    // Client Grants (Feature Set Permissions)
    // =========================================================================

    /// Grant a feature set to a client in a specific space
    pub async fn grant_feature_set(
        &self,
        client_id: &str,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        conn.execute(
            "INSERT OR IGNORE INTO client_grants (client_id, space_id, feature_set_id)
             VALUES (?1, ?2, ?3)",
            params![client_id, space_id, feature_set_id],
        )?;
        
        Ok(())
    }

    /// Revoke a feature set from a client in a specific space
    pub async fn revoke_feature_set(
        &self,
        client_id: &str,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        conn.execute(
            "DELETE FROM client_grants 
             WHERE client_id = ?1 AND space_id = ?2 AND feature_set_id = ?3",
            params![client_id, space_id, feature_set_id],
        )?;
        
        Ok(())
    }

    /// Get all grants for a client in a specific space
    pub async fn get_grants_for_space(
        &self,
        client_id: &str,
        space_id: &str,
    ) -> Result<Vec<String>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        let mut stmt = conn.prepare(
            "SELECT feature_set_id FROM client_grants 
             WHERE client_id = ?1 AND space_id = ?2",
        )?;
        
        let grants = stmt
            .query_map(params![client_id, space_id], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        
        Ok(grants)
    }

    /// Get all grants for a client across all spaces
    pub async fn get_all_grants(
        &self,
        client_id: &str,
    ) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        let mut stmt = conn.prepare(
            "SELECT space_id, feature_set_id FROM client_grants 
             WHERE client_id = ?1
             ORDER BY space_id",
        )?;
        
        let mut grants: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        
        let rows = stmt.query_map(params![client_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        
        for row in rows {
            let (space_id, feature_set_id) = row?;
            grants
                .entry(space_id)
                .or_default()
                .push(feature_set_id);
        }
        
        Ok(grants)
    }

    // =========================================================================
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_token() {
        let hash1 = InboundClientRepository::hash_token("test_token");
        let hash2 = InboundClientRepository::hash_token("test_token");
        let hash3 = InboundClientRepository::hash_token("different_token");
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA-256 hex = 64 chars
    }
}
