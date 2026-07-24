import { buildQuery } from '../fetch-api.helpers';
import type { RouteHandler } from '../fetch-api.types';

/** Built-in server admin routes (per-Space enablement + tool toggles). */
export const builtinsRoutes: Record<string, RouteHandler> = {
  list_builtin_servers: (args) => ({
    method: 'GET',
    path: `/api/v1/builtins${buildQuery({ spaceId: args.spaceId })}`,
  }),
  set_builtin_server_enabled: (args) => ({
    method: 'PUT',
    path: '/api/v1/builtins/server-enabled',
    body: {
      space_id: args.spaceId,
      server_id: args.serverId,
      enabled: args.enabled,
    },
  }),
  set_builtin_tool_enabled: (args) => ({
    method: 'PUT',
    path: '/api/v1/builtins/tool-enabled',
    body: {
      space_id: args.spaceId,
      server_id: args.serverId,
      tool_name: args.toolName,
      enabled: args.enabled,
    },
  }),
};
