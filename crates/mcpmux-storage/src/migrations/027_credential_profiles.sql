-- Migration 027: Central credential profiles / secrets store
--
-- Decouples server credentials and secrets from individual space installations,
-- allowing credentials to be managed centrally, reused across spaces,
-- preserved across uninstalls, and rotated in a single place.

CREATE TABLE IF NOT EXISTS credential_profiles (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    server_id        TEXT NOT NULL,
    server_name      TEXT,
    values_encrypted TEXT NOT NULL DEFAULT '{}',
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_credential_profiles_server_id
    ON credential_profiles(server_id);

ALTER TABLE installed_servers ADD COLUMN credential_profile_id TEXT
    REFERENCES credential_profiles(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_installed_servers_credential_profile
    ON installed_servers(credential_profile_id);
