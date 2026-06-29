-- Migration 020: Inbound client API keys
--
-- Long-lived, host-issued bearer credentials for manually-registered
-- (preregistered) inbound clients, so headless/remote clients can authenticate
-- WITHOUT the interactive OAuth consent flow (the mcpmux:// deep link only
-- works on the host). Keys are shown once at creation and stored only as a
-- SHA-256 hash — never in plaintext. Multiple keys per client allow rotation.

CREATE TABLE IF NOT EXISTS inbound_client_api_keys (
    key_id TEXT PRIMARY KEY,
    client_id TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,   -- SHA-256(presented key), hex
    key_prefix TEXT NOT NULL,        -- first chars (e.g. "mcpk_ab12") for UI display
    label TEXT,                      -- optional user-facing name for the key
    revoked INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT,
    expires_at TEXT,                 -- optional ISO-8601; NULL = no expiry
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (client_id) REFERENCES inbound_clients(client_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_api_keys_client ON inbound_client_api_keys(client_id);
-- Lookups on auth validate by hash and only care about live keys.
CREATE INDEX IF NOT EXISTS idx_api_keys_hash_live ON inbound_client_api_keys(key_hash) WHERE revoked = 0;
