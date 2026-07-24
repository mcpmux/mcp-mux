-- Backfill workspace_bindings.icon from workspace_appearances for mapped roots.
UPDATE workspace_bindings
SET icon = (
    SELECT icon FROM workspace_appearances wa
    WHERE wa.workspace_root = workspace_bindings.workspace_root
)
WHERE icon IS NULL
  AND EXISTS (
    SELECT 1 FROM workspace_appearances wa
    WHERE wa.workspace_root = workspace_bindings.workspace_root
  );

-- Mapped roots store icon on the binding; drop stale appearance rows.
DELETE FROM workspace_appearances
WHERE workspace_root IN (SELECT workspace_root FROM workspace_bindings);
