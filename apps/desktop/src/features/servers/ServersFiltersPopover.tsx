import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, SlidersHorizontal } from 'lucide-react';
import {
  Button,
  ChipButton,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  HoverTooltip,
} from '@mcpmux/ui';
import {
  STATUS_FILTER_IDS,
  TRANSPORT_FILTER_IDS,
  countActiveServerFilters,
  describeAppliedServerFilters,
  getStatusFilterLabel,
  getTransportFilterLabel,
  type StatusFilterKey,
  type TransportFilter,
} from './servers-page.helpers';

interface ServersFiltersPopoverProps {
  transportFilter: TransportFilter;
  onTransportFilterChange: (filter: TransportFilter) => void;
  activeStatusFilters: Set<StatusFilterKey>;
  onToggleStatusFilter: (statusKey: StatusFilterKey) => void;
  onClearStatusFilters: () => void;
  onClearAllFilters: () => void;
}

/**
 * Popover for transport (stdio/http) and Beeper-style multi-select status filters.
 */
export function ServersFiltersPopover({
  transportFilter,
  onTransportFilterChange,
  activeStatusFilters,
  onToggleStatusFilter,
  onClearStatusFilters,
  onClearAllFilters,
}: ServersFiltersPopoverProps) {
  const { t } = useTranslation('servers');
  const [open, setOpen] = useState(false);
  const activeCount = countActiveServerFilters(transportFilter, activeStatusFilters);
  const appliedFilterLines = describeAppliedServerFilters(t, transportFilter, activeStatusFilters);

  return (
    <HoverTooltip
      title={t('filters.appliedTitle')}
      lines={appliedFilterLines}
      hidden={open}
      data-testid="servers-filters-tooltip"
      className="flex-shrink-0"
    >
      <div className="inline-flex items-center gap-2">
        {activeCount > 0 && (
          <Button
            variant="ghost"
            size="md"
            type="button"
            onClick={onClearAllFilters}
            data-testid="servers-filters-clear-all"
          >
            {t('filters.clearAll')}
          </Button>
        )}
        <DropdownMenu open={open} onOpenChange={setOpen}>
          <DropdownMenuTrigger data-testid="servers-filters-trigger">
            <Button
              variant="secondary"
              size="md"
              type="button"
              className={
                activeCount > 0
                  ? 'bg-[rgb(var(--primary))]/10 text-[rgb(var(--primary))] border-[rgb(var(--primary))]/40'
                  : undefined
              }
            >
              <SlidersHorizontal className="h-4 w-4" />
              {t('filters.filters')}
              {activeCount > 0 && (
                <span
                  className="min-w-[1.25rem] px-1.5 py-0.5 text-xs font-semibold rounded-full bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))]"
                  data-testid="servers-filters-count"
                >
                  {activeCount}
                </span>
              )}
              <ChevronDown
                className={`h-4 w-4 text-[rgb(var(--muted))] transition-transform ${open ? 'rotate-180' : ''}`}
              />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-72 p-4 space-y-4" data-testid="servers-filters-popover">
          <div className="space-y-2">
            <p className="text-xs font-medium text-[rgb(var(--muted))]">{t('filters.transport')}</p>
            <div className="flex flex-wrap gap-2">
              {TRANSPORT_FILTER_IDS.map((filterId) => (
                <ChipButton
                  key={filterId}
                  active={transportFilter === filterId}
                  variant="fill"
                  onClick={() => onTransportFilterChange(filterId)}
                  data-testid={`servers-transport-filter-${filterId}`}
                >
                  {getTransportFilterLabel(t, filterId)}
                </ChipButton>
              ))}
            </div>
          </div>

          <div className="space-y-2">
            <p className="text-xs font-medium text-[rgb(var(--muted))]">{t('filters.status')}</p>
            <div className="flex flex-wrap gap-2">
              <ChipButton
                active={activeStatusFilters.size === 0}
                variant="fill"
                onClick={onClearStatusFilters}
                data-testid="servers-status-filter-all"
              >
                {t('filters.all')}
              </ChipButton>
              {STATUS_FILTER_IDS.map((filterId) => (
                <ChipButton
                  key={filterId}
                  active={activeStatusFilters.has(filterId)}
                  variant="outline"
                  onClick={() => onToggleStatusFilter(filterId)}
                  data-testid={`servers-status-filter-${filterId}`}
                >
                  {getStatusFilterLabel(t, filterId)}
                </ChipButton>
              ))}
            </div>
            <p className="text-xs text-[rgb(var(--muted))]">{t('filters.combineHint')}</p>
          </div>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </HoverTooltip>
  );
}
