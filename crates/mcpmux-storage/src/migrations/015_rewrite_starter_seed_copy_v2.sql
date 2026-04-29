-- Migration 015: catch the *other* legacy seed copy that 014 missed.
--
-- Migration 001 (still shipped, can't edit retroactively) seeds the
-- default Space's auto-Starter row with:
--     name = 'Default'
--     description = 'Features automatically granted to all connected clients in this space'
--
-- Migration 014 only rewrote rows whose description was the OTHER stale
-- variant ('The fallback feature set for this space'), set by
-- space_repository.rs::create() at one point in history. So the
-- migration-001-seeded row on every existing install survived 014
-- unchanged, and the Clients UI still shows "Features automatically
-- granted to all connected clients in this space" — which is the most
-- misleading of the lot under resolver v3 (literally the opposite of
-- the truth).
--
-- Same safety guard as 014: only rewrite rows that still match the
-- exact stale seed values, so any operator who customized the copy
-- keeps their change.

UPDATE feature_sets
   SET name = 'Starter',
       description = 'Auto-created with this Space. Edit, rename, or delete freely — bindings and per-client grants pick FeatureSets explicitly, so this one has no special routing role.'
 WHERE is_builtin = 1
   AND name = 'Default'
   AND description = 'Features automatically granted to all connected clients in this space';
