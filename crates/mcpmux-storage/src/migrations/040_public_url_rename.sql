-- Migration 040: gateway.public_url -> gateway.public_base_url key rename.
--
-- Two branches independently introduced this setting under different key
-- names on 2026-05-27; current code only reads gateway.public_base_url, so
-- installs that still carry the old key silently lose their public URL
-- (allowed_hosts drops the tunnel hostname -> remote requests 403).

INSERT OR IGNORE INTO app_settings (key, value, updated_at)
SELECT 'gateway.public_base_url', value, datetime('now')
  FROM app_settings
 WHERE key = 'gateway.public_url';

DELETE FROM app_settings WHERE key = 'gateway.public_url';
