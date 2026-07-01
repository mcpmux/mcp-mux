import { buildQuery } from '../fetch-api.helpers';
import type { RouteHandler } from '../fetch-api.types';

/** Workspace bindings and appearances routes. */
export const workspacesRoutes: Record<string, RouteHandler> = {
  list_workspace_bindings: () => ({ method: 'GET', path: '/api/v1/workspaces/bindings' }),
  list_workspace_bindings_for_space: (args) => ({
    method: 'GET',
    path: `/api/v1/workspaces/bindings/space/${encodeURIComponent(String(args.spaceId))}`,
  }),
  list_reported_workspace_roots: () => ({
    method: 'GET',
    path: '/api/v1/workspaces/reported-roots',
  }),
  clear_unmapped_reported_roots: () => ({
    method: 'POST',
    path: '/api/v1/workspaces/reported-roots/clear-unmapped',
  }),
  forget_reported_root: (args) => ({
    method: 'POST',
    path: '/api/v1/workspaces/reported-roots/forget',
    body: { root: args.root } as Record<string, unknown>,
  }),
  validate_workspace_root: (args) => ({
    method: 'GET',
    path: `/api/v1/workspaces/validate-root${buildQuery({ path: args.path })}`,
  }),
  get_workspace_effective_features: (args) => ({
    method: 'GET',
    path: `/api/v1/workspaces/effective-features${buildQuery({
      workspaceRoot: args.workspaceRoot,
      machineId: args.machineId,
    })}`,
  }),
  list_workspace_appearances: () => ({ method: 'GET', path: '/api/v1/workspaces/appearances' }),
  resolve_workspace_icon_path: (args) => ({
    method: 'GET',
    path: `/api/v1/workspaces/icon-path${buildQuery({ iconRef: args.iconRef })}`,
  }),
  create_workspace_binding: (args) => ({
    method: 'POST',
    path: '/api/v1/workspaces/bindings',
    body: args.input as Record<string, unknown>,
  }),
  update_workspace_binding: (args) => ({
    method: 'PUT',
    path: `/api/v1/workspaces/bindings/${encodeURIComponent(String(args.id))}`,
    body: args.input as Record<string, unknown>,
  }),
  delete_workspace_binding: (args) => ({
    method: 'DELETE',
    path: `/api/v1/workspaces/bindings/${encodeURIComponent(String(args.id))}`,
  }),
  upsert_workspace_appearance: (args) => ({
    method: 'PUT',
    path: '/api/v1/workspaces/appearances',
    body: args.input as Record<string, unknown>,
  }),
  delete_workspace_appearance: (args) => ({
    method: 'DELETE',
    path: '/api/v1/workspaces/appearances',
    body: { workspace_root: args.workspaceRoot },
  }),
  upload_workspace_icon: (args) => ({
    method: 'POST',
    path: '/api/v1/workspaces/appearances',
    body: { source_path: args.sourcePath },
  }),
};
