import { describe, it, expect } from 'vitest';

import { ADMIN_SSE_CHANNELS } from '../../apps/desktop/src/lib/backend/events/admin-sse-hub';
import { routeFor, registeredCommands } from '../../apps/desktop/src/lib/api/fetch-api';

const SPACE_ID = '11111111-1111-1111-1111-111111111111';
const CLIENT_ID = '22222222-2222-2222-2222-222222222222';
const FEATURE_SET_ID = '33333333-3333-3333-3333-333333333333';
const SERVER_ID = 'demo-server';
const FEATURE_ID = '44444444-4444-4444-4444-444444444444';

/** P4 read commands — one vitest row per parity matrix entry. */
const P4_READ_ROUTES: Array<{
  command: string;
  args?: Record<string, unknown>;
  method: 'GET';
  path: string;
}> = [
  {
    command: 'get_gateway_status',
    args: { spaceId: SPACE_ID },
    method: 'GET',
    path: `/api/v1/gateway/status?spaceId=${SPACE_ID}`,
  },
  {
    command: 'probe_gateway_start',
    args: { port: 45818 },
    method: 'GET',
    path: '/api/v1/gateway/probe-start?port=45818',
  },
  { command: 'take_pending_port_conflict', method: 'GET', path: '/api/v1/gateway/pending-port-conflict' },
  { command: 'get_gateway_port_settings', method: 'GET', path: '/api/v1/gateway/port-settings' },
  { command: 'reset_gateway_port', method: 'GET', path: '/api/v1/gateway/reset-port' },
  { command: 'list_connected_servers', method: 'GET', path: '/api/v1/gateway/connected-servers' },
  { command: 'get_pool_stats', method: 'GET', path: '/api/v1/gateway/pool-stats' },
  { command: 'list_spaces', method: 'GET', path: '/api/v1/spaces' },
  { command: 'get_space', args: { id: SPACE_ID }, method: 'GET', path: `/api/v1/spaces/${SPACE_ID}` },
  {
    command: 'read_space_config',
    args: { spaceId: SPACE_ID },
    method: 'GET',
    path: `/api/v1/spaces/${SPACE_ID}/config`,
  },
  {
    command: 'list_installed_servers',
    args: { spaceId: SPACE_ID },
    method: 'GET',
    path: `/api/v1/servers/installed?spaceId=${SPACE_ID}`,
  },
  { command: 'discover_servers', method: 'GET', path: '/api/v1/registry/discover' },
  {
    command: 'get_server_definition',
    args: { serverId: SERVER_ID },
    method: 'GET',
    path: `/api/v1/registry/definition/${SERVER_ID}`,
  },
  { command: 'get_registry_ui_config', method: 'GET', path: '/api/v1/registry/ui-config' },
  { command: 'get_registry_home_config', method: 'GET', path: '/api/v1/registry/home-config' },
  { command: 'is_registry_offline', method: 'GET', path: '/api/v1/registry/offline' },
  { command: 'list_clients', method: 'GET', path: '/api/v1/clients' },
  { command: 'get_client', args: { id: CLIENT_ID }, method: 'GET', path: `/api/v1/clients/${CLIENT_ID}` },
  { command: 'list_feature_sets', method: 'GET', path: '/api/v1/feature-sets' },
  {
    command: 'list_feature_sets_by_space',
    args: { spaceId: SPACE_ID },
    method: 'GET',
    path: `/api/v1/feature-sets/by-space/${SPACE_ID}`,
  },
  {
    command: 'get_feature_set',
    args: { id: FEATURE_SET_ID },
    method: 'GET',
    path: `/api/v1/feature-sets/${FEATURE_SET_ID}`,
  },
  {
    command: 'get_feature_set_with_members',
    args: { id: FEATURE_SET_ID },
    method: 'GET',
    path: `/api/v1/feature-sets/${FEATURE_SET_ID}/with-members`,
  },
  { command: 'list_workspace_bindings', method: 'GET', path: '/api/v1/workspaces/bindings' },
  {
    command: 'list_workspace_bindings_for_space',
    args: { spaceId: SPACE_ID },
    method: 'GET',
    path: `/api/v1/workspaces/bindings/space/${SPACE_ID}`,
  },
  { command: 'list_reported_workspace_roots', method: 'GET', path: '/api/v1/workspaces/reported-roots' },
  {
    command: 'validate_workspace_root',
    args: { path: '/tmp/workspace' },
    method: 'GET',
    path: '/api/v1/workspaces/validate-root?path=%2Ftmp%2Fworkspace',
  },
  {
    command: 'get_workspace_effective_features',
    args: { workspaceRoot: '/tmp/workspace' },
    method: 'GET',
    path: '/api/v1/workspaces/effective-features?workspaceRoot=%2Ftmp%2Fworkspace',
  },
  { command: 'list_workspace_appearances', method: 'GET', path: '/api/v1/workspaces/appearances' },
  {
    command: 'resolve_workspace_icon_path',
    args: { iconRef: 'local:workspace-icons/demo.png' },
    method: 'GET',
    path: '/api/v1/workspaces/icon-path?iconRef=local%3Aworkspace-icons%2Fdemo.png',
  },
  { command: 'get_startup_settings', method: 'GET', path: '/api/v1/settings/startup' },
  { command: 'get_meta_tools_enabled', method: 'GET', path: '/api/v1/settings/meta-tools-enabled' },
  { command: 'get_version', method: 'GET', path: '/api/v1/app/version' },
  { command: 'get_bundle_version', method: 'GET', path: '/api/v1/app/bundle-version' },
  { command: 'get_build_info', method: 'GET', path: '/api/v1/app/build-info' },
  { command: 'get_logs_path', method: 'GET', path: '/api/v1/app/logs-path' },
  {
    command: 'get_server_logs',
    args: { serverId: SERVER_ID, limit: 50, levelFilter: 'error' },
    method: 'GET',
    path: `/api/v1/logs/server/${SERVER_ID}?limit=50&levelFilter=error`,
  },
  {
    command: 'get_server_log_file',
    args: { serverId: SERVER_ID },
    method: 'GET',
    path: `/api/v1/logs/server/${SERVER_ID}/file`,
  },
  { command: 'get_log_retention_days', method: 'GET', path: '/api/v1/logs/retention-days' },
  { command: 'get_oauth_clients', method: 'GET', path: '/api/v1/oauth/clients' },
  {
    command: 'get_oauth_client_grants',
    args: { clientId: CLIENT_ID, spaceId: SPACE_ID },
    method: 'GET',
    path: `/api/v1/oauth/clients/${CLIENT_ID}/grants/${SPACE_ID}`,
  },
  { command: 'list_meta_tool_grants', method: 'GET', path: '/api/v1/meta-tools/grants' },
  {
    command: 'list_server_features',
    args: { spaceId: SPACE_ID },
    method: 'GET',
    path: `/api/v1/server-features?spaceId=${SPACE_ID}`,
  },
  {
    command: 'list_server_features_by_server',
    args: { spaceId: SPACE_ID, serverId: SERVER_ID },
    method: 'GET',
    path: `/api/v1/server-features/by-server?spaceId=${SPACE_ID}&serverId=${SERVER_ID}`,
  },
  {
    command: 'list_server_features_by_type',
    args: { spaceId: SPACE_ID, serverId: SERVER_ID, featureType: 'tool' },
    method: 'GET',
    path: `/api/v1/server-features/by-type?spaceId=${SPACE_ID}&serverId=${SERVER_ID}&featureType=tool`,
  },
  {
    command: 'get_server_feature',
    args: { id: FEATURE_ID },
    method: 'GET',
    path: `/api/v1/server-features/${FEATURE_ID}`,
  },
  {
    command: 'is_clone_id_available',
    args: { spaceId: SPACE_ID, sourceServerId: SERVER_ID, suffix: 'work' },
    method: 'GET',
    path: `/api/v1/servers/clones/available?spaceId=${SPACE_ID}&sourceServerId=${SERVER_ID}&suffix=work`,
  },
  {
    command: 'suggest_clone_suffix',
    args: { spaceId: SPACE_ID, sourceServerId: SERVER_ID },
    method: 'GET',
    path: `/api/v1/servers/clones/suggest?spaceId=${SPACE_ID}&sourceServerId=${SERVER_ID}`,
  },
  {
    command: 'list_clone_dependents',
    args: { spaceId: SPACE_ID, sourceServerId: SERVER_ID },
    method: 'GET',
    path: `/api/v1/servers/clones/dependents?spaceId=${SPACE_ID}&sourceServerId=${SERVER_ID}`,
  },
  {
    command: 'list_builtin_servers',
    args: { spaceId: SPACE_ID },
    method: 'GET',
    path: `/api/v1/builtins?spaceId=${SPACE_ID}`,
  },
  {
    command: 'preview_config_export',
    args: {
      request: { client_type: 'cursor', space_id: SPACE_ID, mask_credentials: true },
    },
    method: 'GET',
    path: `/api/v1/config-export/preview?clientType=cursor&spaceId=${SPACE_ID}&maskCredentials=true`,
  },
  { command: 'get_config_paths', method: 'GET', path: '/api/v1/config-export/paths' },
];

