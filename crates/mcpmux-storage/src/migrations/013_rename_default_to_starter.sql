-- Migration 013: Rename FeatureSetType `default` → `starter`.
--
-- The "Default" name dates back to when the resolver fell back to the
-- per-Space Default FS for any unbound session. Post-resolver-v3 nothing
-- routes there automatically — the type is just a flag for "this FS got
-- auto-seeded with the Space, you can edit/rename/delete it freely."
-- "Starter" matches that role honestly.
--
-- Idempotent: running on a fresh DB seeded with the new value is a no-op.
-- The stable id prefix `fs_default_<space>` is intentionally NOT renamed:
-- those ids are foreign keys in `workspace_binding_feature_sets` and
-- `client_grants`, and rewriting them would cascade for no operator-
-- visible benefit. The on-disk id stays for FK integrity; only the
-- type *label* changes.

UPDATE feature_sets
   SET feature_set_type = 'starter'
 WHERE feature_set_type = 'default';
