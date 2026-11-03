import { useTranslation } from 'react-i18next';
import { Button, SearchField } from '@mcpmux/ui';
import type { SelectionFilter } from './feature-set-features.helpers';

interface FeaturesToolbarProps {
  query: string;
  selectionFilter: SelectionFilter;
  matchCount: number;
  isFiltering: boolean;
  areAllVisibleSelected: boolean;
  hasVisibleAvailable: boolean;
  onQueryChange: (query: string) => void;
  onSelectionFilterChange: (filter: SelectionFilter) => void;
  onToggleVisible: () => void;
}

const FILTER_OPTIONS = [
  { value: 'all', labelKey: 'panel.filterAll' },
  { value: 'selected', labelKey: 'panel.filterSelected' },
  { value: 'unselected', labelKey: 'panel.filterUnselected' },
] as const satisfies readonly { value: SelectionFilter; labelKey: string }[];

/**
 * Search box, All/Selected/Unselected filter, and the bulk select button for the visible rows.
 */
export function FeatureSetFeaturesToolbar({
  query,
  selectionFilter,
  matchCount,
  isFiltering,
  areAllVisibleSelected,
  hasVisibleAvailable,
  onQueryChange,
  onSelectionFilterChange,
  onToggleVisible,
}: FeaturesToolbarProps) {
  const { t } = useTranslation('featuresets');
  let bulkLabel = t('panel.selectAll');
  if (areAllVisibleSelected) {
    bulkLabel = isFiltering
      ? t('panel.deselectMatches', { count: matchCount })
      : t('panel.deselectAll');
  } else if (isFiltering) {
    bulkLabel = t('panel.selectMatches', { count: matchCount });
  }

  return (
    <div className="space-y-2 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3">
      <SearchField
        value={query}
        onChange={(event) => onQueryChange(event.target.value)}
        onClear={() => onQueryChange('')}
        placeholder={t('panel.searchPlaceholder')}
        data-testid="featureset-panel-search"
      />
      <div className="flex items-center justify-between gap-2">
        <div
          role="group"
          aria-label={t('panel.filterLabel')}
          className="inline-flex overflow-hidden rounded-lg border border-[rgb(var(--border))]"
        >
          {FILTER_OPTIONS.map(({ value, labelKey }) => (
            <button
              key={value}
              type="button"
              aria-pressed={selectionFilter === value}
              onClick={() => onSelectionFilterChange(value)}
              data-testid={`featureset-filter-${value}`}
              className={`px-3 py-1 text-xs font-medium transition-colors ${
                selectionFilter === value
                  ? 'bg-primary-500 text-white'
                  : 'bg-[rgb(var(--background))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))]'
              }`}
            >
              {t(labelKey)}
            </button>
          ))}
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={onToggleVisible}
          disabled={!hasVisibleAvailable}
          data-testid="featureset-select-all"
        >
          {bulkLabel}
        </Button>
      </div>
    </div>
  );
}