/** P6 write commands — one vitest row per parity matrix entry. */
const P6_WRITE_ROUTES: Array<{
  command: string;
  args?: Record<string, unknown>;
  method: 'POST' | 'PUT' | 'DELETE';
  path: string;
}> = [
  { command: 'create_space', args: { name: 'Test' }, method: 'POST', path: '/api/v1/spaces' },
  { command: 'update_space', args: { id: SPACE_ID, input: { name: 'X' } }, method: 'PUT', path: `/api/v1/spaces/${SPACE_ID}` },
  { command: 'delete_space', args: { id: SPACE_ID }, method: 'DELETE', path: `/api/v1/spaces/${SPACE_ID}` },
  { command: 'save_space_config', args: { spaceId: SPACE_ID, content: '{}' }, method: 'PUT', path: `/api/v1/spaces/${SPACE_ID}/config` },
  { command: 'remove_server_from_config', args: { spaceId: SPACE_ID, serverId: SERVER_ID }, method: 'DELETE', path: `/api/v1/spaces/${SPACE_ID}/config/servers/${SERVER_ID}` },
  { command: 'start_gateway', args: { port: 45818 }, method: 'POST', path: '/api/v1/gateway/start' },
  { command: 'stop_gateway', method: 'POST', path: '/api/v1/gateway/stop' },
  { command: 'restart_gateway', method: 'POST', path: '/api/v1/gateway/restart' },
  { command: 'disconnect_server', args: { serverId: SERVER_ID, spaceId: SPACE_ID }, method: 'POST', path: '/api/v1/gateway/disconnect' },
  { command: 'connect_all_enabled_servers', method: 'POST', path: '/api/v1/gateway/connect-all' },
  { command: 'refresh_oauth_tokens_on_startup', method: 'POST', path: '/api/v1/gateway/refresh-oauth-tokens' },
  { command: 'set_gateway_port', args: { port: 45818 }, method: 'PUT', path: '/api/v1/gateway/port' },
  { command: 'install_server', args: { id: SERVER_ID, spaceId: SPACE_ID }, method: 'POST', path: '/api/v1/servers/install' },
  { command: 'uninstall_server', args: { id: SERVER_ID, spaceId: SPACE_ID }, method: 'DELETE', path: `/api/v1/servers/${SERVER_ID}` },
  { command: 'save_server_inputs', args: { id: SERVER_ID, inputValues: {}, spaceId: SPACE_ID }, method: 'PUT', path: `/api/v1/servers/${SERVER_ID}/inputs` },
  { command: 'set_server_display_name', args: { id: SERVER_ID, spaceId: SPACE_ID, displayName: 'Demo' }, method: 'PUT', path: `/api/v1/servers/${SERVER_ID}/display-name` },
  { command: 'set_server_oauth_connected', args: { id: SERVER_ID, spaceId: SPACE_ID, connected: true }, method: 'PUT', path: `/api/v1/servers/${SERVER_ID}/oauth-connected` },
  { command: 'enable_server_v2', args: { spaceId: SPACE_ID, serverId: SERVER_ID }, method: 'POST', path: '/api/v1/servers/connections/enable' },
  { command: 'disable_server_v2', args: { spaceId: SPACE_ID, serverId: SERVER_ID }, method: 'POST', path: '/api/v1/servers/connections/disable' },
  { command: 'start_auth_v2', args: { spaceId: SPACE_ID, serverId: SERVER_ID }, method: 'POST', path: '/api/v1/servers/connections/start-auth' },
  { command: 'cancel_auth_v2', args: { spaceId: SPACE_ID, serverId: SERVER_ID }, method: 'POST', path: '/api/v1/servers/connections/cancel-auth' },
  { command: 'retry_connection', args: { spaceId: SPACE_ID, serverId: SERVER_ID }, method: 'POST', path: '/api/v1/servers/connections/retry' },
  { command: 'update_server_package', args: { spaceId: SPACE_ID, serverId: SERVER_ID }, method: 'POST', path: '/api/v1/servers/connections/update-package' },
  { command: 'check_all_server_updates', method: 'POST', path: '/api/v1/servers/updates/check-all' },
  { command: 'check_server_version', args: { spaceId: SPACE_ID, serverId: SERVER_ID }, method: 'POST', path: `/api/v1/servers/${encodeURIComponent(SERVER_ID)}/updates/check` },
  { command: 'logout_server', args: { spaceId: SPACE_ID, serverId: SERVER_ID }, method: 'POST', path: '/api/v1/servers/connections/logout' },
  { command: 'clone_server', args: { spaceId: SPACE_ID, sourceServerId: SERVER_ID, suffix: 'work' }, method: 'POST', path: '/api/v1/servers/clones' },
  { command: 'create_feature_set', args: { input: { name: 'Set', space_id: SPACE_ID } }, method: 'POST', path: '/api/v1/feature-sets' },
  { command: 'update_feature_set', args: { id: FEATURE_SET_ID, input: { name: 'Set' } }, method: 'PUT', path: `/api/v1/feature-sets/${FEATURE_SET_ID}` },
  { command: 'delete_feature_set', args: { id: FEATURE_SET_ID }, method: 'DELETE', path: `/api/v1/feature-sets/${FEATURE_SET_ID}` },
  { command: 'add_feature_set_member', args: { featureSetId: FEATURE_SET_ID, input: { member_type: 'feature', member_id: FEATURE_ID } }, method: 'POST', path: `/api/v1/feature-sets/${FEATURE_SET_ID}/members` },
  { command: 'remove_feature_set_member', args: { featureSetId: FEATURE_SET_ID, memberId: FEATURE_ID }, method: 'DELETE', path: `/api/v1/feature-sets/${FEATURE_SET_ID}/members/${FEATURE_ID}` },
  { command: 'set_feature_set_members', args: { featureSetId: FEATURE_SET_ID, members: [] }, method: 'PUT', path: `/api/v1/feature-sets/${FEATURE_SET_ID}/members` },
  { command: 'create_client', args: { input: { name: 'C', client_type: 'custom' } }, method: 'POST', path: '/api/v1/clients' },
  { command: 'delete_client', args: { id: CLIENT_ID }, method: 'DELETE', path: `/api/v1/clients/${CLIENT_ID}` },
  { command: 'init_preset_clients', method: 'POST', path: '/api/v1/clients/init-presets' },
  { command: 'create_workspace_binding', args: { input: { workspace_root: '/tmp', space_id: SPACE_ID, feature_set_ids: [FEATURE_SET_ID] } }, method: 'POST', path: '/api/v1/workspaces/bindings' },
  { command: 'update_workspace_binding', args: { id: CLIENT_ID, input: { workspace_root: '/tmp', space_id: SPACE_ID, feature_set_ids: [FEATURE_SET_ID] } }, method: 'PUT', path: `/api/v1/workspaces/bindings/${CLIENT_ID}` },
  { command: 'delete_workspace_binding', args: { id: CLIENT_ID }, method: 'DELETE', path: `/api/v1/workspaces/bindings/${CLIENT_ID}` },
  { command: 'upsert_workspace_appearance', args: { input: { workspace_root: '/tmp', icon: 'local:workspace-icons/x.png' } }, method: 'PUT', path: '/api/v1/workspaces/appearances' },
  { command: 'delete_workspace_appearance', args: { workspaceRoot: '/tmp' }, method: 'DELETE', path: '/api/v1/workspaces/appearances' },
  { command: 'upload_workspace_icon', args: { sourcePath: '/tmp/icon.png' }, method: 'POST', path: '/api/v1/workspaces/appearances' },
  { command: 'update_startup_settings', args: { settings: { autoLaunch: false, startMinimized: true, closeToTray: true } }, method: 'PUT', path: '/api/v1/settings/startup' },
  { command: 'set_meta_tools_enabled', args: { enabled: true }, method: 'PUT', path: '/api/v1/settings/meta-tools-enabled' },
  { command: 'clear_server_logs', args: { serverId: SERVER_ID }, method: 'DELETE', path: `/api/v1/logs/server/${SERVER_ID}` },
  { command: 'set_log_retention_days', args: { days: 7 }, method: 'PUT', path: '/api/v1/logs/retention-days' },
  { command: 'refresh_registry', method: 'POST', path: '/api/v1/registry/refresh' },
  { command: 'respond_to_meta_tool_approval', args: { requestId: 'r1', clientId: CLIENT_ID, toolName: 'mcpmux_foo', decision: 'deny' }, method: 'POST', path: '/api/v1/meta-tools/approval' },
  { command: 'revoke_meta_tool_grant', args: { clientId: CLIENT_ID, toolName: 'mcpmux_foo' }, method: 'POST', path: '/api/v1/meta-tools/grants/revoke' },
  { command: 'update_oauth_client', args: { clientId: CLIENT_ID, settings: { client_alias: 'Alias' } }, method: 'PUT', path: `/api/v1/oauth/clients/${CLIENT_ID}` },
  { command: 'delete_oauth_client', args: { clientId: CLIENT_ID }, method: 'DELETE', path: `/api/v1/oauth/clients/${CLIENT_ID}` },
  { command: 'grant_oauth_client_feature_set', args: { clientId: CLIENT_ID, spaceId: SPACE_ID, featureSetId: FEATURE_SET_ID }, method: 'POST', path: `/api/v1/oauth/clients/${CLIENT_ID}/grants` },
  { command: 'revoke_oauth_client_feature_set', args: { clientId: CLIENT_ID, spaceId: SPACE_ID, featureSetId: FEATURE_SET_ID }, method: 'POST', path: `/api/v1/oauth/clients/${CLIENT_ID}/grants/revoke` },
  {
    command: 'get_pending_consent',
    args: { requestId: 'req-1' },
    method: 'GET',
    path: '/api/v1/oauth/consent/pending?requestId=req-1',
  },
  {
    command: 'approve_oauth_consent',
    args: {
      request: { request_id: 'req-1', consent_token: 'tok', client_alias: null, approved: true },
    },
    method: 'POST',
    path: '/api/v1/oauth/consent/approve',
  },
  {
    command: 'reject_oauth_consent',
    args: { request: { request_id: 'req-1', consent_token: 'tok', approved: false } },
    method: 'POST',
    path: '/api/v1/oauth/consent/reject',
  },
  {
    command: 'set_builtin_server_enabled',
    args: { spaceId: SPACE_ID, serverId: SERVER_ID, enabled: true },
    method: 'PUT',
    path: '/api/v1/builtins/server-enabled',
  },
  {
    command: 'set_builtin_tool_enabled',
    args: { spaceId: SPACE_ID, serverId: SERVER_ID, toolName: 'list_tools', enabled: false },
    method: 'PUT',
    path: '/api/v1/builtins/tool-enabled',
  },
  {
    command: 'check_config_exists',
    args: { clientType: 'cursor' },
    method: 'POST',
    path: '/api/v1/config-export/check',
  },
  {
    command: 'backup_existing_config',
    args: { clientType: 'cursor' },
    method: 'POST',
    path: '/api/v1/config-export/backup',
  },
  {
    command: 'export_config_to_file',
    args: {
      request: { client_type: 'cursor', space_id: SPACE_ID },
      path: '/tmp/mcp.json',
    },
    method: 'POST',
    path: '/api/v1/config-export/export',
  },
];

