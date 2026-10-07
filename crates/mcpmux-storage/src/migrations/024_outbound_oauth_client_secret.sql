-- Migration 024: store the client secret from outbound Dynamic Client Registration
--
-- An authorization server may answer McpMux's DCR request with a client_secret
-- (a confidential client). Every token request must then carry it, including
-- refreshes after an app restart. Until now only the client_id was stored, so
-- such servers failed the refresh with invalid_client once McpMux restarted.
-- AES-256-GCM encrypted like other credentials. NULL = public client (no secret).
ALTER TABLE outbound_oauth_clients ADD COLUMN client_secret_encrypted TEXT;
