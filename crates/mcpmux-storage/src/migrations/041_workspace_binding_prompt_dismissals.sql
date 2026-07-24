-- Migration 041: persist WorkspaceNeedsBinding panel dismissals per client + root.
--
-- Closing the binding prompt without saving records (client_id, workspace_root)
-- so reconnects do not re-fire the popup. Cleared when a binding is saved for
-- that workspace_root so a later regression surfaces again.

CREATE TABLE IF NOT EXISTS workspace_binding_prompt_dismissals (
    client_id TEXT NOT NULL,
    workspace_root TEXT NOT NULL,
    dismissed_at TEXT NOT NULL,
    PRIMARY KEY (client_id, workspace_root)
);

CREATE INDEX IF NOT EXISTS idx_wbpd_workspace_root
    ON workspace_binding_prompt_dismissals (workspace_root);
