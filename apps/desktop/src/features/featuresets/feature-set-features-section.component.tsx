import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2, Package } from 'lucide-react';
import type { ServerFeature } from '@/lib/api/serverFeatures';
import type { InstalledServerState } from '@/types/registry';
import { FeatureSetFeaturesToolbar } from './feature-set-features-toolbar.component';
import { FeatureSetServerGroup } from './feature-set-server-group.component';
import {
  buildServerGroups,
  countSelectedAvailable,
  filterGroups,
  getAutoExpandedServerIds,
  getAvailableIds,
  isFiltering,
  type SelectionFilter,
} from './feature-set-features.helpers';

interface FeaturesSectionProps {
  features: ServerFeature[];
  installed: InstalledServerState[];
  isLoading: boolean;
  selectedIds: Set<string>;
  surfacedIds: Set<string>;
  onToggleFeature: (id: string) => void;
  onToggleSurfaced: (id: string) => void;
  onSetIdsSelected: (ids: string[], isSelected: boolean) => void;
}

/**
 * Searchable, filterable list of server groups. While a search or filter is active,
 * groups with matches open on their own unless that would render too many rows.
 */
export function FeatureSetFeaturesSection({
  features,
  installed,
  isLoading,
  selectedIds,
  surfacedIds,
  onToggleFeature,
  onToggleSurfaced,
  onSetIdsSelected,
}: FeaturesSectionProps) {
  const { t } = useTranslation('featuresets');
  const [query, setQuery] = useState('');
  const [selectionFilter, setSelectionFilter] = useState<SelectionFilter>('all');
  const [toggledServerIds, setToggledServerIds] = useState<Set<string>>(new Set());

  const groups = useMemo(() => buildServerGroups(features, installed), [features, installed]);
  const visibleGroups = useMemo(
    () => filterGroups(groups, query, selectionFilter, selectedIds),
    [groups, query, selectionFilter, selectedIds]
  );
  const isNarrowed = isFiltering(query, selectionFilter);
  const autoExpandedIds = useMemo(
    () => (isNarrowed ? getAutoExpandedServerIds(visibleGroups) : new Set<string>()),
    [isNarrowed, visibleGroups]
  );

  const visibleFeatures = visibleGroups.flatMap(({ matches }) => matches);
  const visibleAvailableIds = getAvailableIds(visibleFeatures);
  const areAllVisibleSelected =
    visibleAvailableIds.length > 0 &&
    countSelectedAvailable(visibleFeatures, selectedIds) === visibleAvailableIds.length;
  const hasTooManyMatches = isNarrowed && visibleGroups.length > 0 && autoExpandedIds.size === 0;

  /**
   * Narrowing the list starts a fresh expansion state so earlier manual toggles don't invert it.
   */
  const resetToggles = () => setToggledServerIds(new Set());

  /**
   * Open or close a group, relative to whether search would have opened it.
   */
  const toggleExpanded = (serverId: string) =>
    setToggledServerIds((prev) => {
      const next = new Set(prev);
      if (next.has(serverId)) {
        next.delete(serverId);
      } else {
        next.add(serverId);
      }
      return next;
    });

  let body = (
    <div className="divide-y divide-[rgb(var(--border))]">
      {visibleGroups.map(({ group, matches }) => (
        <FeatureSetServerGroup
          key={group.serverId}
          group={group}
          matches={matches}
          isFiltering={isNarrowed}
          isExpanded={autoExpandedIds.has(group.serverId) !== toggledServerIds.has(group.serverId)}
          selectedIds={selectedIds}
          surfacedIds={surfacedIds}
          onToggleExpanded={toggleExpanded}
          onToggleFeature={onToggleFeature}
          onToggleSurfaced={onToggleSurfaced}
          onSetIdsSelected={onSetIdsSelected}
        />
      ))}
    </div>
  );
  if (isLoading) {
    body = (
      <div className="flex items-center justify-center py-16">
        <Loader2 className="text-primary-500 h-8 w-8 animate-spin" />
      </div>
    );
  } else if (visibleGroups.length === 0) {
    body = (
      <div className="flex flex-col items-center justify-center p-4 py-16 text-center text-[rgb(var(--muted))]">
        <Package className="mb-2 h-8 w-8 opacity-50" />
        <p className="text-sm">{t('panel.noFeaturesMatch')}</p>
      </div>
    );
  }

  return (
    <div className="overflow-hidden rounded-xl border-2 border-[rgb(var(--border))] bg-[rgb(var(--background))]">
      <FeatureSetFeaturesToolbar
        query={query}
        selectionFilter={selectionFilter}
        matchCount={visibleFeatures.length}
        isFiltering={isNarrowed}
        areAllVisibleSelected={areAllVisibleSelected}
        hasVisibleAvailable={visibleAvailableIds.length > 0}
        onQueryChange={(next) => {
          setQuery(next);
          resetToggles();
        }}
        onSelectionFilterChange={(next) => {
          setSelectionFilter(next);
          resetToggles();
        }}
        onToggleVisible={() => onSetIdsSelected(visibleAvailableIds, !areAllVisibleSelected)}
      />
      {hasTooManyMatches && (
        <p className="border-b border-[rgb(var(--border))] px-4 py-2 text-xs text-[rgb(var(--muted))]">
          {t('panel.narrowHint', { count: visibleFeatures.length })}
        </p>
      )}
      {body}
    </div>
  );
}
