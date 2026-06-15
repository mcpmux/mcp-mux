-- Migration 006: Collapse FeatureSet model to Default + Custom only
--
-- Previously each space auto-spawned two builtin FSes (`all` + `default`) and
-- every installed server got a `server-all` set; clients could also grow
-- auto-created "{client_name} - Custom" sets on first use. The resolver no
-- longer consults those — routing is pure `WorkspaceBinding → Space default`.
--
-- This migration:
--   1. Hard-deletes the legacy auto-created rows so they disappear from the UI.
--   2. Rebuilds `inbound_clients` without the `connection_mode` /
--      `locked_space_id` columns (they belonged to the old client-level
--      routing surface and are now dead weight).
--
-- Custom sets the user authored by hand stay; they may still be referenced
-- from `workspace_bindings.locked_feature_set_id` or `spaces.active_feature_set_id`.

PRAGMA foreign_keys = OFF;

-- Clear any `active_feature_set_id` pointing at an `all` / `server-all` set
-- before we delete those rows, otherwise the FK would dangle.
UPDATE spaces
SET active_feature_set_id = NULL
WHERE active_feature_set_id IN (
    SELECT id FROM feature_sets
    WHERE feature_set_type IN ('all', 'server-all')
);

-- Same cleanup for workspace_bindings.locked_feature_set_id — if a binding
-- pinned an 'all'/'server-all' FS, collapse to ActiveForSpace so routing
-- falls through to the space's Default FS instead of dangling.
UPDATE workspace_bindings
SET fs_mode = 'active_for_space', locked_feature_set_id = NULL
WHERE locked_feature_set_id IN (
    SELECT id FROM feature_sets
    WHERE feature_set_type IN ('all', 'server-all')
);

-- Delete legacy auto-created feature sets. We also nuke the per-client
-- "{client_name} - Custom" rows that find_or_create_client_custom_feature_set
-- seeded (they are conventionally named — exact pattern match).
DELETE FROM feature_set_members
WHERE feature_set_id IN (
    SELECT id FROM feature_sets WHERE feature_set_type IN ('all', 'server-all')
);

DELETE FROM feature_sets
WHERE feature_set_type IN ('all', 'server-all')
   OR (feature_set_type = 'custom' AND name LIKE '% - Custom');

-- Rebuild inbound_clients WITHOUT connection_mode + locked_space_id.
-- Mirror the 005 schema; just drop the two dead columns and the FK they had.
CREATE TABLE inbound_clients_new (
    client_id TEXT PRIMARY KEY,

    registration_type TEXT NOT NULL CHECK(registration_type IN ('cimd', 'dcr', 'preregistered')),

    client_name TEXT NOT NULL,
    client_alias TEXT,

    logo_uri TEXT,
    client_uri TEXT,
    software_id TEXT,
    software_version TEXT,

    redirect_uris TEXT NOT NULL,
    grant_types TEXT NOT NULL,
    response_types TEXT NOT NULL,
    token_endpoint_auth_method TEXT NOT NULL,
    scope TEXT,

    metadata_url TEXT,
    metadata_cached_at TEXT,
    metadata_cache_ttl INTEGER DEFAULT 3600,

    grants TEXT,

    approved INTEGER NOT NULL DEFAULT 0,

    last_seen TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT INTO inbound_clients_new (
    client_id,
    registration_type,
    client_name, client_alias,
    logo_uri, client_uri, software_id, software_version,
    redirect_uris, grant_types, response_types, token_endpoint_auth_method, scope,
    metadata_url, metadata_cached_at, metadata_cache_ttl,
    grants,
    approved,
    last_seen, created_at, updated_at
)
SELECT
    client_id,
    registration_type,
    client_name, client_alias,
    logo_uri, client_uri, software_id, software_version,
    redirect_uris, grant_types, response_types, token_endpoint_auth_method, scope,
    metadata_url, metadata_cached_at, metadata_cache_ttl,
    grants,
    approved,
    last_seen, created_at, updated_at
FROM inbound_clients;

DROP TABLE inbound_clients;
ALTER TABLE inbound_clients_new RENAME TO inbound_clients;

CREATE INDEX IF NOT EXISTS idx_inbound_clients_type
    ON inbound_clients(registration_type);
CREATE INDEX IF NOT EXISTS idx_inbound_clients_name
    ON inbound_clients(client_name);
CREATE INDEX IF NOT EXISTS idx_inbound_clients_metadata_url
    ON inbound_clients(metadata_url) WHERE metadata_url IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_inbound_clients_approved
    ON inbound_clients(approved) WHERE approved = 1;

PRAGMA foreign_keys = ON;
