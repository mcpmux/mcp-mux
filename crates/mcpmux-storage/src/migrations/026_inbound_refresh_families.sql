-- Migration 026: rotating refresh tokens for inbound OAuth clients
--
-- Each sign-in starts a refresh-token family. Every refresh replaces the
-- family's current token id; presenting an older one (outside a short
-- retry window for the previous token) revokes the whole family. Families
-- go away with their client.
CREATE TABLE IF NOT EXISTS inbound_refresh_families (
    family_id    TEXT PRIMARY KEY,
    client_id    TEXT NOT NULL REFERENCES inbound_clients(client_id) ON DELETE CASCADE,
    current_jti  TEXT NOT NULL,
    previous_jti TEXT,
    rotated_at   TEXT,
    revoked      INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_inbound_refresh_families_client
    ON inbound_refresh_families(client_id);

-- Refresh tokens issued before families existed are honoured once; their
-- hashes are recorded here so a copy can't be used again.
CREATE TABLE IF NOT EXISTS inbound_spent_legacy_refresh (
    token_hash TEXT PRIMARY KEY,
    spent_at   TEXT NOT NULL
);
