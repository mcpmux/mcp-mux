-- Migration 002: FeatureSet Resolver V2
--
-- Introduces the project-oriented FeatureSet selection model:
--   resolution order = access-key pin > workspace-root binding > space-active FS
--
-- This migration is forward-compatible: the old per-client grants system
-- (client_grants table + inbound_clients.grants JSON) keeps working. The
-- resolver is switched over in a later migration.
--
-- Added in this migration:
--   * inbound_clients.pinned_feature_set_id   — explicit FS for this access key
--   * inbound_clients.pinned_space_id          — Space the access key belongs to
--   * spaces.active_feature_set_id             — default FS per Space when no pin / no workspace match
--   * workspace_bindings                       — (space_id, workspace_root) -> feature_set_id overrides

-- ============================================================================
-- inbound_clients: pinned_feature_set_id + pinned_space_id
-- ============================================================================

-- The FS chosen at approval time. NULL means "follow workspace / space default".
ALTER TABLE inbound_clients ADD COLUMN pinned_feature_set_id TEXT
    REFERENCES feature_sets(id) ON DELETE SET NULL;

-- The Space this access key belongs to. Replaces locked_space_id semantically,
-- but the old column is kept for backwards compat until a later migration drops it.
ALTER TABLE inbound_clients ADD COLUMN pinned_space_id TEXT
    REFERENCES spaces(id) ON DELETE SET NULL;

-- Backfill pinned_space_id from locked_space_id for any existing rows.
UPDATE inbound_clients
SET pinned_space_id = locked_space_id
WHERE pinned_space_id IS NULL AND locked_space_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_inbound_clients_pinned_space ON inbound_clients(pinned_space_id);
CREATE INDEX IF NOT EXISTS idx_inbound_clients_pinned_fs ON inbound_clients(pinned_feature_set_id);

-- ============================================================================
-- spaces.active_feature_set_id
-- ============================================================================

ALTER TABLE spaces ADD COLUMN active_feature_set_id TEXT
    REFERENCES feature_sets(id) ON DELETE SET NULL;

-- Backfill every Space's active FS to its existing 'default' FeatureSet
-- so day-one behavior matches pre-migration: clients with no pin and no
-- workspace match receive the same features they had before.
UPDATE spaces
SET active_feature_set_id = (
    SELECT fs.id
    FROM feature_sets fs
    WHERE fs.space_id = spaces.id
      AND fs.feature_set_type = 'default'
      AND fs.is_deleted = 0
    LIMIT 1
)
WHERE active_feature_set_id IS NULL;

-- ============================================================================
-- workspace_bindings: (space_id, workspace_root) -> feature_set_id
-- ============================================================================
--
-- workspace_root is a normalized absolute filesystem path:
--   * Windows drive letter lowercased (e.g. "d:\projects\foo")
--   * trailing path separator stripped
--   * symlinks / junctions resolved before insert
-- Matching is longest-prefix over the caller's reported MCP roots.

CREATE TABLE IF NOT EXISTS workspace_bindings (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    workspace_root TEXT NOT NULL,
    feature_set_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    UNIQUE(space_id, workspace_root),
    FOREIGN KEY (space_id) REFERENCES spaces(id) ON DELETE CASCADE,
    FOREIGN KEY (feature_set_id) REFERENCES feature_sets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_workspace_bindings_space ON workspace_bindings(space_id);
CREATE INDEX IF NOT EXISTS idx_workspace_bindings_root ON workspace_bindings(workspace_root);
