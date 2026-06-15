-- Migration 010: Track whether each OAuth client has been seen reporting
-- the MCP `roots` capability.
--
-- The flag is stamped once per session by the gateway handler during
-- `on_initialized`. The Clients UI uses it to show a "Reports workspace"
-- vs "Rootless" badge, which in turn tells the user whether the per-client
-- grant editor on that client matters (it only does for rootless clients).
--
-- Default = 0 (unknown / not seen). The flag is monotonic — once a client
-- is observed reporting roots we keep the bit set, even if a later session
-- doesn't (ChatGPT-style connectors may flip per session). Users who want
-- to reset the bit can revoke + re-approve the client.

ALTER TABLE inbound_clients
    ADD COLUMN reports_roots INTEGER NOT NULL DEFAULT 0;
