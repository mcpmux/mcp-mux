import { useTranslation } from 'react-i18next';
import { Check, ChevronDown, ChevronRight, Minus } from 'lucide-react';
import { ServerIcon } from '@/components/ServerIcon';
import type { ServerFeature } from '@/lib/api/serverFeatures';
import { FeatureSetFeatureRow } from './feature-set-feature-row.component';
import {
  countSelectedAvailable,
  getAvailableIds,
  type ServerGroup,
} from './feature-set-features.helpers';

interface ServerGroupProps {
  group: ServerGroup;
  matches: ServerFeature[];
  isFiltering: boolean;
  isExpanded: boolean;
  selectedIds: Set<string>;
  surfacedIds: Set<string>;
  onToggleExpanded: (serverId: string) => void;
  onToggleFeature: (id: string) => void;
  onToggleSurfaced: (id: string) => void;
  onSetIdsSelected: (ids: string[], isSelected: boolean) => void;
}

const BADGE_EMPTY = 'bg-gray-100 dark:bg-gray-900/30 text-gray-600 dark:text-gray-400';
const BADGE_FULL =
  'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 border border-green-300 dark:border-green-700';
const BADGE_PARTIAL =
  'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 border border-amber-300 dark:border-amber-700';

/**
 * Collapsible server section: icon, name, selected count, a tri-state checkbox that
 * bulk-selects the visible available features, and the feature rows when open.
 */
export function FeatureSetServerGroup({
  group,
  matches,
  isFiltering,
  isExpanded,
  selectedIds,
  surfacedIds,
  onToggleExpanded,
  onToggleFeature,
  onToggleSurfaced,
  onSetIdsSelected,
}: ServerGroupProps) {
  const { t } = useTranslation('featuresets');
  const availableTotal = getAvailableIds(group.features).length;
  const selectedTotal = countSelectedAvailable(group.features, selectedIds);
  const visibleAvailableIds = getAvailableIds(matches);
  const visibleSelectedCount = countSelectedAvailable(matches, selectedIds);
  const areAllVisibleSelected =
    visibleAvailableIds.length > 0 && visibleSelectedCount === visibleAvailableIds.length;
  const isMixed = !areAllVisibleSelected && visibleSelectedCount > 0;
  const isFull = availableTotal > 0 && selectedTotal === availableTotal;
  let countBadgeClass = BADGE_PARTIAL;
  if (selectedTotal === 0) {
    countBadgeClass = BADGE_EMPTY;
  } else if (isFull) {
    countBadgeClass = BADGE_FULL;
  }

  return (
    <div className="bg-[rgb(var(--surface))]">
      <div
        className="flex cursor-pointer items-center justify-between gap-3 px-4 py-3 transition-colors hover:bg-[rgb(var(--surface-hover))]"
        onClick={() => onToggleExpanded(group.serverId)}
        data-testid={`featureset-server-group-${group.serverId}`}
      >
        <div className="flex min-w-0 flex-1 items-center gap-3">
          {isExpanded ? (
            <ChevronDown className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
          ) : (
            <ChevronRight className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
          )}
          <span className="flex h-6 w-6 flex-shrink-0 items-center justify-center text-lg">
            <ServerIcon icon={group.icon} className="h-6 w-6 object-contain" fallback="📦" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="mb-1 flex items-center gap-2">
              <span className="truncate text-sm font-medium" title={group.serverId}>
                {group.label}
              </span>
              <span
                className={`flex-shrink-0 rounded-full px-2 py-0.5 text-xs font-bold ${countBadgeClass}`}
              >
                {selectedTotal}/{availableTotal}
              </span>
              {isFiltering && (
                <span className="flex-shrink-0 text-xs text-[rgb(var(--muted))]">
                  {t('panel.matchCount', { count: matches.length })}
                </span>
              )}
            </div>
            {selectedTotal > 0 && (
              <div className="h-1 overflow-hidden rounded-full bg-gray-200 dark:bg-gray-800">
                <div
                  className={`h-full transition-all duration-300 ${
                    isFull ? 'bg-green-500' : 'bg-gradient-to-r from-amber-500 to-green-500'
                  }`}
                  style={{ width: `${(selectedTotal / availableTotal) * 100}%` }}
                />
              </div>
            )}
          </div>
        </div>

        <button
          type="button"
          role="checkbox"
          aria-checked={isMixed ? 'mixed' : areAllVisibleSelected}
          aria-label={areAllVisibleSelected ? t('panel.deselectServer') : t('panel.selectServer')}
          title={areAllVisibleSelected ? t('panel.deselectServer') : t('panel.selectServer')}
          disabled={visibleAvailableIds.length === 0}
          onClick={(event) => {
            event.stopPropagation();
            onSetIdsSelected(visibleAvailableIds, !areAllVisibleSelected);
          }}
          className={`flex h-5 w-5 flex-shrink-0 items-center justify-center rounded border transition-colors disabled:opacity-40 ${
            areAllVisibleSelected || isMixed
              ? 'bg-primary-500 border-primary-500'
              : 'border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--surface))]'
          }`}
        >
          {areAllVisibleSelected && <Check className="h-3.5 w-3.5 text-white" />}
          {isMixed && <Minus className="h-3.5 w-3.5 text-white" />}
        </button>
      </div>

      {isExpanded && (
        <div className="border-t border-[rgb(var(--border))] bg-[rgb(var(--background))]">
          {matches.map((feature) => (
            <FeatureSetFeatureRow
              key={feature.id}
              feature={feature}
              isSelected={selectedIds.has(feature.id)}
              isSurfaced={surfacedIds.has(feature.id)}
              onToggle={onToggleFeature}
              onToggleSurfaced={onToggleSurfaced}
            />
          ))}
        </div>
      )}
    </div>
  );
}
