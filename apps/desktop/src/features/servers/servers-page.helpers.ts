import type { TFunction } from 'i18next';
import type { ServerFeature } from '@/lib/api/serverFeatures';
import type { ServerViewModel } from '../../types/registry';

/** Runtime action used to derive status filter buckets. */
export type ServerActionKey =
  | 'enable'
  | 'configure'
  | 'connecting'
  | 'authenticating'
  | 'auth_required'
  | 'running'
  | 'error'
  | 'connected_auto';

/** Transport filter for installed servers. */
export type TransportFilter = 'all' | 'stdio' | 'http';

/** Status bucket for Beeper-style multi-select filters. */
export type StatusFilterKey = 'connected' | 'disabled' | 'error' | 'needs_setup';

export const TRANSPORT_FILTER_IDS: TransportFilter[] = ['all', 'stdio', 'http'];

export const STATUS_FILTER_IDS: StatusFilterKey[] = [
  'connected',
  'disabled',
  'error',
  'needs_setup',
];

/**
 * Resolve the display label for a transport filter chip.
 */
export function getTransportFilterLabel(t: TFunction<'servers'>, id: TransportFilter): string {
  switch (id) {
    case 'all':
      return t('filters.transportAll');
    case 'stdio':
      return t('filters.transportStdio');
    case 'http':
      return t('filters.transportHttp');
    default: {
      const _exhaustive: never = id;
      return _exhaustive;
    }
  }
}

/**
 * Resolve the display label for a status filter chip.
 */
export function getStatusFilterLabel(t: TFunction<'servers'>, id: StatusFilterKey): string {
  switch (id) {
    case 'connected':
      return t('filters.statusConnected');
    case 'disabled':
      return t('filters.statusDisabled');
    case 'error':
      return t('filters.statusError');
    case 'needs_setup':
      return t('filters.statusNeedsSetup');
    default: {
      const _exhaustive: never = id;
      return _exhaustive;
    }
  }
}

/** Group discovered features by installed server id. */
export function groupFeaturesByServerId(features: ServerFeature[]): Record<string, ServerFeature[]> {
  return features.reduce<Record<string, ServerFeature[]>>((acc, feature) => {
    const bucket = acc[feature.server_id] ?? [];
    bucket.push(feature);
    acc[feature.server_id] = bucket;
    return acc;
  }, {});
}

/**
 * Map a server action to the status filter bucket it belongs in.
 */
export function statusKeyFromAction(action: ServerActionKey): StatusFilterKey {
  switch (action) {
    case 'running':
    case 'connected_auto':
      return 'connected';
    case 'enable':
      return 'disabled';
    case 'error':
      return 'error';
    default:
      return 'needs_setup';
  }
}

/** Whether a server matches the selected transport filter. */
export function matchesTransport(server: ServerViewModel, transportFilter: TransportFilter): boolean {
  if (transportFilter === 'all') {
    return true;
  }

  return server.transport.type === transportFilter;
}

/**
 * Whether a server matches active status toggles.
 * Empty set means show all (Beeper-style: no status filter applied).
 */
export function matchesStatus(
  action: ServerActionKey,
  activeStatusFilters: ReadonlySet<StatusFilterKey>
): boolean {
  if (activeStatusFilters.size === 0) {
    return true;
  }

  return activeStatusFilters.has(statusKeyFromAction(action));
}

/** Whether a feature name or description matches the search query. */
function featureMatchesQuery(feature: ServerFeature, query: string): boolean {
  return (
    feature.feature_name.toLowerCase().includes(query) ||
    (feature.display_name?.toLowerCase().includes(query) ?? false) ||
    (feature.description?.toLowerCase().includes(query) ?? false)
  );
}

/**
 * Whether an installed server matches transport, status, and search filters.
 */
export function serverMatchesFilters(
  server: ServerViewModel,
  searchQuery: string,
  features: ServerFeature[],
  transportFilter: TransportFilter,
  activeStatusFilters: ReadonlySet<StatusFilterKey>,
  serverAction: ServerActionKey
): boolean {
  if (!matchesTransport(server, transportFilter)) {
    return false;
  }

  if (!matchesStatus(serverAction, activeStatusFilters)) {
    return false;
  }

  const query = searchQuery.trim().toLowerCase();
  if (!query) {
    return true;
  }

  const metadataMatch =
    server.name.toLowerCase().includes(query) ||
    server.id.toLowerCase().includes(query) ||
    (server.description?.toLowerCase().includes(query) ?? false);

  if (metadataMatch) {
    return true;
  }

  return features.some((feature) => featureMatchesQuery(feature, query));
}

/**
 * Count non-default transport and status filters for the Filters button badge.
 */
export function countActiveServerFilters(
  transportFilter: TransportFilter,
  activeStatusFilters: ReadonlySet<StatusFilterKey>
): number {
  let count = activeStatusFilters.size;
  if (transportFilter !== 'all') {
    count += 1;
  }
  return count;
}

/** Per-status counts for the My Servers header summary. */
export type ServerCountSummary = {
  installed: number;
  connected: number;
  disabled: number;
  error: number;
  needsSetup: number;
};

/**
 * Aggregate installed-server counts by status bucket (same buckets as status filters).
 */
export function computeServerCountSummary(
  servers: ServerViewModel[],
  getAction: (server: ServerViewModel) => ServerActionKey
): ServerCountSummary {
  const summary: ServerCountSummary = {
    installed: servers.length,
    connected: 0,
    disabled: 0,
    error: 0,
    needsSetup: 0,
  };

  for (const server of servers) {
    switch (statusKeyFromAction(getAction(server))) {
      case 'connected':
        summary.connected += 1;
        break;
      case 'disabled':
        summary.disabled += 1;
        break;
      case 'error':
        summary.error += 1;
        break;
      case 'needs_setup':
        summary.needsSetup += 1;
        break;
    }
  }

  return summary;
}

/**
 * Compact inline summary next to the My Servers title.
 */
export function formatServerCountSummary(
  t: TFunction<'servers'>,
  summary: ServerCountSummary
): string {
  return t('countSummary.inline', summary);
}

/**
 * Tooltip lines for the server count hover panel.
 */
export function describeServerCountSummary(
  t: TFunction<'servers'>,
  summary: ServerCountSummary
): string[] {
  const lines = [
    t('countSummary.installed', { count: summary.installed }),
    t('countSummary.connected', { count: summary.connected }),
    t('countSummary.disabled', { count: summary.disabled }),
    t('countSummary.error', { count: summary.error }),
  ];

  if (summary.needsSetup > 0) {
    lines.push(t('countSummary.needsSetup', { count: summary.needsSetup }));
  }

  return lines;
}

/**
 * Human-readable lines describing the currently applied server list filters.
 */
export function describeAppliedServerFilters(
  t: TFunction<'servers'>,
  transportFilter: TransportFilter,
  activeStatusFilters: ReadonlySet<StatusFilterKey>
): string[] {
  const transportLabel = getTransportFilterLabel(t, transportFilter);

  const statusLabel =
    activeStatusFilters.size === 0
      ? t('filters.all')
      : STATUS_FILTER_IDS.filter((filterId) => activeStatusFilters.has(filterId))
          .map((filterId) => getStatusFilterLabel(t, filterId))
          .join(', ');

  if (countActiveServerFilters(transportFilter, activeStatusFilters) === 0) {
    return [t('filters.noFiltersApplied'), t('filters.showingAll')];
  }

  return [
    t('filters.transportLine', { label: transportLabel }),
    t('filters.statusLine', { label: statusLabel }),
  ];
}
