-- Optional friendly display name for workspace bindings (separate from workspace_root).
ALTER TABLE workspace_bindings ADD COLUMN label TEXT;
