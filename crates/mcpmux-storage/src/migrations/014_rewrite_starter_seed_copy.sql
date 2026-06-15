-- Migration 014: Rewrite the auto-seeded Starter FS's display copy.
--
-- Migration 001 hard-coded `name = 'Default'` and
-- `description = 'The fallback feature set for this space'` on every
-- auto-seeded FS row. Both lie under the new resolver — nothing routes
-- to this FS automatically anymore. Migration 013 fixed the *type* but
-- couldn't re-run on DBs that had already recorded it as applied, so
-- the human-readable copy stayed wrong on those installs. This migration
-- rewrites the copy.
--
-- Safety: only updates rows that *still* match the exact seeded values.
-- An operator who renamed their auto-FS to anything else keeps their
-- custom name + description untouched. The `is_builtin = 1` filter
-- prevents collisions with a user-created FS that happens to be named
-- "Default".

UPDATE feature_sets
   SET name = 'Starter',
       description = 'Auto-created with this Space. Edit, rename, or delete freely — bindings and per-client grants pick FeatureSets explicitly, so this one has no special routing role.'
 WHERE is_builtin = 1
   AND name = 'Default'
   AND description = 'The fallback feature set for this space';
