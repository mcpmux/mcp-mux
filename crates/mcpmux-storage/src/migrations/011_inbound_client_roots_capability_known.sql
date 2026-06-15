-- Migration 011: Distinguish "we haven't seen this client initialize yet"
-- from "this client explicitly does NOT support MCP roots".
--
-- Migration 010 added `reports_roots` defaulting to 0. The Clients UI
-- treated the column as a 2-state — but a brand-new approved client that
-- has never opened a session looks identical to a known-rootless client.
-- This migration adds an explicit "known" flag so the UI can render three
-- states: unknown (no badge), reports-workspace, rootless.
--
-- `roots_capability_known` flips to 1 the first time the gateway processes
-- `notifications/initialized` for a session of this client. After that the
-- value is sticky. `reports_roots` remains sticky-positive: once we've
-- seen *any* session declare the capability, we treat the whole client as
-- roots-capable so a one-off rootless reconnect doesn't bounce the badge.

ALTER TABLE inbound_clients
    ADD COLUMN roots_capability_known INTEGER NOT NULL DEFAULT 0;

-- Backfill: any row that already has reports_roots = 1 must have been
-- observed at least once, so seed it as "known". Rows with reports_roots = 0
-- stay at "unknown" — they may legitimately be either case until we see
-- their next initialize.
UPDATE inbound_clients
   SET roots_capability_known = 1
 WHERE reports_roots = 1;
