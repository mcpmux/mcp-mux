import { buildQuery } from '../fetch-api.helpers';
import type { RouteHandler } from '../fetch-api.types';

/** App settings, logs, and meta-tool admin routes. */
export const appSettingsRoutes: Record<string, RouteHandler> = {
  get_startup_settings: () => ({ method: 'GET', path: '/api/v1/settings/startup' }),
  get_server_update_settings: () => ({ method: 'GET', path: '/api/v1/settings/server-updates' }),
  get_meta_tools_enabled: () => ({ method: 'GET', path: '/api/v1/settings/meta-tools-enabled' }),
  get_version: () => ({ method: 'GET', path: '/api/v1/app/version' }),
  get_bundle_version: () => ({ method: 'GET', path: '/api/v1/app/bundle-version' }),
  get_build_info: () => ({ method: 'GET', path: '/api/v1/app/build-info' }),
  get_logs_path: () => ({ method: 'GET', path: '/api/v1/app/logs-path' }),
  get_server_logs: (args) => ({
    method: 'GET',
    path: `/api/v1/logs/server/${encodeURIComponent(String(args.serverId))}${buildQuery({
      limit: args.limit,
      levelFilter: args.levelFilter,
    })}`,
  }),
  get_server_log_file: (args) => ({
    method: 'GET',
    path: `/api/v1/logs/server/${encodeURIComponent(String(args.serverId))}/file`,
  }),
  get_log_retention_days: () => ({ method: 'GET', path: '/api/v1/logs/retention-days' }),
  update_startup_settings: (args) => ({
    method: 'PUT',
    path: '/api/v1/settings/startup',
    body: args.settings as Record<string, unknown>,
  }),
  update_server_update_settings: (args) => ({
    method: 'PUT',
    path: '/api/v1/settings/server-updates',
    body: args.settings as Record<string, unknown>,
  }),
  set_meta_tools_enabled: (args) => ({
    method: 'PUT',
    path: '/api/v1/settings/meta-tools-enabled',
    body: { enabled: args.enabled },
  }),
  clear_server_logs: (args) => ({
    method: 'DELETE',
    path: `/api/v1/logs/server/${encodeURIComponent(String(args.serverId))}`,
  }),
  set_log_retention_days: (args) => ({
    method: 'PUT',
    path: '/api/v1/logs/retention-days',
    body: { days: args.days },
  }),
  list_meta_tool_grants: () => ({ method: 'GET', path: '/api/v1/meta-tools/grants' }),
  respond_to_meta_tool_approval: (args) => ({
    method: 'POST',
    path: '/api/v1/meta-tools/approval',
    body: {
      request_id: args.requestId,
      client_id: args.clientId,
      tool_name: args.toolName,
      decision: args.decision,
    },
  }),
  revoke_meta_tool_grant: (args) => ({
    method: 'POST',
    path: '/api/v1/meta-tools/grants/revoke',
    body: { client_id: args.clientId, tool_name: args.toolName },
  }),
};
