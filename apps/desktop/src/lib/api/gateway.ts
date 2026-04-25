import { invoke } from '@tauri-apps/api/core';

/**
 * Gateway status.
 */
export interface GatewayStatus {
  running: boolean;
  url: string | null;
  active_sessions: number;
  connected_backends: number;
}

/**
 * Config export format.
 */
export type ExportFormat = 'cursor' | 'vscode' | 'claude';

/**
 * Get gateway status.
 */
export async function getGatewayStatus(spaceId?: string): Promise<GatewayStatus> {
  return invoke('get_gateway_status', { spaceId });
}

/**
 * Probe result for a proposed gateway start.
 *
 * `source` tells the UI which tier the preferred port came from so it can
 * phrase the prompt correctly ("your configured port" vs "the default port").
 */
export interface GatewayStartProbe {
  preferredPort: number;
  preferredAvailable: boolean;
  source: 'override' | 'configured' | 'default';
}

/**
 * Ask the backend whether the gateway can start on its preferred port.
 * Does not start anything — used by the UI to decide whether to prompt.
 */
export async function probeGatewayStart(port?: number): Promise<GatewayStartProbe> {
  return invoke('probe_gateway_start', { port });
}

/**
 * Auto-start port conflict raised during app launch. When non-null, the UI
 * must prompt the user before the gateway will bind.
 */
export interface PendingPortConflict {
  preferredPort: number;
  source: 'configured' | 'default';
}

/**
 * Atomically read AND clear the deferred auto-start port conflict.
 *
 * "Take" semantics — only the first caller gets the conflict; subsequent
 * calls return null. Prevents duplicate prompts under React StrictMode's
 * double-mount.
 */
export async function takePendingPortConflict(): Promise<PendingPortConflict | null> {
  return invoke('take_pending_port_conflict');
}

/**
 * Error marker the backend returns when the preferred port is busy and
 * `allowDynamicFallback` is false. Shape: `PORT_IN_USE:<port>:<source>`.
 */
export interface PortInUseError {
  kind: 'PortInUse';
  port: number;
  source: 'override' | 'configured' | 'default';
}

/** Parse the `PORT_IN_USE:<port>:<source>` sentinel the backend emits. */
export function parsePortInUseError(err: unknown): PortInUseError | null {
  const msg = err instanceof Error ? err.message : typeof err === 'string' ? err : '';
  const match = /^PORT_IN_USE:(\d+):(override|configured|default)$/.exec(msg);
  if (!match) return null;
  return {
    kind: 'PortInUse',
    port: Number(match[1]),
    source: match[2] as PortInUseError['source'],
  };
}

/**
 * Start the gateway server. Strict by default — pass `allowDynamicFallback`
 * to let the gateway pick a dynamic port when the preferred one is taken.
 */
export async function startGateway(opts?: {
  port?: number;
  allowDynamicFallback?: boolean;
}): Promise<string> {
  return invoke('start_gateway', {
    port: opts?.port,
    allowDynamicFallback: opts?.allowDynamicFallback,
  });
}

/**
 * Stop the gateway server.
 */
export async function stopGateway(): Promise<void> {
  return invoke('stop_gateway');
}

/**
 * Restart the gateway server. Same semantics as `startGateway`.
 */
export async function restartGateway(opts?: {
  port?: number;
  allowDynamicFallback?: boolean;
}): Promise<string> {
  return invoke('restart_gateway', {
    port: opts?.port,
    allowDynamicFallback: opts?.allowDynamicFallback,
  });
}

/**
 * Export config for a client.
 */
export async function exportConfig(
  format: ExportFormat,
  clientId?: string
): Promise<string> {
  return invoke('export_config', { format, clientId });
}

/**
 * Backend server status.
 */
export interface BackendStatus {
  id: string;
  name: string;
  status: string;
  tools_count: number;
}

/**
 * Connect an installed server to the gateway.
 */
export async function connectServer(serverId: string): Promise<void> {
  return invoke('connect_server', { serverId });
}

/**
 * Disconnect a server from the gateway.
 * @param serverId - The server ID to disconnect
 * @param spaceId - The space ID (required for proper space isolation)
 * @param logout - If true, also delete stored credentials (OAuth tokens)
 */
