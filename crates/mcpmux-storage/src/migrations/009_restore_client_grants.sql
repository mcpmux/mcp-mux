-- Migration 009: Restore client_grants for rootless clients.
--
-- Migration 003 dropped this table when the FeatureSetResolver was made
-- authoritative — but the resolver only handles roots-capable clients.
-- Clients that don't declare the MCP roots capability (Claude.ai web,
-- ChatGPT, …) need a per-OAuth-client default FeatureSet, which is what
-- this table stores.
--
-- Resolution order (resolver v3):
--   1. Session has roots + WorkspaceBinding matches → binding.fs
--   2. Session has roots + no binding              → deny + emit prompt
--   3. Session has no roots, client roots-capable  → empty (waiting on roots)
--   4. Session has no roots, client rootless       → client_grants for (client, space)
--   5. Otherwise                                   → deny
--
-- Schema mirrors migration 001's pre-003 definition.

CREATE TABLE IF NOT EXISTS client_grants (
    client_id TEXT NOT NULL,        -- References inbound_clients.client_id
    space_id TEXT NOT NULL,
    feature_set_id TEXT NOT NULL,
    PRIMARY KEY (client_id, space_id, feature_set_id),
    FOREIGN KEY (client_id) REFERENCES inbound_clients(client_id) ON DELETE CASCADE,
    FOREIGN KEY (space_id) REFERENCES spaces(id) ON DELETE CASCADE,
    FOREIGN KEY (feature_set_id) REFERENCES feature_sets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_client_grants_client ON client_grants(client_id);
CREATE INDEX IF NOT EXISTS idx_client_grants_space ON client_grants(space_id);
