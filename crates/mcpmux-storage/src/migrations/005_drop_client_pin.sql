-- Migration 005: Drop client-level FeatureSet pinning
--
-- The per-client pin was an escape hatch for "lock this access key to FS X".
-- In the new model, routing is keyed on the workspace root (WorkspaceBinding)
-- not the client identity — two IDEs opening the same folder should see the
-- same tools regardless of which one they are. Sessions without a root fall
-- through to the Space's active FS (same behaviour as today's default).
--
-- SQLite requires table-rebuild semantics for DROP COLUMN when the column has
-- a FOREIGN KEY reference. PRAGMA foreign_keys is toggled off during the copy
-- so the FK constraint doesn't block the rebuild; it's restored at the end.

PRAGMA foreign_keys = OFF;

-- Mirror the original `inbound_clients` schema (migration 001) MINUS the two
-- pinned_* columns added in migration 002. Keep every other column so the
-- copy preserves all user data (OAuth metadata, approval flag, aliases, …).
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

    connection_mode TEXT NOT NULL DEFAULT 'follow_active',
    locked_space_id TEXT,

    grants TEXT,

    approved INTEGER NOT NULL DEFAULT 0,

    last_seen TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    FOREIGN KEY (locked_space_id) REFERENCES spaces(id) ON DELETE SET NULL
);

INSERT INTO inbound_clients_new (
    client_id,
    registration_type,
    client_name, client_alias,
    logo_uri, client_uri, software_id, software_version,
    redirect_uris, grant_types, response_types, token_endpoint_auth_method, scope,
    metadata_url, metadata_cached_at, metadata_cache_ttl,
    connection_mode, locked_space_id,
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
    connection_mode, locked_space_id,
    grants,
    approved,
    last_seen, created_at, updated_at
FROM inbound_clients;

DROP TABLE inbound_clients;
ALTER TABLE inbound_clients_new RENAME TO inbound_clients;

-- Recreate the indices that 001 defined (migration 001 used CREATE INDEX
-- IF NOT EXISTS so re-creating is safe when run on databases that already
-- dropped them alongside the table).
CREATE INDEX IF NOT EXISTS idx_inbound_clients_type
    ON inbound_clients(registration_type);
CREATE INDEX IF NOT EXISTS idx_inbound_clients_name
    ON inbound_clients(client_name);
CREATE INDEX IF NOT EXISTS idx_inbound_clients_metadata_url
    ON inbound_clients(metadata_url) WHERE metadata_url IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_inbound_clients_approved
    ON inbound_clients(approved) WHERE approved = 1;

PRAGMA foreign_keys = ON;
