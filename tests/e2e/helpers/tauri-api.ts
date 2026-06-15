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

/** The system's `is_default` Space — the gateway's routing fallback. */
export async function getDefaultSpace(): Promise<Space | null> {
  const spaces = await listSpaces();
  return spaces.find((s) => s.is_default) ?? null;
}

/**
 * The Space tests operate against. In the e2e environment this is the default
 * Space (the gateway's routing fallback), so it aliases {@link getDefaultSpace}.
 * Kept as a distinct name because several specs read it as "the active Space".
 */
export async function getActiveSpace(): Promise<Space | null> {
  return getDefaultSpace();
}

// ============================================================================
// Client API
// ============================================================================

export interface Client {
  id: string;
  name: string;
  client_type: string;
  last_seen: string | null;
}

export interface CreateClientInput {
  name: string;
  client_type: string;
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

// ============================================================================
// FeatureSet API
// ============================================================================

export interface FeatureSet {
  id: string;
  name: string;
  // 'starter' is the current auto-seeded type; 'default' is the legacy alias.
  feature_set_type: 'starter' | 'default' | 'custom';
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

/** Grant a feature set to an OAuth client in a space (rootless-client routing). */
export async function grantOAuthClientFeatureSet(
  clientId: string,
  spaceId: string,
  featureSetId: string
): Promise<void> {
  return invoke<void>('grant_oauth_client_feature_set', {
    clientId,
    spaceId,
    featureSetId,
  });
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

// ============================================================================
// Workspace Binding API (primary routing config)
// ============================================================================

export interface WorkspaceBinding {
  id: string;
  workspace_root: string;
  space_id: string;
  /** A binding maps to zero or more FeatureSets (order = render order). */
  feature_set_ids: string[];
  created_at: string;
  updated_at: string;
}

export interface WorkspaceBindingInput {
  workspace_root: string;
  space_id: string;
  /** MAY be empty — an empty mapping means "no Space tools" for that root. */
  feature_set_ids: string[];
}

export async function listWorkspaceBindings(): Promise<WorkspaceBinding[]> {
  return invoke<WorkspaceBinding[]>('list_workspace_bindings');
}

export async function createWorkspaceBinding(
  input: WorkspaceBindingInput
): Promise<WorkspaceBinding> {
  return invoke<WorkspaceBinding>('create_workspace_binding', { input });
}

export async function updateWorkspaceBinding(
  id: string,
  input: WorkspaceBindingInput
): Promise<WorkspaceBinding> {
  return invoke<WorkspaceBinding>('update_workspace_binding', { id, input });
}

export async function deleteWorkspaceBinding(id: string): Promise<void> {
  return invoke<void>('delete_workspace_binding', { id });
}
