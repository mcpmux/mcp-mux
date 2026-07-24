-- Migration 037: binding_type on workspace_bindings ('path' | 'id').
--
-- Existing rows are path-typed folder bindings. Id-type rows route a rootless
-- OAuth/API client by clientId (stored in workspace_root) instead of a folder
-- path. Partial unique indexes are rebuilt so path and id rows can coexist.

ALTER TABLE workspace_bindings
    ADD COLUMN binding_type TEXT NOT NULL DEFAULT 'path'
    CHECK (binding_type IN ('path', 'id'));

UPDATE workspace_bindings SET binding_type = 'path';

DROP INDEX IF EXISTS idx_wb_root_global;
DROP INDEX IF EXISTS idx_wb_root_machine;
DROP INDEX IF EXISTS idx_wb_root_scoped;

CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_root_global
    ON workspace_bindings(workspace_root)
    WHERE binding_type = 'path' AND machine_id IS NULL AND client_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_root_machine
    ON workspace_bindings(machine_id, workspace_root)
    WHERE binding_type = 'path' AND machine_id IS NOT NULL AND client_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_root_scoped
    ON workspace_bindings(client_id, workspace_root)
    WHERE binding_type = 'path' AND client_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_id_global
    ON workspace_bindings(workspace_root)
    WHERE binding_type = 'id' AND machine_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_id_machine
    ON workspace_bindings(machine_id, workspace_root)
    WHERE binding_type = 'id' AND machine_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_workspace_bindings_binding_type
    ON workspace_bindings(binding_type);
