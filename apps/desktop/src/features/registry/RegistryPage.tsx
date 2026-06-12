/**
 * Registry page for browsing and installing MCP servers.
 *
 * Uses API-driven filters and client-side sorting (see ADR-001).
 */

import { useEffect, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { useToast, ToastContainer } from '@mcpmux/ui';
import { useRegistryStore } from '../../stores/registryStore';
import { ServerCard } from './ServerCard';
import { ServerDetailModal } from './ServerDetailModal';
import { useViewSpace, useNavigateTo } from '@/stores';
import { capture } from '@/lib/analytics';
import { RequestServerCTA, ContributeMenu } from '@/components/Contribute';

export function RegistryPage() {
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
    length: displayServers.length,
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

  // Track search analytics with longer debounce to capture final query only
  useEffect(() => {
    if (!localSearch.trim()) return;
    const timer = setTimeout(() => {
      capture('registry_search', { query: localSearch.trim() });
    }, 1500);
    return () => clearTimeout(timer);
  }, [localSearch]);

  const handleInstall = async (id: string) => {
    const server = servers.find((s) => s.id === id);
    const serverName = server?.name || 'Server';
    try {
      await installServer(id, viewSpace?.id);
      success('Server installed', `"${serverName}" has been installed`, {
        duration: 6000,
        action: {
          label: 'Go to Tools to enable →',
          onClick: () => navigateTo('servers'),
        },
      });
    } catch {
      showToastError('Install failed', `Failed to install "${serverName}"`);
    }
  };

  const handleUninstall = async (id: string) => {
    const server = servers.find((s) => s.id === id);
    const serverName = server?.name || 'Server';
    try {
      await uninstallServer(id);
      if (selectedServer?.id === id) {
        selectServer(null);
      }
      success('Server uninstalled', `"${serverName}" has been uninstalled`);
    } catch {
      showToastError('Uninstall failed', `Failed to uninstall "${serverName}"`);
    }
  };

  // Check if any filters are active
  const hasActiveFilters = Object.values(activeFilters).some((v) => v && v !== 'all');

  return (
    <div className="flex h-full flex-col" data-testid="registry-page">
      <ToastContainer toasts={toasts} onClose={dismiss} />
      {/* Header */}
      <div className="border-b border-[rgb(var(--border-subtle))] p-6">
        <div className="mb-1 flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <h1 className="text-2xl font-bold tracking-tight" data-testid="registry-title">
              Discover
            </h1>
            {isOffline && (
              <span className="rounded-full bg-amber-500/20 px-2 py-0.5 text-xs font-medium text-amber-600 dark:text-amber-400">
                Offline
              </span>
            )}
          </div>
          {/* Always-reachable contribute menu — users don't have to trigger
              an empty search to find the request / bug / feature links. */}
          <ContributeMenu variant="ghost" size="sm" />
        </div>
        <p className="text-sm text-[rgb(var(--muted))]">
          {isOffline
            ? 'Showing cached servers (no internet connection)'
            : 'Browse the registry and add new tools to this Space in one click'}
        </p>
      </div>

      {/* Search and Filters */}
      <div className="space-y-4 border-b border-[rgb(var(--border-subtle))] p-4">
        {/* Search */}
        <div className="relative">
          <svg
            className="absolute left-3 top-1/2 h-5 w-5 -translate-y-1/2 text-[rgb(var(--muted))]"
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
            placeholder="Search servers..."
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
              <span className="text-sm text-[rgb(var(--muted))]">Sort:</span>
              <select
                value={activeSort}
                onChange={(e) => setSort(e.target.value)}
                className="rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface-hover))] px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/50"
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
            >
              Clear filters
            </button>
          )}
        </div>
      </div>

      {/* Error */}
      {error && (
        <div className="mx-4 mt-4 flex items-center justify-between rounded-lg border border-[rgb(var(--error))]/30 bg-[rgb(var(--error))]/10 p-4 text-sm text-[rgb(var(--error))]">
          <span>{error}</span>
          <button onClick={clearError} className="hover:opacity-70">
            ✕
          </button>
        </div>
      )}

      {/* Server Grid */}
      <div className="registry-grid-container flex-1 overflow-y-auto p-4">
        {isLoading && displayServers.length === 0 ? (
          <div className="flex h-full items-center justify-center">
            <div className="h-8 w-8 animate-spin rounded-full border-2 border-[rgb(var(--primary))] border-t-transparent" />
          </div>
        ) : displayServers.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center px-8 text-[rgb(var(--muted))]">
            <svg className="mb-4 h-16 w-16" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={1}
                d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
            <p className="text-lg">No servers found</p>
            <p className="mb-6 text-sm">Try adjusting your search or filters</p>
            {/* Empty-search CTA — push the user toward requesting or
                contributing the missing server rather than just giving up. */}
            <div className="w-full max-w-xl">
              <RequestServerCTA searchTerm={searchQuery || undefined} />
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
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
      <div className="flex items-center justify-between border-t border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] p-4">
        <div className="text-sm text-[rgb(var(--muted))]" data-testid="server-count">
          {displayServers.length} server{displayServers.length !== 1 ? 's' : ''} found
          {servers.filter((s) => s.is_installed).length > 0 && (
            <span className="ml-2 border-l border-[rgb(var(--border-subtle))] pl-2">
              {servers.filter((s) => s.is_installed).length} installed
            </span>
          )}
        </div>

        {totalPages > 1 && (
          <div className="flex items-center gap-2">
            <button
              onClick={() => handlePageChange(activePage - 1)}
              disabled={activePage === 1}
              className="rounded-lg p-1.5 transition-colors hover:bg-[rgb(var(--surface-hover))] disabled:opacity-30 disabled:hover:bg-transparent"
            >
              <svg
                width="20"
                height="20"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M15 18l-6-6 6-6" />
              </svg>
            </button>
            <span className="min-w-[3rem] text-center text-sm font-medium">
              {activePage} / {totalPages}
            </span>
            <button
              onClick={() => handlePageChange(activePage + 1)}
              disabled={activePage === totalPages}
              className="rounded-lg p-1.5 transition-colors hover:bg-[rgb(var(--surface-hover))] disabled:opacity-30 disabled:hover:bg-transparent"
            >
              <svg
                width="20"
                height="20"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
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
        className={`cursor-pointer appearance-none rounded-lg border bg-[rgb(var(--surface-hover))] py-1.5 pl-3 pr-8 text-sm focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/50 ${
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
      <ChevronDown className="pointer-events-none absolute right-2 top-1/2 h-4 w-4 -translate-y-1/2 text-[rgb(var(--muted))]" />
    </div>
  );
}
