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
export async function getGatewayStatus(): Promise<GatewayStatus> {
  return invoke('get_gateway_status');
}

/**
 * Start the gateway server.
 */
export async function startGateway(port?: number): Promise<string> {
  return invoke('start_gateway', { port });
}

/**
 * Stop the gateway server.
 */
export async function stopGateway(): Promise<void> {
  return invoke('stop_gateway');
}

/**
 * Restart the gateway server.
 */
export async function restartGateway(): Promise<string> {
  return invoke('restart_gateway');
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
 * Represents apps connecting TO MCP Mux (e.g., Cursor, VS Code, Claude Desktop).
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
  
  // MCP client preferences
  connection_mode: string;
  locked_space_id: string | null;
  last_seen: string | null;
  created_at: string;
  has_active_tokens: boolean;
}

/**
 * Update client settings request.
 */
export interface UpdateClientRequest {
  client_alias?: string;
  connection_mode?: 'follow_active' | 'locked' | 'ask_on_change';
  locked_space_id?: string | null;
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
