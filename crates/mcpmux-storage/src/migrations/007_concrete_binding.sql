-- Migration 007: Collapse WorkspaceBinding + Space resolution to concrete pointers
--
-- Bindings used to carry a matrix of modes:
--   space_mode             = active | locked
--   locked_space_id        (set when locked)
--   fs_mode                = active_for_space | locked
--   locked_feature_set_id  (set when locked)
-- And Space carried `active_feature_set_id` so Active-mode bindings could
-- follow whichever FS the user had promoted.
--
-- That indirection didn't carry its weight — the only real use case is
-- "for root X, use space S + feature set F". This migration collapses
-- every binding to a concrete `(space_id, feature_set_id)` pair and drops
-- the Space's `active_feature_set_id` column. Bindings that can't be
-- concretely resolved (missing a locked target on either side) are dropped
-- — there's no sensible place to land them in the new model.
--
-- SQLite note: PRAGMA foreign_keys can only be toggled outside a transaction
-- (the migration runner wraps each file in one), so we can't use the
-- table-rebuild pattern for `spaces` — a bare DROP would cascade through
-- feature_sets' ON DELETE CASCADE. Instead we DROP COLUMN directly, which
-- SQLite 3.35+ supports natively and doesn't touch dependent rows.

-- ---------------------------------------------------------------------------
-- 1. Drop WorkspaceBindings that can't be concretely resolved, then rebuild
--    the table with just the concrete pointer columns.
-- ---------------------------------------------------------------------------

-- Bindings with ActiveForSpace or Active modes can't be promoted into
-- (space_id, feature_set_id) without guessing — drop them.
DELETE FROM workspace_bindings
WHERE
    space_mode <> 'locked'
    OR fs_mode <> 'locked'
    OR locked_space_id IS NULL
    OR locked_feature_set_id IS NULL;

-- Rebuild the table around the columns we actually keep. The delete above
-- means every remaining row has non-null locked_* columns; the copy below
-- promotes them to the new NOT NULL schema without relying on FK cascade.
CREATE TABLE workspace_bindings_new (
    id              TEXT PRIMARY KEY,
    workspace_root  TEXT NOT NULL UNIQUE,
    space_id        TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    feature_set_id  TEXT NOT NULL REFERENCES feature_sets(id) ON DELETE CASCADE,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

INSERT INTO workspace_bindings_new
    (id, workspace_root, space_id, feature_set_id, created_at, updated_at)
SELECT
    id,
    workspace_root,
    locked_space_id,
    locked_feature_set_id,
    created_at,
    updated_at
FROM workspace_bindings;

DROP TABLE workspace_bindings;
ALTER TABLE workspace_bindings_new RENAME TO workspace_bindings;

CREATE INDEX IF NOT EXISTS idx_workspace_bindings_space
    ON workspace_bindings(space_id);
CREATE INDEX IF NOT EXISTS idx_workspace_bindings_fs
    ON workspace_bindings(feature_set_id);

-- ---------------------------------------------------------------------------
-- 2. Drop spaces.active_feature_set_id directly via ALTER — no rebuild.
-- ---------------------------------------------------------------------------

ALTER TABLE spaces DROP COLUMN active_feature_set_id;
