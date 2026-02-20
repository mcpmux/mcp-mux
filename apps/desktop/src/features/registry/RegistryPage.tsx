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
import { useViewSpace } from '@/stores';
import { capture } from '@/lib/analytics';

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
        if (localSearch.trim()) {
          capture('registry_search', { query: localSearch.trim() });
        }
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [localSearch, searchQuery, search]);

  const handleInstall = async (id: string) => {
    const server = servers.find(s => s.id === id);
    const serverName = server?.name || 'Server';
    try {
      await installServer(id, viewSpace?.id);
      success('Server installed', `"${serverName}" has been installed`);
    } catch {
      showToastError('Install failed', `Failed to install "${serverName}"`);
    }
  };

  const handleUninstall = async (id: string) => {
    const server = servers.find(s => s.id === id);
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
  const hasActiveFilters = Object.values(activeFilters).some(v => v && v !== 'all');

  return (
    <div className="h-full flex flex-col" data-testid="registry-page">
      <ToastContainer toasts={toasts} onClose={dismiss} />
      {/* Header */}
      <div className="p-6 border-b border-[rgb(var(--border-subtle))]">
        <div className="flex items-center gap-3 mb-1">
          <h1 className="text-2xl font-bold" data-testid="registry-title">Discover Servers</h1>
          {isOffline && (
            <span className="px-2 py-0.5 text-xs font-medium bg-amber-500/20 text-amber-600 dark:text-amber-400 rounded-full">
              Offline
            </span>
          )}
        </div>
        <p className="text-sm text-[rgb(var(--muted))]">
          {isOffline 
            ? 'Showing cached servers (no internet connection)'
            : 'Browse and install MCP servers from the registry'
          }
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
                className="bg-[rgb(var(--surface-hover))] border border-[rgb(var(--border-subtle))] rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/50"
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
          <div className="flex flex-col items-center justify-center h-full text-[rgb(var(--muted))]">
            <svg className="w-16 h-16 mb-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={1}
                d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
            <p className="text-lg">No servers found</p>
            <p className="text-sm">Try adjusting your search or filters</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
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
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] disabled:opacity-30 disabled:hover:bg-transparent transition-colors"
            >
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M15 18l-6-6 6-6" />
              </svg>
            </button>
            <span className="text-sm font-medium min-w-[3rem] text-center">
              {activePage} / {totalPages}
            </span>
            <button
              onClick={() => handlePageChange(activePage + 1)}
              disabled={activePage === totalPages}
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] disabled:opacity-30 disabled:hover:bg-transparent transition-colors"
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
