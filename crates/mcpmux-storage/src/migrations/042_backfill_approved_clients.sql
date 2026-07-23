-- Migration 042: backfill stale approved flags on OAuth clients with real traffic.
--
-- last_seen is only set after a successful authenticated request, which requires
-- an already-issued token. Clients with last_seen set were already granted
-- access; the approved flag just never caught up on legacy auto-approve paths.

UPDATE inbound_clients
SET approved = 1
WHERE approved = 0
  AND last_seen IS NOT NULL;
