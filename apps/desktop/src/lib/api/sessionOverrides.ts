import { invoke } from '@tauri-apps/api/core';

/** Session-scoped server enable/disable overrides from meta tools. */
export interface SessionOverride {
  session_id: string;
  enabled: string[];
  disabled: string[];
  /** Reported MCP workspace roots for this session (may be empty). */
  roots: string[];
}

/** List override state for all sessions, or one session when `sessionId` is set. */
export async function listSessionOverrides(
  sessionId?: string
): Promise<SessionOverride[]> {
  return invoke('list_session_overrides', { sessionId: sessionId ?? null });
}

/** Drop all overrides for a session and refresh its tool list. */
export async function clearSessionOverrides(sessionId: string): Promise<void> {
  return invoke('clear_session_overrides', { sessionId });
}

/** Whether session-scope enable/disable meta tools require approval. Default false. */
export async function getSessionOverridesRequireApproval(): Promise<boolean> {
  return invoke('get_session_overrides_require_approval');
}

/** Persist the session-override approval gate. */
export async function setSessionOverridesRequireApproval(
  requireApproval: boolean
): Promise<void> {
  return invoke('set_session_overrides_require_approval', { requireApproval });
}

/**
 * True when a session's reported root relates to the workspace path shown
 * in the inspector (exact match or parent/child prefix).
 */
export function sessionRootMatchesWorkspace(
  sessionRoot: string,
  workspaceRoot: string
): boolean {
  if (sessionRoot === workspaceRoot) return true;
  const sep = sessionRoot.includes('\\') ? '\\' : '/';
  return (
    workspaceRoot.startsWith(`${sessionRoot}${sep}`) ||
    sessionRoot.startsWith(`${workspaceRoot}${sep}`)
  );
}

/** Filter overrides to sessions reporting this workspace root. */
export function overridesForWorkspace(
  overrides: SessionOverride[],
  workspaceRoot: string
): SessionOverride[] {
  return overrides.filter((entry) =>
    entry.roots.some((root) => sessionRootMatchesWorkspace(root, workspaceRoot))
  );
}
