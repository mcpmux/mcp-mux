-- Add source column to installed_servers table
-- Tracks how the server was installed: registry, user_config, or manual_entry
--
-- Values:
-- - 'registry': Installed from the server registry (default)
-- - 'user_config:/path/to/file.json': From a user config file
-- - 'manual_entry': Manually added via UI

ALTER TABLE installed_servers ADD COLUMN source TEXT NOT NULL DEFAULT 'registry';

-- Index for querying servers by source (useful for listing all servers from a config file)
CREATE INDEX IF NOT EXISTS idx_installed_servers_source ON installed_servers(source);
