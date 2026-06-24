/** HTTP route descriptor for admin REST transport. */
export interface ApiRoute {
  method: 'GET' | 'POST' | 'PUT' | 'DELETE';
  path: string;
  body?: Record<string, unknown>;
}

/** Maps Tauri command args to an admin REST route. */
export type RouteHandler = (args: Record<string, unknown>) => ApiRoute;
