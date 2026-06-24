import { appSettingsRoutes } from './app-settings.routes';
import { catalogRoutes } from './catalog.routes';
import { configExportRoutes } from './config-export.routes';
import { gatewayRoutes } from './gateway.routes';
import { permissionsRoutes } from './permissions.routes';
import { serversRoutes } from './servers.routes';
import { spacesRoutes } from './spaces.routes';
import { workspacesRoutes } from './workspaces.routes';
import type { ApiRoute } from '../fetch-api.types';

const COMMAND_ROUTES = {
  ...gatewayRoutes,
  ...spacesRoutes,
  ...serversRoutes,
  ...catalogRoutes,
  ...permissionsRoutes,
  ...workspacesRoutes,
  ...appSettingsRoutes,
  ...configExportRoutes,
};

/**
 * Map a Tauri IPC command name and its argument object to an admin REST route.
 *
 * @param command - Tauri command identifier (e.g. `list_spaces`, `start_gateway`).
 * @param args - Command-specific payload; keys use camelCase matching the TS API layer.
 * @returns HTTP method, path, and optional JSON body for `fetchApi`.
 */
export function routeFor(command: string, args: Record<string, unknown> = {}): ApiRoute {
  const handler = COMMAND_ROUTES[command as keyof typeof COMMAND_ROUTES];
  if (!handler) {
    throw new Error(`Unknown command: ${command}`);
  }
  return handler(args);
}

/** All registered admin transport command names (for tests and diagnostics). */
export const registeredCommands = Object.keys(COMMAND_ROUTES).sort();
