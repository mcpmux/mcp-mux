import type { RouteHandler } from '../fetch-api.types';

/** Space CRUD and config admin routes. */
export const spacesRoutes: Record<string, RouteHandler> = {
  list_spaces: () => ({ method: 'GET', path: '/api/v1/spaces' }),
  get_space: (args) => ({
    method: 'GET',
    path: `/api/v1/spaces/${encodeURIComponent(String(args.id))}`,
  }),
  read_space_config: (args) => ({
    method: 'GET',
    path: `/api/v1/spaces/${encodeURIComponent(String(args.spaceId))}/config`,
  }),
  create_space: (args) => ({
    method: 'POST',
    path: '/api/v1/spaces',
    body: { name: args.name, icon: args.icon },
  }),
  update_space: (args) => {
    const input = (args.input as Record<string, unknown> | undefined) ?? {
      name: args.name,
      icon: args.icon,
      description: args.description,
    };
    return {
      method: 'PUT',
      path: `/api/v1/spaces/${encodeURIComponent(String(args.id))}`,
      body: input,
    };
  },
  delete_space: (args) => ({
    method: 'DELETE',
    path: `/api/v1/spaces/${encodeURIComponent(String(args.id))}`,
  }),
  save_space_config: (args) => ({
    method: 'PUT',
    path: `/api/v1/spaces/${encodeURIComponent(String(args.spaceId))}/config`,
    body: { content: args.content },
  }),
  remove_server_from_config: (args) => ({
    method: 'DELETE',
    path: `/api/v1/spaces/${encodeURIComponent(String(args.spaceId))}/config/servers/${encodeURIComponent(String(args.serverId))}`,
  }),
  update_server_in_config: (args) => ({
    method: 'PUT',
    path: `/api/v1/spaces/${encodeURIComponent(String(args.spaceId))}/config/servers/${encodeURIComponent(String(args.serverId))}`,
    body: { entry: args.entry },
  }),
  list_space_base_dirs: (args) => ({
    method: 'GET',
    path: `/api/v1/spaces/${encodeURIComponent(String(args.spaceId))}/base-dirs`,
  }),
  add_space_base_dir: (args) => ({
    method: 'POST',
    path: `/api/v1/spaces/${encodeURIComponent(String(args.spaceId))}/base-dirs`,
    body: { path: args.path },
  }),
  remove_space_base_dir: (args) => ({
    method: 'DELETE',
    path: `/api/v1/spaces/base-dirs/${encodeURIComponent(String(args.id))}`,
  }),
};
