-- Migration 012: Multi-FS workspace bindings.
--
-- One workspace root can now route into N FeatureSets (composed at the
-- resolver into a single allow set). Up until now each binding owned a
-- single `feature_set_id` column on `workspace_bindings`; this migration
-- moves to a junction table and recreates `workspace_bindings` without
-- the legacy column.
--
-- Order:
--   1. Create the junction.
--   2. Backfill (binding_id, feature_set_id, sort_order=0) from each
--      current row.
--   3. Recreate `workspace_bindings` without the column (the recreate-and-
--      copy pattern keeps us compatible with older SQLite that doesn't
--      support `ALTER TABLE … DROP COLUMN`).

CREATE TABLE workspace_binding_feature_sets (
    binding_id      TEXT NOT NULL REFERENCES workspace_bindings(id) ON DELETE CASCADE,
    feature_set_id  TEXT NOT NULL REFERENCES feature_sets(id) ON DELETE CASCADE,
    -- Stable rendering order in the UI; resolver doesn't care about order
    -- but the operator may want "primary" to render first.
    sort_order      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (binding_id, feature_set_id)
);

CREATE INDEX IF NOT EXISTS idx_wbfs_binding
    ON workspace_binding_feature_sets(binding_id);

-- Backfill — every existing binding has exactly one FS.
INSERT INTO workspace_binding_feature_sets (binding_id, feature_set_id, sort_order)
SELECT id, feature_set_id, 0
FROM workspace_bindings;

-- Recreate `workspace_bindings` without the legacy column.
CREATE TABLE workspace_bindings_new (
    id              TEXT PRIMARY KEY,
    workspace_root  TEXT NOT NULL UNIQUE,
    space_id        TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

INSERT INTO workspace_bindings_new
    (id, workspace_root, space_id, created_at, updated_at)
SELECT
    id, workspace_root, space_id, created_at, updated_at
FROM workspace_bindings;

DROP TABLE workspace_bindings;
ALTER TABLE workspace_bindings_new RENAME TO workspace_bindings;

CREATE INDEX IF NOT EXISTS idx_workspace_bindings_root
    ON workspace_bindings(workspace_root);
CREATE INDEX IF NOT EXISTS idx_workspace_bindings_space
    ON workspace_bindings(space_id);
