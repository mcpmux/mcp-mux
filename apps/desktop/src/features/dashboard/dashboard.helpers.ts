import type { TFunction } from 'i18next';
import type { ConnectionStatus, ServerStatusResponse } from '@/lib/api/serverManager';
import type { InstalledServerState, ServerDefinition } from '@/types/registry';
import { resolveInstalledDisplayName } from '@/features/servers/server-display-name.helpers';
import i18n from '@/i18n';

/** Aggregated counts shown in the dashboard stat cards. */
export type DashboardStats = {
  installedServers: number;
  connectedServers: number;
  featureSets: number;
  clients: number;
  workspaceBindings: number;
  spaces: number;
};

/** Severity bucket for a server that needs operator attention. */
export type AttentionKind = 'error' | 'auth_required' | 'needs_setup';

/** One installed server surfaced in the health panel. */
export type AttentionServer = {
  serverId: string;
  displayName: string;
  kind: AttentionKind;
  detail: string;
};

const ATTENTION_PRIORITY: Record<AttentionKind, number> = {
  error: 0,
  auth_required: 1,
  needs_setup: 2,
};

const MAX_ATTENTION_SERVERS = 8;

/**
 * Resolve the dashboard namespace translator (hook call sites may pass their own `t`).
 */
function dashboardT(): TFunction<'dashboard'> {
  return i18n.getFixedT('dashboard');
}

/**
 * Whether an installed server is missing values for required transport inputs.
 */
export function hasMissingRequiredInputs(state: InstalledServerState): boolean {
  if (!state.cached_definition) {
    return false;
  }

  try {
    const definition = JSON.parse(state.cached_definition) as ServerDefinition;
    const inputs = definition.transport.metadata?.inputs ?? [];
    const values = state.input_values ?? {};

    return inputs.some((input) => input.required && !values[input.id]);
  } catch {
    return false;
  }
}

/**
 * Map a runtime connection status to a dashboard attention item, if any.
 */
export function attentionFromStatus(
  status: ConnectionStatus,
  message: string | null,
  t: TFunction<'dashboard'> = dashboardT()
): Pick<AttentionServer, 'kind' | 'detail'> | null {
  if (status === 'error') {
    return { kind: 'error', detail: message ?? t('health.details.connectionError') };
  }

  if (status === 'oauth_required') {
    return { kind: 'auth_required', detail: t('health.details.authRequired') };
  }

  return null;
}

/**
 * Build the ordered list of enabled servers that need attention in the current Space.
 */
export function buildAttentionServers(
  installed: InstalledServerState[],
  statuses: Record<string, ServerStatusResponse>,
  t: TFunction<'dashboard'> = dashboardT()
): AttentionServer[] {
  const items: AttentionServer[] = [];

  for (const server of installed) {
    if (!server.enabled) {
      continue;
    }

    const displayName = resolveInstalledDisplayName(server);
    const runtime = statuses[server.server_id];

    if (hasMissingRequiredInputs(server)) {
      items.push({
        serverId: server.server_id,
        displayName,
        kind: 'needs_setup',
        detail: t('health.details.missingConfig'),
      });
      continue;
    }

    if (runtime) {
      const fromStatus = attentionFromStatus(runtime.status, runtime.message, t);
      if (fromStatus) {
        items.push({
          serverId: server.server_id,
          displayName,
          ...fromStatus,
        });
      }
    }
  }

  return items
    .sort(
      (left, right) =>
        ATTENTION_PRIORITY[left.kind] - ATTENTION_PRIORITY[right.kind] ||
        left.displayName.localeCompare(right.displayName)
    )
    .slice(0, MAX_ATTENTION_SERVERS);
}
