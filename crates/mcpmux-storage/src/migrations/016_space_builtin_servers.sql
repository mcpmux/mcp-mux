-- Migration 016: per-Space built-in server config.
--
-- Built-in servers (e.g. "Tool Optimization" / the mcpmux_* tools) and their
-- individual tools are enabled/disabled PER SPACE. Only deviations from the
-- default are stored — a missing row means "use the descriptor default"
-- (server on, tool on). This replaces the old GLOBAL
-- `gateway.meta_tools_enabled` app-setting switch.

CREATE TABLE IF NOT EXISTS space_builtin_servers (
    space_id   TEXT NOT NULL,
    server_id  TEXT NOT NULL,
    enabled    INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (space_id, server_id)
);

CREATE TABLE IF NOT EXISTS space_builtin_tools (
    space_id   TEXT NOT NULL,
    server_id  TEXT NOT NULL,
    tool_name  TEXT NOT NULL,
    enabled    INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (space_id, server_id, tool_name)
);

-- Preserve an existing GLOBAL "off" preference: if the operator had the
-- meta-tools master switch disabled, seed a disabled Tool Optimization row for
-- every current Space so behaviour doesn't silently flip back on. When the
-- switch was on/absent we seed nothing (the default is already "on").
INSERT OR IGNORE INTO space_builtin_servers (space_id, server_id, enabled)
SELECT s.id, 'tool-optimization', 0
  FROM spaces s
 WHERE (SELECT value FROM app_settings WHERE key = 'gateway.meta_tools_enabled')
       IN ('false', '0');

-- The global switch is superseded by the per-Space config above.
DELETE FROM app_settings WHERE key = 'gateway.meta_tools_enabled';
