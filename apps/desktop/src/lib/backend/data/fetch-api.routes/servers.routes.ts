import { buildQuery } from '../fetch-api.helpers';
import type { RouteHandler } from '../fetch-api.types';

/** Installed servers, connections, and clone admin routes. */
export const serversRoutes: Record<string, RouteHandler> = {
  list_installed_servers: (args) => ({
    method: 'GET',
    path: `/api/v1/servers/installed${buildQuery({ spaceId: args.spaceId })}`,
  }),
  get_server_statuses: (args) => ({
    method: 'GET',
    path: `/api/v1/servers/connections${buildQuery({ spaceId: args.spaceId })}`,
  }),
  install_server: (args) => ({
    method: 'POST',
    path: '/api/v1/servers/install',
    body: { id: args.id, space_id: args.spaceId },
  }),
  uninstall_server: (args) => ({
    method: 'DELETE',
    path: `/api/v1/servers/${encodeURIComponent(String(args.id))}`,
    body: { space_id: args.spaceId },
  }),
  save_server_inputs: (args) => ({
    method: 'PUT',
    path: `/api/v1/servers/${encodeURIComponent(String(args.id))}/inputs`,
    body: {
      input_values: args.inputValues,
      space_id: args.spaceId,
      env_overrides: args.envOverrides,
      args_append: args.argsAppend,
      extra_headers: args.extraHeaders,
      default_params: args.defaultParams,
      display_name_override: args.displayNameOverride,
      update_policy: args.updatePolicy,
      pinned_version: args.pinnedVersion,
    },
  }),
  set_server_display_name: (args) => ({
    method: 'PUT',
    path: `/api/v1/servers/${encodeURIComponent(String(args.id))}/display-name`,
    body: { space_id: args.spaceId, display_name: args.displayName },
  }),
  set_server_oauth_connected: (args) => ({
    method: 'PUT',
    path: `/api/v1/servers/${encodeURIComponent(String(args.id))}/oauth-connected`,
    body: { space_id: args.spaceId, connected: args.connected },
  }),
  enable_server_v2: (args) => ({
    method: 'POST',
    path: '/api/v1/servers/connections/enable',
    body: { space_id: args.spaceId, server_id: args.serverId },
  }),
  disable_server_v2: (args) => ({
    method: 'POST',
    path: '/api/v1/servers/connections/disable',
    body: { space_id: args.spaceId, server_id: args.serverId },
  }),
  start_auth_v2: (args) => ({
    method: 'POST',
    path: '/api/v1/servers/connections/start-auth',
    body: { space_id: args.spaceId, server_id: args.serverId },
  }),
  cancel_auth_v2: (args) => ({
    method: 'POST',
    path: '/api/v1/servers/connections/cancel-auth',
    body: { space_id: args.spaceId, server_id: args.serverId },
  }),
  retry_connection: (args) => ({
    method: 'POST',
    path: '/api/v1/servers/connections/retry',
    body: { space_id: args.spaceId, server_id: args.serverId },
  }),
  update_server_package: (args) => ({
    method: 'POST',
    path: '/api/v1/servers/connections/update-package',
    body: { space_id: args.spaceId, server_id: args.serverId },
  }),
  logout_server: (args) => ({
    method: 'POST',
    path: '/api/v1/servers/connections/logout',
    body: { space_id: args.spaceId, server_id: args.serverId },
  }),
  clone_server: (args) => ({
    method: 'POST',
    path: '/api/v1/servers/clones',
    body: {
      space_id: args.spaceId,
      source_server_id: args.sourceServerId,
      suffix: args.suffix,
      alias: args.alias,
      display_name: args.displayName,
    },
  }),
  is_clone_id_available: (args) => ({
    method: 'GET',
    path: `/api/v1/servers/clones/available${buildQuery({
      spaceId: args.spaceId,
      sourceServerId: args.sourceServerId,
      suffix: args.suffix,
    })}`,
  }),
  suggest_clone_suffix: (args) => ({
    method: 'GET',
    path: `/api/v1/servers/clones/suggest${buildQuery({
      spaceId: args.spaceId,
      sourceServerId: args.sourceServerId,
    })}`,
  }),
  list_clone_dependents: (args) => ({
    method: 'GET',
    path: `/api/v1/servers/clones/dependents${buildQuery({
      spaceId: args.spaceId,
      sourceServerId: args.sourceServerId,
    })}`,
  }),
  check_all_server_updates: () => ({
    method: 'POST',
    path: '/api/v1/servers/updates/check-all',
  }),
  check_server_version: (args) => ({
    method: 'POST',
    path: `/api/v1/servers/${encodeURIComponent(String(args.serverId))}/updates/check`,
    body: { space_id: args.spaceId },
  }),
};
