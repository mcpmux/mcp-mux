import { buildQuery } from '../fetch-api.helpers';
import type { RouteHandler } from '../fetch-api.types';

/** MCP client config export admin routes (preview, paths, backup). */
export const configExportRoutes: Record<string, RouteHandler> = {
  preview_config_export: (args) => {
    const request = args.request as
      | { client_type?: string; space_id?: string; mask_credentials?: boolean }
      | undefined;
    return {
      method: 'GET',
      path: `/api/v1/config-export/preview${buildQuery({
        clientType: request?.client_type,
        spaceId: request?.space_id,
        maskCredentials: request?.mask_credentials,
      })}`,
    };
  },
  get_config_paths: () => ({ method: 'GET', path: '/api/v1/config-export/paths' }),
  check_config_exists: (args) => ({
    method: 'POST',
    path: '/api/v1/config-export/check',
    body: { clientType: args.clientType },
  }),
  backup_existing_config: (args) => ({
    method: 'POST',
    path: '/api/v1/config-export/backup',
    body: { clientType: args.clientType },
  }),
  export_config_to_file: (args) => ({
    method: 'POST',
    path: '/api/v1/config-export/export',
    body: { request: args.request, path: args.path },
  }),
};
