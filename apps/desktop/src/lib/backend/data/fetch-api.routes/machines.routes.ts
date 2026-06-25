import type { RouteHandler } from '../fetch-api.types';

/** Machine catalog and local install identity admin routes. */
export const machinesRoutes: Record<string, RouteHandler> = {
  list_machines: () => ({ method: 'GET', path: '/api/v1/machines' }),
  create_machine: (args) => ({
    method: 'POST',
    path: '/api/v1/machines',
    body: { name: args.name, icon: args.icon, hostname: args.hostname },
  }),
  update_machine: (args) => ({
    method: 'PUT',
    path: `/api/v1/machines/${encodeURIComponent(String(args.id))}`,
    body: { name: args.name, icon: args.icon, hostname: args.hostname },
  }),
  delete_machine: (args) => ({
    method: 'DELETE',
    path: `/api/v1/machines/${encodeURIComponent(String(args.id))}`,
  }),
  get_local_machine_id: () => ({ method: 'GET', path: '/api/v1/machines/local' }),
  set_local_machine_id: (args) => ({
    method: 'PUT',
    path: '/api/v1/machines/local',
    body: { machine_id: args.machineId ?? null },
  }),
  get_hostname: () => ({ method: 'GET', path: '/api/v1/machines/hostname' }),
};
