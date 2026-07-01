import type { RouteHandler } from '../fetch-api.types';

/** Machine catalog and local install identity admin routes. */
export const machinesRoutes: Record<string, RouteHandler> = {
  list_machines: () => ({ method: 'GET', path: '/api/v1/machines' }),
  create_machine: (args) => ({
    method: 'POST',
    path: '/api/v1/machines',
    body: args.input as Record<string, unknown>,
  }),
  update_machine: (args) => ({
    method: 'PUT',
    path: `/api/v1/machines/${encodeURIComponent(String(args.id))}`,
    body: args.input as Record<string, unknown>,
  }),
  delete_machine: (args) => ({
    method: 'DELETE',
    path: `/api/v1/machines/${encodeURIComponent(String(args.id))}`,
  }),
  get_local_machine_id: () => ({ method: 'GET', path: '/api/v1/machines/local' }),
  set_local_machine_id: (args) => ({
    method: 'PUT',
    path: '/api/v1/machines/local',
    body: (args.input ?? { machine_id: null }) as Record<string, unknown>,
  }),
  get_hostname: () => ({ method: 'GET', path: '/api/v1/machines/hostname' }),
  get_viewer_machine_id: (args) => ({
    method: 'GET',
    path: `/api/v1/machines/viewer/${encodeURIComponent(String(args.viewerId))}`,
  }),
  set_viewer_machine_id: (args) => ({
    method: 'PUT',
    path: `/api/v1/machines/viewer/${encodeURIComponent(String(args.viewerId))}`,
    body: (args.input ?? { machine_id: null }) as Record<string, unknown>,
  }),
  get_client_machine_id: (args) => ({
    method: 'GET',
    path: `/api/v1/oauth/clients/${encodeURIComponent(String(args.clientId))}/machine`,
  }),
  set_client_machine_id: (args) => ({
    method: 'PUT',
    path: `/api/v1/oauth/clients/${encodeURIComponent(String(args.clientId))}/machine`,
    body: (args.input ?? { machine_id: null }) as Record<string, unknown>,
  }),
};
