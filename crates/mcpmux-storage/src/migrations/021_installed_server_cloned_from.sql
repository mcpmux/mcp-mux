-- Track clone lineage on installed servers (display-only in v1).
ALTER TABLE installed_servers ADD COLUMN cloned_from TEXT;
