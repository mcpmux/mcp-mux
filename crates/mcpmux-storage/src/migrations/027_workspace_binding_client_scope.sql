-- Migration 027: Optional client_id scope on workspace_bindings.
--
-- Global bindings (client_id IS NULL) remain the default — one per path,
-- shared by any client without a scoped override. Scoped bindings
-- (client_id set) let distinct OAuth clients route the same filesystem
-- path to different FeatureSets.
--
-- Partial unique indexes replace the old global UNIQUE(workspace_root).

CREATE TABLE workspace_binding_feature_sets_backup AS
SELECT * FROM workspace_binding_feature_sets;

DROP TABLE workspace_binding_feature_sets;

CREATE TABLE workspace_bindings_v3 (
    id              TEXT PRIMARY KEY,
    workspace_root  TEXT NOT NULL,
    client_id       TEXT REFERENCES inbound_clients(client_id) ON DELETE SET NULL,
    label           TEXT,
    icon            TEXT,
    space_id        TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

INSERT INTO workspace_bindings_v3
    (id, workspace_root, client_id, label, icon, space_id, created_at, updated_at)
SELECT id, workspace_root, NULL, label, icon, space_id, created_at, updated_at
FROM workspace_bindings;

DROP TABLE workspace_bindings;
ALTER TABLE workspace_bindings_v3 RENAME TO workspace_bindings;

CREATE TABLE workspace_binding_feature_sets (
    binding_id      TEXT NOT NULL REFERENCES workspace_bindings(id) ON DELETE CASCADE,
    feature_set_id  TEXT NOT NULL REFERENCES feature_sets(id) ON DELETE CASCADE,
    sort_order      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (binding_id, feature_set_id)
);

INSERT INTO workspace_binding_feature_sets
SELECT binding_id, feature_set_id, sort_order
FROM workspace_binding_feature_sets_backup;

DROP TABLE workspace_binding_feature_sets_backup;

CREATE INDEX IF NOT EXISTS idx_wbfs_binding
    ON workspace_binding_feature_sets(binding_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_root_global
    ON workspace_bindings(workspace_root)
    WHERE client_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_root_scoped
    ON workspace_bindings(client_id, workspace_root)
    WHERE client_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_workspace_bindings_root
    ON workspace_bindings(workspace_root);

CREATE INDEX IF NOT EXISTS idx_workspace_bindings_space
    ON workspace_bindings(space_id);

CREATE INDEX IF NOT EXISTS idx_workspace_bindings_client
    ON workspace_bindings(client_id);
