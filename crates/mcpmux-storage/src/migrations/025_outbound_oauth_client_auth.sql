-- Migration 025: how an outbound DCR client authenticates, and when its secret expires
--
-- An RFC 7591 registration response carries token_endpoint_auth_method, the method
-- the server registered the client with (e.g. client_secret_post), and, whenever a
-- secret is issued, client_secret_expires_at. McpMux must send the secret the way
-- the server registered it, and must not reuse a client whose secret has expired.
-- NULL = the server didn't say; for the expiry also: the secret never expires.
-- client_secret_expires_at is RFC 3339 text, like created_at.
ALTER TABLE outbound_oauth_clients ADD COLUMN token_endpoint_auth_method TEXT;
ALTER TABLE outbound_oauth_clients ADD COLUMN client_secret_expires_at TEXT;
