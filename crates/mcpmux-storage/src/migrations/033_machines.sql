-- Migration 033: Machine catalog for per-host workspace organization.
--
-- Each McpMux install registers as a Machine; workspace bindings may optionally
-- scope to a machine (see migration 034).

CREATE TABLE machines (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    icon        TEXT,
    hostname    TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
