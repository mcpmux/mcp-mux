import type { FeatureType, ServerFeature } from '@/lib/api/serverFeatures';
import { resolveInstalledDisplayName } from '@/features/servers/server-display-name.helpers';
import type { InstalledServerState, ServerDefinition } from '@/types/registry';

/** Most rows auto-expansion will render for a search before it falls back to collapsed groups. */
export const AUTO_EXPAND_ROW_CAP = 150;

export type SelectionFilter = 'all' | 'selected' | 'unselected';

export interface ServerGroup {
  serverId: string;
  label: string;
  icon: string | null;
  features: ServerFeature[];
}

export interface VisibleServerGroup {
  group: ServerGroup;
  matches: ServerFeature[];
}

const FEATURE_TYPE_ORDER: Record<FeatureType, number> = { tool: 0, prompt: 1, resource: 2 };

/**
 * Display title for a feature: the server-advertised title, else the raw MCP name.
 */
export function getFeatureLabel(feature: ServerFeature): string {
  return feature.display_name?.trim() || feature.feature_name;
}

/**
 * Parse the definition cached on an installed server, or null when absent or malformed.
 */
function parseCachedDefinition(state: InstalledServerState): ServerDefinition | null {
  if (!state.cached_definition) {
    return null;
  }
  try {
    return JSON.parse(state.cached_definition) as ServerDefinition;
  } catch {
    return null;
  }
}

/**
 * Order features tools first, then prompts, then resources, each by label.
 */
function compareFeatures(a: ServerFeature, b: ServerFeature): number {
  const typeDelta = FEATURE_TYPE_ORDER[a.feature_type] - FEATURE_TYPE_ORDER[b.feature_type];
  if (typeDelta !== 0) {
    return typeDelta;
  }
  return getFeatureLabel(a).localeCompare(getFeatureLabel(b));
}

/**
 * Group features by server, naming each group from the installed server record.
 * Groups are sorted by display name; servers with no install record fall back to
 * the last segment of their id.
 */
export function buildServerGroups(
  features: ServerFeature[],
  installed: InstalledServerState[]
): ServerGroup[] {
  const installedById = new Map(installed.map((state) => [state.server_id, state]));
  const featuresByServer = new Map<string, ServerFeature[]>();
  features.forEach((feature) => {
    const list = featuresByServer.get(feature.server_id);
    if (list) {
      list.push(feature);
      return;
    }
    featuresByServer.set(feature.server_id, [feature]);
  });

  return Array.from(featuresByServer, ([serverId, list]) => {
    const state = installedById.get(serverId);
    const definition = state ? parseCachedDefinition(state) : null;
    return {
      serverId,
      label: resolveInstalledDisplayName(
        state ?? { server_id: serverId, server_name: null },
        definition
      ),
      icon: definition?.icon ?? null,
      features: [...list].sort(compareFeatures),
    };
  }).sort((a, b) => a.label.localeCompare(b.label));
}

/**
 * Whether a feature matches an already trimmed, lowercased query on its own fields or its server's.
 */
function matchesQuery(feature: ServerFeature, group: ServerGroup, query: string): boolean {
  return [
    feature.feature_name,
    feature.display_name,
    feature.description,
    group.label,
    group.serverId,
  ].some((value) => value?.toLowerCase().includes(query));
}

/**
 * Whether the search box or the selection filter is narrowing the list.
 */
export function isFiltering(query: string, selectionFilter: SelectionFilter): boolean {
  return query.trim() !== '' || selectionFilter !== 'all';
}

/**
 * Narrow groups to the features matching the search query and selection filter.
 */
export function filterGroups(
  groups: ServerGroup[],
  query: string,
  selectionFilter: SelectionFilter,
  selectedIds: Set<string>
): VisibleServerGroup[] {
  const normalizedQuery = query.trim().toLowerCase();
  return groups
    .map((group) => ({
      group,
      matches: group.features.filter((feature) => {
        if (normalizedQuery && !matchesQuery(feature, group, normalizedQuery)) {
          return false;
        }
        if (selectionFilter === 'selected') {
          return selectedIds.has(feature.id);
        }
        if (selectionFilter === 'unselected') {
          return !selectedIds.has(feature.id);
        }
        return true;
      }),
    }))
    .filter(({ matches }) => matches.length > 0);
}

/**
 * Ids of the available features in the given matches. Bulk actions only touch these.
 */
export function getAvailableIds(features: ServerFeature[]): string[] {
  return features.filter((feature) => feature.is_available).map((feature) => feature.id);
}

/**
 * How many available features in the list are selected.
 */
export function countSelectedAvailable(
  features: ServerFeature[],
  selectedIds: Set<string>
): number {
  return features.filter((feature) => feature.is_available && selectedIds.has(feature.id)).length;
}

/**
 * Server ids to open for the current matches, or none when the rows would exceed the cap.
 */
export function getAutoExpandedServerIds(visibleGroups: VisibleServerGroup[]): Set<string> {
  const rowCount = visibleGroups.reduce((total, { matches }) => total + matches.length, 0);
  if (rowCount > AUTO_EXPAND_ROW_CAP) {
    return new Set();
  }
  return new Set(visibleGroups.map(({ group }) => group.serverId));
}