/** Direct UI SSE channels fan-out outside the domain-event bus (`ADMIN_SSE_CHANNELS`). */
const ADMIN_DIRECT_UI_SSE_CHANNELS = [
  'oauth-consent-request',
  'oauth-client-changed',
  'builtin-server-config-changed',
] as const;

describe('admin transport mapping', () => {
  it.each(P4_READ_ROUTES)('maps read $command', ({ command, args, method, path }) => {
    expect(routeFor(command, args)).toEqual({ method, path });
  });

  it.each(P6_WRITE_ROUTES)('maps write $command', ({ command, args, method, path }) => {
    const route = routeFor(command, args);
    expect(route.method).toEqual(method);
    expect(route.path).toEqual(path);
  });

  it('rejects unknown commands', () => {
    expect(() => routeFor('not_a_real_command')).toThrow('Unknown command: not_a_real_command');
  });

  it('does not register superseded apiCall commands', () => {
    expect(registeredCommands).not.toContain('export_config');
    expect(registeredCommands).not.toContain('connect_server');
    expect(registeredCommands).not.toContain('disconnect_server_v2');
  });
});

describe('admin SSE event channels', () => {
  it.each(ADMIN_DIRECT_UI_SSE_CHANNELS)(
    'documents web-admin direct SSE channel %s',
    (channel) => {
      expect(channel.length).toBeGreaterThan(0);
    }
  );

  it('keeps direct UI SSE channels separate from domain SSE channels', () => {
    for (const channel of ADMIN_DIRECT_UI_SSE_CHANNELS) {
      expect(ADMIN_SSE_CHANNELS).not.toContain(channel);
    }
  });
});
