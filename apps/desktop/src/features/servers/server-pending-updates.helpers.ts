import type { InstalledServerState, ServerDefinition } from '@/types/registry';

import {
  isPackageManagedTransport,
  resolveCurrentPackageVersion,
  shouldShowPackageUpdate,
} from './server-update-policy.helpers';

/** One installed server with a newer package available on the registry. */
export interface ServerPendingUpdate {
  spaceId: string;
  serverId: string;
  name: string;
  enabled: boolean;
  currentVersion: string | null;
  latestVersion: string;
}

/**
 * Resolve a server definition from cached install JSON or the discovery map.
 */
function resolveServerDefinition(
  state: InstalledServerState,
  definitionById: Map<string, ServerDefinition>
): ServerDefinition | null {
  if (state.cached_definition) {
    try {
      return JSON.parse(state.cached_definition) as ServerDefinition;
    } catch {
      return definitionById.get(state.server_id) ?? null;
    }
  }
  return definitionById.get(state.server_id) ?? null;
}

/**
 * Build the list of package-managed installs that have a newer version available.
 */
export function buildPendingServerUpdates(
  installed: InstalledServerState[],
  definitions: ServerDefinition[] = []
): ServerPendingUpdate[] {
  const definitionById = new Map(definitions.map((definition) => [definition.id, definition]));
  const pending: ServerPendingUpdate[] = [];

  for (const state of installed) {
    const definition = resolveServerDefinition(state, definitionById);
    if (!definition || definition.transport.type !== 'stdio') {
      continue;
    }

    const command = definition.transport.command;
    if (!isPackageManagedTransport(command)) {
      continue;
    }

    const currentVersion = resolveCurrentPackageVersion({
      pinnedVersion: state.pinned_version,
      transportCommand: command,
      transportArgs: definition.transport.args,
      installedVersion: state.current_version,
    });
    const latestVersion = state.latest_available_version;
    if (
      !latestVersion ||
      !shouldShowPackageUpdate({
        updatePolicy: state.update_policy ?? 'notify',
        latestVersion,
        currentVersion,
        transportCommand: command,
        transportArgs: definition.transport.args,
      })
    ) {
      continue;
    }

    pending.push({
      spaceId: state.space_id,
      serverId: state.server_id,
      name: state.server_name ?? definition.name ?? state.server_id,
      enabled: state.enabled,
      currentVersion,
      latestVersion,
    });
  }

  return pending.sort((left, right) => left.name.localeCompare(right.name));
}
