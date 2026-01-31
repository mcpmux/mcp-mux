-- Migration 002: Add approved flag to inbound_clients
-- 
-- This tracks whether a client has been explicitly approved by the user.
-- DCR creates the client entry, but approval happens separately.
-- Silent re-authentication only works for approved clients.

ALTER TABLE inbound_clients ADD COLUMN approved INTEGER NOT NULL DEFAULT 0;

-- Index for quick approved client lookups
CREATE INDEX IF NOT EXISTS idx_inbound_clients_approved ON inbound_clients(approved) WHERE approved = 1;
