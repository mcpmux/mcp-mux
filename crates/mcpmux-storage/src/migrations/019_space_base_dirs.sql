-- Migration 019: per-Space base directories.
--
-- A Space can claim one or more base directories. Any workspace root reported
-- by a connected client that sits at or under a base dir is "scoped" to that
-- Space: an unmapped root falls back to that Space's Starter (not the global
-- default), and the meta-tools / mapping popup restrict to that Space.
--
-- `path` is the NORMALIZED workspace root (see `normalize_workspace_root`:
-- lower-cased drive letter on Windows, `\` separators, no trailing slash).
-- UNIQUE(path) enforces one owner per exact path — the same folder can't be a
-- base dir of two Spaces. Nesting across Spaces is allowed and resolved by
-- longest-prefix at lookup time. ON DELETE CASCADE drops a Space's base dirs
-- with it.

CREATE TABLE IF NOT EXISTS space_base_dirs (
    id         TEXT PRIMARY KEY,
    space_id   TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    path       TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_space_base_dirs_space ON space_base_dirs(space_id);
