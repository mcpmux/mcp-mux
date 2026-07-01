import { buildQuery } from '../fetch-api.helpers';
import type { RouteHandler } from '../fetch-api.types';

/** Gateway lifecycle and pool admin routes. */
export const gatewayRoutes: Record<string, RouteHandler> = {
  get_gateway_status: (args) => ({
    method: 'GET',
    path: `/api/v1/gateway/status${buildQuery({ spaceId: args.spaceId })}`,
  }),
  probe_gateway_start: (args) => ({
    method: 'GET',
    path: `/api/v1/gateway/probe-start${buildQuery({ port: args.port })}`,
  }),
  take_pending_port_conflict: () => ({
    method: 'GET',
    path: '/api/v1/gateway/pending-port-conflict',
  }),
  get_gateway_port_settings: () => ({
    method: 'GET',
    path: '/api/v1/gateway/port-settings',
  }),
  reset_gateway_port: () => ({ method: 'GET', path: '/api/v1/gateway/reset-port' }),
  list_connected_servers: () => ({
    method: 'GET',
    path: '/api/v1/gateway/connected-servers',
  }),
  get_pool_stats: () => ({ method: 'GET', path: '/api/v1/gateway/pool-stats' }),
  start_gateway: (args) => ({
    method: 'POST',
    path: '/api/v1/gateway/start',
    body: { port: args.port, allowDynamicFallback: args.allowDynamicFallback },
  }),
  stop_gateway: () => ({ method: 'POST', path: '/api/v1/gateway/stop' }),
  restart_gateway: (args) => ({
    method: 'POST',
    path: '/api/v1/gateway/restart',
    body: { port: args.port, allowDynamicFallback: args.allowDynamicFallback },
  }),
  disconnect_server: (args) => ({
    method: 'POST',
    path: '/api/v1/gateway/disconnect',
    body: { serverId: args.serverId, spaceId: args.spaceId, logout: args.logout },
  }),
  connect_all_enabled_servers: () => ({
    method: 'POST',
    path: '/api/v1/gateway/connect-all',
  }),
  refresh_oauth_tokens_on_startup: () => ({
    method: 'POST',
    path: '/api/v1/gateway/refresh-oauth-tokens',
  }),
  set_gateway_port: (args) => ({
    method: 'PUT',
    path: '/api/v1/gateway/port',
    body: { port: args.port },
  }),
};