export async function disconnectServer(serverId: string, spaceId: string, logout?: boolean): Promise<void> {
  return invoke('disconnect_server', { serverId, spaceId, logout });
}

/**
 * List all connected backend servers.
 */
export async function listConnectedServers(): Promise<BackendStatus[]> {
  return invoke('list_connected_servers');
}

/**
 * Inbound client registration type (per MCP spec 2025-11-25)
 */
export type RegistrationType = 'cimd' | 'dcr' | 'preregistered';

/**
 * Inbound client (unified OAuth + MCP model)
 * 
 * Represents apps connecting TO McpMux (e.g., Cursor, VS Code, Claude Desktop).
 * Supports three MCP registration approaches:
 * - CIMD: Client ID Metadata Documents (client_id is a URL)
 * - DCR: Dynamic Client Registration (server generates client_id)
 * - Preregistered: Server pre-configures client_id
 * 
 * Per RFC 7591, clients self-identify via metadata they provide.
 * Use `logo_uri`, `software_id`, and `client_name` for client identification.
 */
export interface OAuthClient {
  client_id: string;
  registration_type: RegistrationType;
  client_name: string;
  client_alias: string | null;
  redirect_uris: string[];
  scope: string | null;
  
  // Approval status - true if user has explicitly approved this client
  approved: boolean;
  
  // RFC 7591 Client Metadata (use these for client identification)
  logo_uri?: string | null;  // URL for client's logo
  client_uri?: string | null;  // URL of client's homepage
  software_id?: string | null;  // Unique identifier (e.g., "com.cursor.app")
  software_version?: string | null;  // Client software version
  
  // CIMD-specific fields (only used when registration_type='cimd')
  metadata_url?: string | null;  // URL where metadata was fetched
  metadata_cached_at?: string | null;  // When we last fetched
  metadata_cache_ttl?: number | null;  // Cache duration in seconds

  last_seen: string | null;
  created_at: string;
}

/**
 * Update client settings request. Only the display alias is editable.
 */
export interface UpdateClientRequest {
  client_alias?: string;
}

/**
 * List all registered OAuth clients (Cursor, Claude, etc.)
 */
export async function listOAuthClients(): Promise<OAuthClient[]> {
  return invoke('get_oauth_clients');
}

/**
 * Update an OAuth client's settings.
 */
export async function updateOAuthClient(
  clientId: string,
  settings: UpdateClientRequest
): Promise<OAuthClient> {
  return invoke('update_oauth_client', { clientId, settings });
}

/**
 * Delete an OAuth client.
 */
export async function deleteOAuthClient(clientId: string): Promise<void> {
  return invoke('delete_oauth_client', { clientId });
}

/**
 * Result of bulk server connection.
 */
export interface BulkConnectResult {
  connected: number;
  reused: number;
  failed: number;
  oauth_required: number;
  errors: string[];
}

/**
 * Connect all enabled servers from all spaces.
 * This is typically called on gateway startup.
 */
export async function connectAllEnabledServers(): Promise<BulkConnectResult> {
  return invoke('connect_all_enabled_servers');
}

/**
 * Pool statistics.
 */
export interface PoolStats {
  total_instances: number;
  connected_instances: number;
  total_space_server_mappings: number;
}

/**
 * Get server pool statistics.
 */
export async function getPoolStats(): Promise<PoolStats> {
  return invoke('get_pool_stats');
}

/**
 * Result of OAuth token refresh operation.
 */
export interface RefreshResult {
  servers_checked: number;
  tokens_refreshed: number;
  refresh_failed: number;
}

/**
 * Refresh OAuth tokens on startup for all installed HTTP servers.
 * This should be called during app initialization before connecting to servers.
 */
export async function refreshOAuthTokensOnStartup(): Promise<RefreshResult> {
  return invoke('refresh_oauth_tokens_on_startup');
}

/**
 * Open a URL using the system's default handler.
 * 
 * This is needed for custom protocol URLs (like `cursor://`) that
 * the webview's opener plugin may not be allowed to open directly.
 */
export async function openUrl(url: string): Promise<void> {
  return invoke('open_url', { url });
}
