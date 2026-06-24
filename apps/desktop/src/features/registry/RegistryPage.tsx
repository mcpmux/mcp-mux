/**
 * Registry page for browsing and installing MCP servers.
 * 
 * Uses API-driven filters and client-side sorting (see ADR-001).
 */

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';
import { useToast, ToastContainer } from '@mcpmux/ui';
import { useRegistryStore } from '../../stores/registryStore';
import { ServerCard } from './ServerCard';
import { ServerDetailModal } from './ServerDetailModal';
import { useViewSpace, useNavigateTo } from '@/stores';
import { capture } from '@/lib/analytics';
import { RequestServerCTA, ContributeMenu } from '@/components/Contribute';

export function RegistryPage() {
  const { t } = useTranslation('registry');
  const {
    servers,
    displayServers,
    uiConfig,
    activeFilters,
    activeSort,
    searchQuery,
    isLoading,
    error,
    selectedServer,
    isOffline,
    loadRegistry,
    setFilter,
    setSort,
    search,
    clearFilters,
    installServer,
    uninstallServer,
    selectServer,
    clearError,
    setSpaceId,
  } = useRegistryStore();

  const [localSearch, setLocalSearch] = useState('');
  const viewSpace = useViewSpace();
  const navigateTo = useNavigateTo();
  const { toasts, success, error: showToastError, dismiss } = useToast();

  const itemsPerPage = uiConfig?.items_per_page ?? 24;

  // Create a key that changes when filters/search/sort change to reset pagination
  const paginationKey = JSON.stringify({
    filters: activeFilters,
    sort: activeSort,
    search: searchQuery,
    length: displayServers.length
  });

  // Local page state that resets when key changes
  const [pageState, setPageState] = useState({ page: 1, key: paginationKey });

  // Reset page when key changes
  if (pageState.key !== paginationKey) {
    setPageState({ page: 1, key: paginationKey });
  }

  const activePage = pageState.page;

  // Pagination logic
  const totalPages = Math.ceil(displayServers.length / itemsPerPage);
  const paginatedServers = displayServers.slice(
    (activePage - 1) * itemsPerPage,
    activePage * itemsPerPage
  );

  const handlePageChange = (newPage: number) => {
    if (newPage >= 1 && newPage <= totalPages) {
      setPageState({ page: newPage, key: paginationKey });
      document.querySelector('.registry-grid-container')?.scrollTo({ top: 0, behavior: 'smooth' });
    }
  };

  // Load registry on mount
  useEffect(() => {
    setSpaceId(viewSpace?.id ?? null);
    loadRegistry(viewSpace?.id);
  }, [loadRegistry, setSpaceId, viewSpace?.id]);

  // Debounced search
  useEffect(() => {
    const timer = setTimeout(() => {
      if (localSearch !== searchQuery) {
        search(localSearch);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [localSearch, searchQuery, search]);

  // Track search analytics: one event per settled query, never per keystroke.
  useEffect(() => {
    const query = localSearch.trim();
    if (!query) return;
    const timer = setTimeout(() => {
      if (searchQuery.trim() !== query) return;
      capture('registry_search', {
        query,
        query_length: query.length,
        results_count: displayServers.length,
        has_results: displayServers.length > 0,
      });
    }, 1200);
    return () => clearTimeout(timer);
  }, [localSearch, searchQuery, displayServers.length]);

  const handleInstall = async (id: string) => {
    const server = servers.find(s => s.id === id);
    const serverName = server?.name || t('toast.fallbackServerName');
    try {
      await installServer(id, viewSpace?.id);
      success(t('toast.installed'), t('toast.installedBody', { name: serverName }), {
        duration: 6000,
        action: {
          label: t('toast.goToServers'),
          onClick: () => navigateTo('servers'),
        },
      });
    } catch {
      showToastError(t('toast.installFailed'), t('toast.installFailedBody', { name: serverName }));
    }
  };

  const handleUninstall = async (id: string) => {
    const server = servers.find(s => s.id === id);
    const serverName = server?.name || t('toast.fallbackServerName');
    try {
      await uninstallServer(id);
      if (selectedServer?.id === id) {
        selectServer(null);
      }
      success(t('toast.uninstalled'), t('toast.uninstalledBody', { name: serverName }));
    } catch {
      showToastError(t('toast.uninstallFailed'), t('toast.uninstallFailedBody', { name: serverName }));
    }
  };

  // Check if any filters are active
  const hasActiveFilters = Object.values(activeFilters).some(v => v && v !== 'all');

  return (
    <div className="h-full flex flex-col" data-testid="registry-page">
      <ToastContainer toasts={toasts} onClose={dismiss} />
      {/* Header */}
      <div className="p-6 border-b border-[rgb(var(--border-subtle))]">
        <div className="flex items-center justify-between gap-3 mb-1">
          <div className="flex items-center gap-3">
            <h1 className="text-2xl font-bold" data-testid="registry-title">{t('title')}</h1>
            {isOffline && (
              <span
                className="px-2 py-0.5 text-xs font-medium bg-amber-500/20 text-amber-600 dark:text-amber-400 rounded-full"
                data-testid="registry-offline-badge"
              >
                {t('offline')}
              </span>
            )}
          </div>
          {/* Always-reachable contribute menu — users don't have to trigger
              an empty search to find the request / bug / feature links. */}
          <ContributeMenu variant="ghost" size="sm" />
        </div>
        <p className="text-sm text-[rgb(var(--muted))]">
          {isOffline ? t('subtitleOffline') : t('subtitleOnline')}
        </p>
      </div>

      {/* Search and Filters */}
      <div className="p-4 border-b border-[rgb(var(--border-subtle))] space-y-4">
        {/* Search */}
        <div className="relative">
          <svg
            className="absolute left-3 top-1/2 -translate-y-1/2 w-5 h-5 text-[rgb(var(--muted))]"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
            />
          </svg>
          <input
            type="text"
            placeholder={t('searchPlaceholder')}
            value={localSearch}
            onChange={(e) => setLocalSearch(e.target.value)}
            className="input w-full pl-10"
            data-testid="search-input"
          />
        </div>

        {/* Filter Bar */}
        <div className="flex flex-wrap items-center gap-3">
          {/* Filter Dropdowns */}
          {uiConfig?.filters.map((filter) => (
            <FilterDropdown
              key={filter.id}
              filter={filter}
              value={activeFilters[filter.id] ?? 'all'}
              onChange={(optionId) => setFilter(filter.id, optionId)}
            />
          ))}

          {/* Sort Dropdown */}
          {uiConfig && uiConfig.sort_options.length > 0 && (
            <div className="ml-auto flex items-center gap-2">
              <span className="text-sm text-[rgb(var(--muted))]">{t('sort')}</span>
              <select
                value={activeSort}
                onChange={(e) => setSort(e.target.value)}
                className="bg-[rgb(var(--surface-hover))] border border-[rgb(var(--border-subtle))] rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/50"
                data-testid="registry-sort-select"
              >
                {uiConfig.sort_options.map((opt) => (
                  <option key={opt.id} value={opt.id}>
                    {opt.label}
                  </option>
                ))}
              </select>
            </div>
          )}

          {/* Clear Filters */}
          {hasActiveFilters && (
            <button
              onClick={clearFilters}
              className="text-sm text-[rgb(var(--primary))] hover:underline"
              data-testid="registry-clear-filters"
            >
              {t('clearFilters')}
            </button>
          )}
        </div>
      </div>

      {/* Error */}
      {error && (
        <div className="mx-4 mt-4 p-4 bg-[rgb(var(--error))]/10 border border-[rgb(var(--error))]/30 rounded-lg text-[rgb(var(--error))] text-sm flex items-center justify-between">
          <span>{error}</span>
          <button onClick={clearError} className="hover:opacity-70">
            ✕
          </button>
        </div>
      )}

      {/* Server Grid */}
      <div className="flex-1 overflow-y-auto p-4 registry-grid-container">
        {isLoading && displayServers.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <div className="animate-spin rounded-full h-8 w-8 border-2 border-[rgb(var(--primary))] border-t-transparent" />
          </div>
        ) : displayServers.length === 0 ? (
          <div
            className="flex flex-col items-center justify-center h-full text-[rgb(var(--muted))] px-8"
            data-testid="registry-empty-state"
          >
            <svg className="w-16 h-16 mb-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={1}
                d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
            <p className="text-lg">{t('empty.title')}</p>
            <p className="text-sm mb-6">{t('empty.desc')}</p>
            {/* Empty-search CTA — push the user toward requesting or
                contributing the missing server rather than just giving up. */}
            <div className="w-full max-w-xl">
              <RequestServerCTA searchTerm={searchQuery || undefined} />
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4" data-testid="registry-server-grid">
            {paginatedServers.map((server) => (
              <ServerCard
                key={server.id}
                server={server}
                onInstall={handleInstall}
                onUninstall={handleUninstall}
                onViewDetails={selectServer}
                isLoading={isLoading}
              />
            ))}
          </div>
        )}
      </div>

      {/* Footer: Stats & Pagination */}
      <div className="p-4 border-t border-[rgb(var(--border-subtle))] flex items-center justify-between bg-[rgb(var(--surface))]">
        <div className="text-sm text-[rgb(var(--muted))]" data-testid="server-count">
          {t('footer.found', { count: displayServers.length })}
          {servers.filter((s) => s.is_installed).length > 0 && (
            <span className="ml-2 border-l border-[rgb(var(--border-subtle))] pl-2">
              {t('footer.installed', { count: servers.filter((s) => s.is_installed).length })}
            </span>
          )}
        </div>

        {totalPages > 1 && (
          <div className="flex items-center gap-2">
            <button
              onClick={() => handlePageChange(activePage - 1)}
              disabled={activePage === 1}
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] disabled:opacity-30 disabled:hover:bg-transparent transition-colors"
              data-testid="registry-pagination-prev"
            >
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M15 18l-6-6 6-6" />
              </svg>
            </button>
            <span
              className="text-sm font-medium min-w-[3rem] text-center"
              data-testid="registry-pagination-info"
            >
              {activePage} / {totalPages}
            </span>
            <button
              onClick={() => handlePageChange(activePage + 1)}
              disabled={activePage === totalPages}
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] disabled:opacity-30 disabled:hover:bg-transparent transition-colors"
              data-testid="registry-pagination-next"
            >
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M9 18l6-6-6-6" />
              </svg>
            </button>
          </div>
        )}
      </div>

      {/* Detail Modal */}
      {selectedServer && (
        <ServerDetailModal
          server={selectedServer}
          onClose={() => selectServer(null)}
          onInstall={handleInstall}
          onUninstall={handleUninstall}
          isLoading={isLoading}
        />
      )}
    </div>
  );
}

// ============================================
// Filter Dropdown Component
// ============================================

import type { FilterDefinition } from '../../types/registry';

interface FilterDropdownProps {
  filter: FilterDefinition;
  value: string;
  onChange: (optionId: string) => void;
}

function FilterDropdown({ filter, value, onChange }: FilterDropdownProps) {
  const isActive = value && value !== 'all';

  return (
    <div className="relative">
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className={`appearance-none bg-[rgb(var(--surface-hover))] border rounded-lg pl-3 pr-8 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/50 cursor-pointer ${
          isActive
            ? 'border-[rgb(var(--primary))] text-[rgb(var(--foreground))]'
            : 'border-[rgb(var(--border-subtle))] text-[rgb(var(--muted))]'
        }`}
      >
        {filter.options.map((opt) => (
          <option key={opt.id} value={opt.id}>
            {opt.icon ? `${opt.icon} ${opt.label}` : opt.label}
          </option>
        ))}
      </select>
      <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 w-4 h-4 pointer-events-none text-[rgb(var(--muted))]" />
    </div>
  );
}
