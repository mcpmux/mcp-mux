/**
 * Tauri API Helper for E2E Tests
 *
 * Uses window.__TAURI_TEST_API__ exposed by the app.
 */

// Generic invoke helper
export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return browser.execute(
    async (cmd: string, cmdArgs: Record<string, unknown>) => {
      if (!window.__TAURI_TEST_API__) {
        throw new Error('Tauri Test API not available');
      }
      return window.__TAURI_TEST_API__.invoke(cmd, cmdArgs);
    },
    command,
    args || {}
  ) as Promise<T>;
}

// Emit a Tauri event (for simulating deep link events in tests)
export async function emitEvent(event: string, payload: unknown): Promise<void> {
  return browser.execute(
    async (evt: string, data: unknown) => {
      if (!window.__TAURI_TEST_API__?.emit) {
        throw new Error('Tauri Test API emit not available');
      }
      return window.__TAURI_TEST_API__.emit(evt, data);
    },
    event,
    payload
  ) as Promise<void>;
}

// ============================================================================
// Space API
// ============================================================================

export interface Space {
  id: string;
  name: string;
  icon: string | null;
  is_default: boolean;
}

export async function createSpace(name: string, icon?: string): Promise<Space> {
  return invoke<Space>('create_space', { name, icon });
}

export async function deleteSpace(id: string): Promise<void> {
  return invoke<void>('delete_space', { id });
}

export async function listSpaces(): Promise<Space[]> {
  return invoke<Space[]>('list_spaces');
}

export async function getActiveSpace(): Promise<Space | null> {
  return invoke<Space | null>('get_active_space');
}

export async function setActiveSpace(id: string): Promise<void> {
  return invoke<void>('set_active_space', { id });
}

// ============================================================================
// Client API
// ============================================================================

export interface Client {
  id: string;
  name: string;
  client_type: string;
  connection_mode: 'locked' | 'follow_active' | 'ask_on_change';
  locked_space_id: string | null;
  grants: Record<string, string[]>;
}

export interface CreateClientInput {
  name: string;
  client_type: string;
  connection_mode: string;
  locked_space_id?: string;
}

export async function createClient(input: CreateClientInput): Promise<Client> {
  return invoke<Client>('create_client', { input });
}

export async function deleteClient(id: string): Promise<void> {
  return invoke<void>('delete_client', { id });
}

export async function listClients(): Promise<Client[]> {
  return invoke<Client[]>('list_clients');
}

export async function grantFeatureSetToClient(
  clientId: string,
  spaceId: string,
  featureSetId: string
): Promise<void> {
  return invoke<void>('grant_feature_set_to_client', { clientId, spaceId, featureSetId });
}

/** Grant a feature set to an OAuth/inbound client (Cursor, VS Code, etc.) */
export async function grantOAuthClientFeatureSet(
  clientId: string,
  spaceId: string,
  featureSetId: string
): Promise<void> {
  return invoke<void>('grant_oauth_client_feature_set', { clientId, spaceId, featureSetId });
}

// ============================================================================
// FeatureSet API
// ============================================================================

export interface FeatureSet {
  id: string;
  name: string;
  feature_set_type: 'all' | 'default' | 'server-all' | 'custom';
  server_id: string | null;
  is_builtin: boolean;
}

export async function listFeatureSetsBySpace(spaceId: string): Promise<FeatureSet[]> {
  return invoke<FeatureSet[]>('list_feature_sets_by_space', { spaceId });
}

export async function createFeatureSet(input: {
  name: string;
  space_id: string;
  description?: string;
  icon?: string;
}): Promise<FeatureSet> {
  return invoke<FeatureSet>('create_feature_set', { input });
}

export async function deleteFeatureSet(id: string): Promise<void> {
  return invoke<void>('delete_feature_set', { id });
}

/** Add a feature (tool/prompt/resource) to a feature set. */
export async function addFeatureToSet(
  featureSetId: string,
  featureId: string,
  mode: 'include' | 'exclude' = 'include'
): Promise<void> {
  return invoke<void>('add_feature_to_set', { featureSetId, featureId, mode });
}

/** List all server features in a space. */
export async function listServerFeatures(
  spaceId: string,
  includeUnavailable?: boolean
): Promise<{ id: string; server_id: string; feature_type: string; feature_name: string }[]> {
  return invoke('list_server_features', { spaceId, includeUnavailable });
}

// ============================================================================
// Server API
// ============================================================================

export interface InstalledServer {
  id: string;
  space_id: string;
  server_id: string; // Definition ID (e.g. "github-server")
  is_enabled?: boolean;
  enabled?: boolean;
  input_values: Record<string, string>;
}

export async function installServer(id: string, spaceId: string): Promise<void> {
  return invoke<void>('install_server', { id, spaceId });
}

export async function uninstallServer(id: string, spaceId: string): Promise<void> {
  return invoke<void>('uninstall_server', { id, spaceId });
}

export async function listInstalledServers(spaceId?: string): Promise<InstalledServer[]> {
  return invoke<InstalledServer[]>('list_installed_servers', { spaceId });
}

export async function saveServerInputs(
  id: string,
  inputValues: Record<string, string>,
  spaceId: string
): Promise<void> {
  return invoke<void>('save_server_inputs', { id, inputValues, spaceId });
}

export async function enableServerV2(spaceId: string, serverId: string): Promise<void> {
  return invoke<void>('enable_server_v2', { spaceId, serverId });
}

export async function disableServerV2(spaceId: string, serverId: string): Promise<void> {
  return invoke<void>('disable_server_v2', { spaceId, serverId });
}

// ============================================================================
// Registry API
// ============================================================================

/** Force-refresh the server registry bundle (bypasses cache). */
export async function refreshRegistry(): Promise<void> {
  return invoke<void>('refresh_registry');
}

// ============================================================================
// OAuth API
// ============================================================================

/** Approve a DCR-registered OAuth client by ID (for E2E testing). */
export async function approveOAuthClient(clientId: string): Promise<void> {
  return invoke<void>('approve_oauth_client', { clientId });
}

// ============================================================================
// Server Feature Seeding API (for E2E / screenshots)
// ============================================================================

export interface SeedFeatureInput {
  space_id: string;
  server_id: string;
  feature_type: 'tool' | 'prompt' | 'resource';
  feature_name: string;
  display_name?: string;
  description?: string;
}

/** Seed server features into the database for screenshot/E2E purposes. */
export async function seedServerFeatures(features: SeedFeatureInput[]): Promise<string[]> {
  return invoke<string[]>('seed_server_features', { features });
}

// ============================================================================
// Logs API
// ============================================================================

export interface ServerLogEntry {
  timestamp: string;
  level: string;
  source: string;
  message: string;
  metadata?: Record<string, unknown>;
}

export async function getServerLogs(
  serverId: string,
  limit?: number,
  levelFilter?: string
): Promise<ServerLogEntry[]> {
  return invoke<ServerLogEntry[]>('get_server_logs', {
    serverId,
    limit,
    levelFilter,
  });
}

export async function clearServerLogs(serverId: string): Promise<void> {
  return invoke<void>('clear_server_logs', { serverId });
}

// ============================================================================
// Gateway API
// ============================================================================

export interface GatewayStatus {
  running: boolean;
  url: string | null;
  connected_backends: number;
}

export async function getGatewayStatus(): Promise<GatewayStatus> {
  return invoke<GatewayStatus>('get_gateway_status');
}
