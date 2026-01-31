-- App Settings Table
-- 
-- Key-value store for application-wide settings.
-- Replaces scattered config files (gateway-port.txt, etc.) with a unified store.
--
-- Examples:
-- - gateway.port = "45818"
-- - gateway.auto_start = "true"
-- - ui.theme = "dark"
-- - ui.window_state = '{"x":100,"y":100,"width":800,"height":600}'

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Seed default settings
INSERT OR IGNORE INTO app_settings (key, value, updated_at)
VALUES 
    ('gateway.auto_start', 'true', datetime('now'));
