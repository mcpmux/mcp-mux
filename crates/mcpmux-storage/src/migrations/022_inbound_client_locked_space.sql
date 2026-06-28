-- Migration 022: lock an inbound client to a single Space
--
-- An API-key (or any pre-registered) client may be confined to one Space. When
-- set, the FeatureSet resolver always resolves this client to `locked_space_id`,
-- ignoring an X-Mcpmux-Workspace header that points at a *different* Space (the
-- header may still select a FeatureSet *within* the locked Space). NULL =
-- unlocked (the default) — the client routes freely by header / clientId
-- mapping / default Space.
ALTER TABLE inbound_clients ADD COLUMN locked_space_id TEXT;
