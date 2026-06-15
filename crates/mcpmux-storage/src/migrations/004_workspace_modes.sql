-- Migration 004: Workspace-root-driven routing.
--
-- Each WorkspaceBinding now has TWO resolution modes — one for the Space
-- axis, one for the FeatureSet axis — each either "active" (follow the
-- global default) or "locked" to a specific id. See
-- `mcpmux.space/diagrams/workppace-root-session/` for the plan.
--
-- Changes:
--   * Drop the `space_id` uniqueness from `workspace_bindings` — routing is
--     now keyed on the root alone, not on (space_id, root), and the binding
--     itself carries space info via `space_mode`.
--   * Add `space_mode` + `space_id`   (with space_id NULL when Active).
--   * Add `fs_mode`    + `fs_id`      (with fs_id NULL when ActiveForSpace).
--   * Backfill existing rows: a binding today has (space_id, feature_set_id)
--     both set, so migrate to space_mode='locked' + fs_mode='locked'. This
--     preserves exact existing behaviour.
--   * The old (space_id, feature_set_id) columns stay readable for one
--     release; new writes use the mode columns only. They're dropped in
--     migration 005 once the resolver is on the new path everywhere.
--
-- Lifetime: forward-compatible additive. Old code can still read
-- (space_id, feature_set_id) directly; new code reads via the mode pair.

-- 1. Add the mode columns with sane defaults for existing rows.
ALTER TABLE workspace_bindings ADD COLUMN space_mode TEXT NOT NULL DEFAULT 'active';
ALTER TABLE workspace_bindings ADD COLUMN fs_mode TEXT NOT NULL DEFAULT 'active_for_space';
-- New nullable "locked to" pointers. Use distinct column names so we can
-- keep the old `space_id` / `feature_set_id` columns for one release.
ALTER TABLE workspace_bindings ADD COLUMN locked_space_id TEXT
    REFERENCES spaces(id) ON DELETE SET NULL;
ALTER TABLE workspace_bindings ADD COLUMN locked_feature_set_id TEXT
    REFERENCES feature_sets(id) ON DELETE SET NULL;

-- 2. Backfill existing bindings. Today every row has both ids populated
-- (non-null) so the "locked+locked" mode preserves exact behaviour.
UPDATE workspace_bindings
SET
    space_mode = 'locked',
    locked_space_id = space_id,
    fs_mode = 'locked',
    locked_feature_set_id = feature_set_id
WHERE space_id IS NOT NULL AND feature_set_id IS NOT NULL;

-- 3. Globalize uniqueness. The old `UNIQUE(space_id, workspace_root)`
-- conflicts with the new model where a root resolves globally. SQLite
-- doesn't support ALTER TABLE … DROP CONSTRAINT, so the pragmatic approach
-- is a table rebuild. We keep the old columns around for read compat.
CREATE TABLE workspace_bindings_v2 (
    id TEXT PRIMARY KEY,
    workspace_root TEXT NOT NULL UNIQUE,
    space_mode TEXT NOT NULL DEFAULT 'active',
    locked_space_id TEXT REFERENCES spaces(id) ON DELETE SET NULL,
    fs_mode TEXT NOT NULL DEFAULT 'active_for_space',
    locked_feature_set_id TEXT REFERENCES feature_sets(id) ON DELETE SET NULL,
    -- Legacy columns preserved until migration 005 ships and everything is
    -- on the new mode columns. Unused by new code.
    legacy_space_id TEXT,
    legacy_feature_set_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT INTO workspace_bindings_v2 (
    id, workspace_root, space_mode, locked_space_id, fs_mode, locked_feature_set_id,
    legacy_space_id, legacy_feature_set_id, created_at, updated_at
)
SELECT
    id, workspace_root, space_mode, locked_space_id, fs_mode, locked_feature_set_id,
    space_id, feature_set_id, created_at, updated_at
FROM workspace_bindings;

DROP TABLE workspace_bindings;
ALTER TABLE workspace_bindings_v2 RENAME TO workspace_bindings;

CREATE INDEX IF NOT EXISTS idx_workspace_bindings_root ON workspace_bindings(workspace_root);
CREATE INDEX IF NOT EXISTS idx_workspace_bindings_locked_space ON workspace_bindings(locked_space_id);
CREATE INDEX IF NOT EXISTS idx_workspace_bindings_locked_fs ON workspace_bindings(locked_feature_set_id);
