-- Migration 034: Optional machine_id scope on workspace_bindings.
--
-- Global bindings (machine_id IS NULL) remain the default — one canonical row
-- per path when client_id is also unset. Machine-scoped bindings let the same
-- filesystem path map to different FeatureSets on different hosts.
--
-- Partial unique indexes mirror migration 027's client_id pattern.

ALTER TABLE workspace_bindings
    ADD COLUMN machine_id TEXT REFERENCES machines(id) ON DELETE SET NULL;

DROP INDEX IF EXISTS idx_wb_root_global;

CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_root_global
    ON workspace_bindings(workspace_root)
    WHERE machine_id IS NULL AND client_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_root_machine
    ON workspace_bindings(machine_id, workspace_root)
    WHERE machine_id IS NOT NULL AND client_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_workspace_bindings_machine
    ON workspace_bindings(machine_id);
