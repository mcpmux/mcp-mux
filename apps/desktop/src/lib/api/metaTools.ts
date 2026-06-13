import { invoke } from '@tauri-apps/api/core';

/** An "always allow from (client, tool)" entry kept in the gateway's broker. */
export interface MetaToolGrantEntry {
  client_id: string;
  tool_name: string;
}

/** Audit row emitted on every `mcpmux_*` invocation. */
export interface MetaToolAuditEvent {
  client_id: string;
  session_id: string | null;
  tool_name: string;
  /** "allow_once" | "always_for_this_session_and_client" | "deny" | "timeout" | "approval_required" | "rate_limited" | "invalid_args" | "read" | "error" */
  decision: string;
  resolved_feature_set_id: string | null;
  summary: string;
  /** Populated by the Tauri bridge. */
  timestamp: string;
}

/** List every session-scoped "always allow" entry in the gateway. */
export async function listMetaToolGrants(): Promise<MetaToolGrantEntry[]> {
  return invoke('list_meta_tool_grants');
}

/** Revoke a single "always allow" entry. */
export async function revokeMetaToolGrant(clientId: string, toolName: string): Promise<boolean> {
  return invoke('revoke_meta_tool_grant', { clientId, toolName });
}

// The mcpmux_* enablement switch is now per-Space — see
// `@/lib/api/builtinServers` (listBuiltinServers / setBuiltinServerEnabled /
// setBuiltinToolEnabled). The old global get/set_meta_tools_enabled were removed.

/**
 * DEBUG/dev only: toggle auto-approval of every write meta tool.
 *
 * When on, `mcpmux_manage_feature_set` / `mcpmux_bind_current_workspace` and
 * friends are approved without a dialog — so a developer can self-create
 * feature sets and bindings and exercise routing end-to-end. Session-only:
 * resets on gateway restart (to the `MCPMUX_DEBUG_AUTO_APPROVE` env default).
 */
export async function setMetaToolsAutoApprove(enabled: boolean): Promise<boolean> {
  return invoke('set_meta_tools_auto_approve', { enabled });
}

/** Whether write meta tools are currently auto-approved (DEBUG mode state). */
export async function getMetaToolsAutoApprove(): Promise<boolean> {
  return invoke('get_meta_tools_auto_approve');
}

/**
 * Respond to a pending approval request. Normally called by
 * `<MetaToolApprovalDialog>`; exported here for tests and advanced flows.
 */
export async function respondToMetaToolApproval(
  requestId: string,
  clientId: string,
  toolName: string,
  decision: 'allow_once' | 'always_for_this_session_and_client' | 'deny'
): Promise<boolean> {
  return invoke('respond_to_meta_tool_approval', {
    requestId,
    clientId,
    toolName,
    decision,
  });
}
