import { invoke } from '@tauri-apps/api/core';

/**
 * Built-in MCP servers that McpMux ships (e.g. "Tool Optimization", the
 * `mcpmux_*` tools). Enablement + per-tool toggles are scoped **per Space**.
 */
export interface BuiltinTool {
  name: string;
  description: string;
  /** Mutating tool — gated behind a native approval dialog at call time. */
  write: boolean;
  enabled: boolean;
}

export interface BuiltinServer {
  id: string;
  name: string;
  description: string;
  /** Whether this server is enabled for the queried Space. */
  enabled: boolean;
  tools: BuiltinTool[];
}

/** List built-in servers with their enable state + per-tool toggles for a Space. */
export async function listBuiltinServers(spaceId: string): Promise<BuiltinServer[]> {
  return invoke('list_builtin_servers', { spaceId });
}

/** Enable/disable a built-in server for a Space. */
export async function setBuiltinServerEnabled(
  spaceId: string,
  serverId: string,
  enabled: boolean
): Promise<void> {
  return invoke('set_builtin_server_enabled', { spaceId, serverId, enabled });
}

/** Enable/disable a single tool of a built-in server for a Space. */
export async function setBuiltinToolEnabled(
  spaceId: string,
  serverId: string,
  toolName: string,
  enabled: boolean
): Promise<void> {
  return invoke('set_builtin_tool_enabled', { spaceId, serverId, toolName, enabled });
}
