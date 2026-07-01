-- Migration 035: Optional machine_id on inbound_clients (OAuth MCP peers).
--
-- Lets each connected client be assigned to a machine catalog entry so the
-- resolver can route the same workspace path to different bindings per machine.

ALTER TABLE inbound_clients ADD COLUMN machine_id TEXT REFERENCES machines(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_inbound_clients_machine_id ON inbound_clients(machine_id);
