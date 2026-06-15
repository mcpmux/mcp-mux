-- Migration 017: Purge orphaned feature_set_members.
--
-- The refactor (#151) changed a feature member's identity from a qualified
-- "server_id/tool_name" string to the `server_features.id` UUID, but no
-- migration converted the rows that migration 001's `default` (now "Starter")
-- set had accumulated under the old model. On upgraded installs those members
-- survived as dangling rows whose `member_id` resolves to no live feature, so:
--   * the FeatureSets card counted the raw rows (e.g. "93 members"), while
--   * the detail/resolver matched against live feature ids and found none
--     selected — the set effectively granted 0 tools.
--
-- Those orphaned members already resolve to nothing, so deleting them changes
-- no effective behavior — it just makes the count honest and leaves the
-- Starter set empty, identical to a freshly created Space (the new model gives
-- the Starter set no special routing role). Composition members that point at
-- a deleted set are cleaned up the same way.
--
-- Idempotent: re-running deletes nothing once the table is consistent. Any
-- valid members a user added under the new model are kept (their member_id
-- still resolves).

-- Feature members whose target feature no longer exists.
DELETE FROM feature_set_members
 WHERE member_type = 'feature'
   AND member_id NOT IN (SELECT id FROM server_features);

-- Composition members whose target FeatureSet no longer exists.
DELETE FROM feature_set_members
 WHERE member_type = 'feature_set'
   AND member_id NOT IN (SELECT id FROM feature_sets);
