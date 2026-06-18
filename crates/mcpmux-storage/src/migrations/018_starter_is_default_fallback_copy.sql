-- Migration 018: the Starter FS is the default fallback again (and can't be
-- deleted).
--
-- After the "default FeatureSet for unmapped roots" change, an unmapped folder
-- (plus rootless / unknown sessions) falls back to the default Space's Starter
-- FS instead of being denied. That makes the Starter load-bearing: it's the
-- default toolset for anything not explicitly mapped, and it is no longer
-- deletable. Migrations 014/015 (and the seed paths) set a description that
-- now lies — "no special routing role; delete freely" — so rewrite it.
--
-- Safety: only rewrite rows that STILL match the exact 014/015 seed text, so
-- an operator who customized the copy keeps their change.

UPDATE feature_sets
   SET description = 'Auto-created with this Space. Unmapped folders fall back to this set — it''s the default toolset for anything you haven''t explicitly mapped. Edit or rename it to change what they get; it can''t be deleted.'
 WHERE is_builtin = 1
   AND feature_set_type IN ('starter', 'default')
   AND description = 'Auto-created with this Space. Edit, rename, or delete freely — bindings and per-client grants pick FeatureSets explicitly, so this one has no special routing role.';
