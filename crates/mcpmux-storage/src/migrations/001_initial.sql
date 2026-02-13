-- McpMux Database Schema v4 (Unified Client Model)
-- 
-- Architecture:
-- - Server definitions come from REGISTRY (JSON/API), NOT database
-- - Database stores user data: installations, configs, credentials, features
-- 
-- OAuth Model (Unified):
-- - clients: Unified table for OAuth registration (DCR) + user preferences + grants
-- - backend_oauth_registrations: McpMux's OAuth client credentials with remote MCP servers
-- - oauth_authorization_codes: Pending auth codes during PKCE flow
-- - oauth_tokens: Access/refresh tokens we issue to clients
--
-- Space-scoping model:
-- - installed_servers: Per-space server installations with user's input values
-- - credentials: OAuth tokens, API keys per (space, server)
-- - server_features: Discovered tools/prompts per (space, server)
-- - feature_sets: Permission bundles per space
-- - client_grants: Per-space permissions (client + space + feature_set)

-- ============================================================================
-- SPACES
-- ============================================================================

CREATE TABLE IF NOT EXISTS spaces (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    icon TEXT,
    description TEXT,
    is_default INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_spaces_is_default ON spaces(is_default);

-- Create default space
INSERT OR IGNORE INTO spaces (id, name, icon, description, is_default, sort_order, created_at, updated_at)
VALUES (
    '00000000-0000-0000-0000-000000000001',
    'My Space',
    '🏠',
    'Default workspace for your MCP servers',
    1,
    0,
    datetime('now'),
    datetime('now')
);

-- ============================================================================
-- INSTALLED SERVERS (Per-Space)
-- Tracks which registry servers are installed in each space
-- ============================================================================

CREATE TABLE IF NOT EXISTS installed_servers (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    server_id TEXT NOT NULL,  -- Registry server ID
    
    -- Cached server info for offline operation
    server_name TEXT,                          -- Display name from definition
    cached_definition TEXT,                    -- Full ServerDefinition JSON
    
    -- User's input values (encrypted JSON)
    input_values TEXT NOT NULL DEFAULT '{}',
    
    enabled INTEGER NOT NULL DEFAULT 0,
    env_overrides TEXT NOT NULL DEFAULT '{}',  -- JSON
    args_append TEXT NOT NULL DEFAULT '[]',    -- JSON array
    extra_headers TEXT NOT NULL DEFAULT '{}',  -- JSON - arbitrary HTTP headers for HTTP/SSE transports
    
    -- OAuth connection state (persistent)
    oauth_connected INTEGER NOT NULL DEFAULT 0,

    -- Installation source tracking
    source TEXT NOT NULL DEFAULT 'registry',  -- 'registry', 'user_config:/path/to/file.json', 'manual_entry'

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    UNIQUE(space_id, server_id),
    FOREIGN KEY (space_id) REFERENCES spaces(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_installed_servers_space ON installed_servers(space_id);
CREATE INDEX IF NOT EXISTS idx_installed_servers_enabled ON installed_servers(space_id, enabled);
CREATE INDEX IF NOT EXISTS idx_installed_servers_source ON installed_servers(source);

-- ============================================================================
-- CREDENTIALS (Per-Space, Typed Rows)
-- Each token/key is a separate row with its own type and expiry.
-- One row per (space, server, credential_type).
-- ============================================================================

CREATE TABLE IF NOT EXISTS credentials (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    server_id TEXT NOT NULL,
    credential_type TEXT NOT NULL,  -- 'access_token', 'refresh_token', 'api_key', 'basic_auth_user', 'basic_auth_pass'

    -- Only the secret value is encrypted (AES-256-GCM). Not a JSON blob.
    credential_value TEXT NOT NULL,

    -- Metadata stored as plaintext for queryability
    expires_at TEXT,       -- RFC3339, nullable (refresh tokens / API keys may not expire)
    token_type TEXT,       -- 'Bearer', etc. (only for access_token)
    scope TEXT,            -- OAuth scope (only for access_token)

    last_used_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    UNIQUE(space_id, server_id, credential_type),
    FOREIGN KEY (space_id) REFERENCES spaces(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_credentials_space_server ON credentials(space_id, server_id);
CREATE INDEX IF NOT EXISTS idx_credentials_type ON credentials(space_id, server_id, credential_type);
CREATE INDEX IF NOT EXISTS idx_credentials_expiry ON credentials(credential_type, expires_at);

-- ============================================================================
-- OUTBOUND OAUTH CLIENTS
-- McpMux acting as OAuth CLIENT when connecting TO backend MCP servers
-- (e.g., McpMux → Cloudflare, Atlassian, GitHub servers that require OAuth)
-- 
-- This is for OUTBOUND connections where McpMux authenticates TO backend servers.
-- (For INBOUND: external apps → McpMux, see inbound_oauth_clients & inbound_mcp_clients)
--
-- Stores McpMux's client_id on each backend server (from DCR with that server).
-- Also caches OAuth metadata to avoid RMCP discovery failures on non-spec-compliant servers.
-- ============================================================================

CREATE TABLE IF NOT EXISTS outbound_oauth_clients (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    server_id TEXT NOT NULL,   -- Registry server ID (e.g., "cloudflare-mcp")
    server_url TEXT NOT NULL,  -- Base URL for backend MCP server
    client_id TEXT NOT NULL,   -- McpMux's client_id on that backend server (from DCR)
    redirect_uri TEXT,         -- Callback URI used during DCR (for port change detection)
    metadata_json TEXT,        -- Cached OAuth metadata as JSON (for RMCP's set_metadata)

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    UNIQUE(space_id, server_id),
    FOREIGN KEY (space_id) REFERENCES spaces(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_outbound_oauth_space_server ON outbound_oauth_clients(space_id, server_id);

-- ============================================================================
-- SERVER FEATURES (Per-Space)
-- Discovered features (tools, prompts, resources) from connected servers
-- ============================================================================

CREATE TABLE IF NOT EXISTS server_features (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    server_id TEXT NOT NULL,
    
    feature_type TEXT NOT NULL,  -- 'tool', 'prompt', 'resource'
    feature_name TEXT NOT NULL,
    display_name TEXT,
    description TEXT,
    raw_json TEXT,  -- Complete JSON from backend MCP server (future-proof)
    
    discovered_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    is_available INTEGER NOT NULL DEFAULT 1,
    
    UNIQUE(space_id, server_id, feature_type, feature_name),
    FOREIGN KEY (space_id) REFERENCES spaces(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_server_features_space ON server_features(space_id);
CREATE INDEX IF NOT EXISTS idx_server_features_server ON server_features(space_id, server_id);

-- ============================================================================
-- FEATURE SETS (Per-Space)
-- Permission bundles for tools/prompts/resources
-- ============================================================================

CREATE TABLE IF NOT EXISTS feature_sets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    icon TEXT,
    space_id TEXT,  -- NULL = global/builtin
    
    feature_set_type TEXT NOT NULL DEFAULT 'custom',  -- 'all', 'default', 'server-all', 'custom'
    server_id TEXT,  -- For 'server-all' type
    
    is_builtin INTEGER NOT NULL DEFAULT 0,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    
    FOREIGN KEY (space_id) REFERENCES spaces(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_feature_sets_space ON feature_sets(space_id);
CREATE INDEX IF NOT EXISTS idx_feature_sets_type ON feature_sets(feature_set_type);

-- Default feature sets for default space
INSERT OR IGNORE INTO feature_sets (id, name, description, icon, space_id, feature_set_type, is_builtin, created_at, updated_at)
VALUES (
    'fs_all_00000000-0000-0000-0000-000000000001',
    'All Features',
    'All features from all connected MCP servers in this space',
    '🌐',
    '00000000-0000-0000-0000-000000000001',
    'all',
    1,
    datetime('now'),
    datetime('now')
);

INSERT OR IGNORE INTO feature_sets (id, name, description, icon, space_id, feature_set_type, is_builtin, created_at, updated_at)
VALUES (
    'fs_default_00000000-0000-0000-0000-000000000001',
    'Default',
    'Features automatically granted to all connected clients in this space',
    '⭐',
    '00000000-0000-0000-0000-000000000001',
    'default',
    1,
    datetime('now'),
    datetime('now')
);

-- ============================================================================
-- FEATURE SET MEMBERS (Composition)
-- ============================================================================

CREATE TABLE IF NOT EXISTS feature_set_members (
    id TEXT PRIMARY KEY,
    feature_set_id TEXT NOT NULL,
    member_type TEXT NOT NULL,  -- 'feature_set' or 'feature'
    member_id TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'include',  -- 'include' or 'exclude'
    created_at TEXT NOT NULL,
    UNIQUE(feature_set_id, member_type, member_id),
    FOREIGN KEY (feature_set_id) REFERENCES feature_sets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_fsm_feature_set ON feature_set_members(feature_set_id);

-- ============================================================================
-- INBOUND CLIENTS (Unified OAuth + MCP Model)
-- Applications connecting TO McpMux (e.g., Cursor, VS Code, Claude Desktop)
--
-- Supports three MCP registration approaches (per MCP spec 2025-11-25):
-- 1. Client ID Metadata Documents (CIMD) - client_id is a URL
-- 2. Dynamic Client Registration (DCR) - server generates client_id
-- 3. Pre-registration - server pre-configures client_id
--
-- This unified table stores:
-- - OAuth registration data (redirect_uris, grant_types, logo_uri, etc.)
-- - MCP client preferences (connection_mode, grants, locked_space_id)
-- - Registration metadata (type, caching info for CIMD)
-- ============================================================================

CREATE TABLE IF NOT EXISTS inbound_clients (
    client_id TEXT PRIMARY KEY,        -- Can be URL (CIMD), generated ID (DCR), or pre-configured
    
    -- Registration Metadata
    registration_type TEXT NOT NULL CHECK(registration_type IN ('cimd', 'dcr', 'preregistered')),
    
    -- Client Identity
    client_name TEXT NOT NULL,
    client_alias TEXT,                 -- User-friendly override name
    
    -- RFC 7591 OAuth Client Metadata
    logo_uri TEXT,                     -- URL for client's logo
    client_uri TEXT,                   -- URL of client's homepage
    software_id TEXT,                  -- Unique identifier (e.g., "com.cursor.app")
    software_version TEXT,             -- Version of the client software
    
    -- OAuth Protocol Fields
    redirect_uris TEXT NOT NULL,       -- JSON array of allowed redirect URIs
    grant_types TEXT NOT NULL,         -- JSON array (e.g., ["authorization_code", "refresh_token"])
    response_types TEXT NOT NULL,      -- JSON array (e.g., ["code"])
    token_endpoint_auth_method TEXT NOT NULL,  -- Usually "none" for public clients
    scope TEXT,                        -- Space-separated scopes
    
    -- CIMD-Specific Fields (only used when registration_type='cimd')
    metadata_url TEXT,                 -- URL where metadata was fetched (same as client_id for CIMD)
    metadata_cached_at TEXT,           -- When we last fetched the metadata
    metadata_cache_ttl INTEGER DEFAULT 3600,  -- Cache duration in seconds (default 1 hour)
    
    -- MCP Client Preferences
    connection_mode TEXT NOT NULL DEFAULT 'follow_active',  -- How client resolves spaces
    locked_space_id TEXT,              -- If locked to specific space
    
    -- Permissions: JSON map of space_id -> [feature_set_ids]
    grants TEXT,

    -- Approval status (user must explicitly approve clients)
    approved INTEGER NOT NULL DEFAULT 0,

    -- Timestamps
    last_seen TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    FOREIGN KEY (locked_space_id) REFERENCES spaces(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_inbound_clients_type ON inbound_clients(registration_type);
CREATE INDEX IF NOT EXISTS idx_inbound_clients_name ON inbound_clients(client_name);
CREATE INDEX IF NOT EXISTS idx_inbound_clients_metadata_url ON inbound_clients(metadata_url) WHERE metadata_url IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_inbound_clients_approved ON inbound_clients(approved) WHERE approved = 1;

-- ============================================================================
-- CLIENT GRANTS (Per-Space permissions for INBOUND clients)
-- Maps which feature sets each client can access in each space
-- ============================================================================

CREATE TABLE IF NOT EXISTS client_grants (
    client_id TEXT NOT NULL,        -- References inbound_clients.client_id
    space_id TEXT NOT NULL,
    feature_set_id TEXT NOT NULL,
    PRIMARY KEY (client_id, space_id, feature_set_id),
    FOREIGN KEY (client_id) REFERENCES inbound_clients(client_id) ON DELETE CASCADE,
    FOREIGN KEY (space_id) REFERENCES spaces(id) ON DELETE CASCADE,
    FOREIGN KEY (feature_set_id) REFERENCES feature_sets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_client_grants_client ON client_grants(client_id);
CREATE INDEX IF NOT EXISTS idx_client_grants_space ON client_grants(space_id);

-- ============================================================================
-- OAUTH AUTHORIZATION CODES (Gateway's OAuth Server - INBOUND)
-- Pending authorization codes for PKCE flow
-- Issued to INBOUND clients during authorization flow
-- ============================================================================

CREATE TABLE IF NOT EXISTS oauth_authorization_codes (
    code TEXT PRIMARY KEY,
    client_id TEXT NOT NULL,           -- References inbound_clients.client_id
    redirect_uri TEXT NOT NULL,
    scope TEXT,
    code_challenge TEXT NOT NULL,
    code_challenge_method TEXT NOT NULL DEFAULT 'S256',
    state TEXT,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (client_id) REFERENCES inbound_clients(client_id) ON DELETE CASCADE
);

-- ============================================================================
-- OAUTH TOKENS (Gateway's OAuth Server - INBOUND)
-- Access/refresh tokens we issue to INBOUND clients
-- ============================================================================

CREATE TABLE IF NOT EXISTS oauth_tokens (
    id TEXT PRIMARY KEY,
    client_id TEXT NOT NULL,           -- References inbound_clients.client_id
    token_type TEXT NOT NULL,          -- 'access' or 'refresh'
    token_hash TEXT NOT NULL,          -- SHA-256 hash of token
    scope TEXT,
    expires_at TEXT,
    revoked INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    -- For refresh tokens, link to parent access token
    parent_token_id TEXT,
    FOREIGN KEY (client_id) REFERENCES inbound_clients(client_id) ON DELETE CASCADE,
    FOREIGN KEY (parent_token_id) REFERENCES oauth_tokens(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_oauth_tokens_hash ON oauth_tokens(token_hash);
CREATE INDEX IF NOT EXISTS idx_oauth_tokens_client ON oauth_tokens(client_id);

-- ============================================================================
-- SETTINGS
-- ============================================================================

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- ============================================================================
-- APP SETTINGS
-- Key-value store for application-wide settings.
-- ============================================================================

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Seed default settings
INSERT OR IGNORE INTO app_settings (key, value, updated_at)
VALUES
    ('gateway.auto_start', 'true', datetime('now'));
