-- Migration 008: Repair the default-space invariant.
--
-- Older code paths (and one bad branch in `SpaceAppService::create`) could
-- promote a user-created space to `is_default = 1` if it happened to be
-- created when no spaces existed. Combined with bare DB edits during early
-- testing, this left some installs with the wrong space marked default
-- (or worse, multiple defaults). The resolver picks "the" default space via
-- a query that returns whichever row SQLite hands back first, so symptoms
-- vary across machines.
--
-- This migration enforces the invariant: the seeded "My Space" row
-- (id `00000000-0000-0000-0000-000000000001`) is the canonical default,
-- and no other row carries the flag. It's idempotent — running it on a
-- healthy DB is a no-op.

-- Make sure the canonical row exists. The seed in migration 001 uses
-- `INSERT OR IGNORE`, so a freshly-installed DB already has it. This
-- safety net covers DBs that lost the row through manual editing.
INSERT OR IGNORE INTO spaces
    (id, name, icon, description, is_default, sort_order, created_at, updated_at)
VALUES (
    '00000000-0000-0000-0000-000000000001',
    'My Space',
    '🏠',
    'Default workspace for your MCP servers',
    1,
    0,
    datetime('now'),
    datetime('now')
);

-- Sole-default invariant: clear the flag on every other row first, then
-- set it on the canonical row. Order matters — the inverse would briefly
-- leave the table with two defaults if the canonical row was already flagged.
UPDATE spaces
   SET is_default = 0
 WHERE id <> '00000000-0000-0000-0000-000000000001';

UPDATE spaces
   SET is_default = 1
 WHERE id = '00000000-0000-0000-0000-000000000001';
