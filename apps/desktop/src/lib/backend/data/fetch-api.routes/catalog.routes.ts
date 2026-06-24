import { buildQuery } from '../fetch-api.helpers';
import type { RouteHandler } from '../fetch-api.types';

/** Registry discovery and server feature catalog routes. */
export const catalogRoutes: Record<string, RouteHandler> = {
  discover_servers: () => ({ method: 'GET', path: '/api/v1/registry/discover' }),
  get_server_definition: (args) => ({
    method: 'GET',
    path: `/api/v1/registry/definition/${encodeURIComponent(String(args.serverId))}`,
  }),
  get_registry_ui_config: () => ({ method: 'GET', path: '/api/v1/registry/ui-config' }),
  get_registry_home_config: () => ({ method: 'GET', path: '/api/v1/registry/home-config' }),
  is_registry_offline: () => ({ method: 'GET', path: '/api/v1/registry/offline' }),
  refresh_registry: () => ({ method: 'POST', path: '/api/v1/registry/refresh' }),
  list_server_features: (args) => ({
    method: 'GET',
    path: `/api/v1/server-features${buildQuery({
      spaceId: args.spaceId,
      includeUnavailable: args.includeUnavailable,
    })}`,
  }),
  list_server_features_by_server: (args) => ({
    method: 'GET',
    path: `/api/v1/server-features/by-server${buildQuery({
      spaceId: args.spaceId,
      serverId: args.serverId,
      includeUnavailable: args.includeUnavailable,
    })}`,
  }),
  list_server_features_by_type: (args) => ({
    method: 'GET',
    path: `/api/v1/server-features/by-type${buildQuery({
      spaceId: args.spaceId,
      serverId: args.serverId,
      featureType: args.featureType,
      includeUnavailable: args.includeUnavailable,
    })}`,
  }),
  get_server_feature: (args) => ({
    method: 'GET',
    path: `/api/v1/server-features/${encodeURIComponent(String(args.id))}`,
  }),
};
