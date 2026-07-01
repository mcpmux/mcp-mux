import type { InstalledServerState, ServerDefinition } from '@/types/registry';

/**
 * Resolve the effective display label for an installed server.
 *
 * Mirrors the Rust `InstalledServer::display_name()` precedence so the UI and
 * meta-tools agree on what to show. Order:
 * 1. `display_name_override` (user-supplied, survives user-config sync)
 * 2. `server_name` cached from the definition at install time
 * 3. `definition.name` if a parsed registry definition is provided
 * 4. Final segment of `server_id`
 */
export function resolveInstalledDisplayName(
  state: Pick<InstalledServerState, 'server_id' | 'server_name' | 'display_name_override'>,
  definition?: Pick<ServerDefinition, 'name'> | null
): string {
  const override = state.display_name_override?.trim();
  if (override) {
    return override;
  }

  if (state.server_name && state.server_name.length > 0) {
    return state.server_name;
  }

  if (definition?.name) {
    return definition.name;
  }

  const tail = state.server_id.split('/').pop();
  return tail && tail.length > 0 ? tail : state.server_id;
}
