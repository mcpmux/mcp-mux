-- User-editable emoji override for a client's Connections-page icon.
-- NULL means "fall back to logo_uri / known-client-name resolution".

ALTER TABLE inbound_clients ADD COLUMN client_icon TEXT;
