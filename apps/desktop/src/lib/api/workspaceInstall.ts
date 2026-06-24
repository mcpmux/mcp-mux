import { invoke } from '@tauri-apps/api/core';

/** A client the per-workspace installer can write a config for. */
export interface WorkspaceInstallClient {
  id: string;
  label: string;
  /** Project-local config path, relative to the workspace folder. */
  config_path: string;
}

/** Result of writing one client's config. */
export interface WorkspaceInstallResult {
  client: string;
  label: string;
  path: string;
  /** "created" | "updated" | "error". */
  action: string;
  backed_up: string | null;
  error: string | null;
}

/** A copy-paste config snippet for one client. */
export interface WorkspaceConfigSnippet {
  client: string;
  label: string;
  config_path: string;
  /** Full file content (top-level key + the McpMux entry). */
  content: string;
}

/** The project-local clients the installer supports (Cursor, VS Code, …). */
export async function listWorkspaceInstallClients(): Promise<WorkspaceInstallClient[]> {
  return invoke('list_workspace_install_clients');
}

/** Generate a copy-paste config snippet for one client (writes nothing). */
export async function generateWorkspaceConfigSnippet(args: {
  client: string;
  serverUrl: string;
  workspaceRoot: string;
  bearer?: string | null;
}): Promise<WorkspaceConfigSnippet> {
  return invoke('generate_workspace_config_snippet', {
    client: args.client,
    serverUrl: args.serverUrl,
    workspaceRoot: args.workspaceRoot,
    bearer: args.bearer ?? null,
  });
}

/** Create or extend the selected clients' configs inside `workspaceRoot`. */
export async function installWorkspaceMcpConfig(args: {
  workspaceRoot: string;
  serverUrl: string;
  clients: string[];
  bearer?: string | null;
}): Promise<WorkspaceInstallResult[]> {
  return invoke('install_workspace_mcp_config', {
    workspaceRoot: args.workspaceRoot,
    serverUrl: args.serverUrl,
    clients: args.clients,
    bearer: args.bearer ?? null,
  });
}

/** Whether system-wide inbound auth is disabled (no access key required). */
export async function getGatewayAuthDisabled(): Promise<boolean> {
  return invoke('get_gateway_auth_disabled');
}

/** Enable/disable system-wide inbound auth. Takes effect immediately. */
export async function setGatewayAuthDisabled(disabled: boolean): Promise<boolean> {
  return invoke('set_gateway_auth_disabled', { disabled });
}
