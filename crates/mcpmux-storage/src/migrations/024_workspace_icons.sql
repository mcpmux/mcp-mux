-- Workspace icon metadata for mapped and unmapped workspace roots.
ALTER TABLE workspace_bindings ADD COLUMN icon TEXT;

CREATE TABLE IF NOT EXISTS workspace_appearances (
    workspace_root TEXT PRIMARY KEY,
    icon TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
