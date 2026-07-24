-- Migration 038: Optional locked_space_id on inbound_clients.
--
-- Confines an API-key (or other inbound) client to one Space. The resolver
-- treats this as a narrowing filter — bindings/grants outside the locked
-- Space are ignored; no in-Space match still resolves to Unbound.

ALTER TABLE inbound_clients ADD COLUMN locked_space_id TEXT REFERENCES spaces(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_inbound_clients_locked_space_id ON inbound_clients(locked_space_id);
