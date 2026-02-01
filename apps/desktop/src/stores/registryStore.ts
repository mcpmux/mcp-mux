/**
 * Registry store for managing MCP server browsing and discovery.
 * 
 * Uses bundle-only strategy with API-driven filters (see ADR-001).
 * All filtering, sorting, and searching is done client-side.
 */

import { create } from 'zustand';
import type { 
  ServerViewModel, 
  ServerDefinition, 
  InstalledServerState,
  UiConfig,
  HomeConfig,
  FilterMatch,
  SortOption,
} from '../types/registry';
import * as api from '../lib/api/registry';

// ============================================
// State & Actions Types
// ============================================

interface RegistryState {
  /** All servers (merged view) */
  servers: ServerViewModel[];
  /** Filtered/sorted servers for display */
  displayServers: ServerViewModel[];
  /** UI configuration from bundle (filters, sort options) */
  uiConfig: UiConfig | null;
  /** Home configuration from bundle (featured IDs) */
  homeConfig: HomeConfig | null;
  /** Active filter values keyed by filter ID */
  activeFilters: Record<string, string>;
  /** Active sort option ID */
  activeSort: string;
  /** Current search query */
  searchQuery: string;
  /** Active view space for install status */
  spaceId: string | null;
  /** Loading state */
  isLoading: boolean;
  /** Error message */
  error: string | null;
  /** Currently selected server for detail view */
  selectedServer: ServerViewModel | null;
  /** Whether running in offline mode (using disk cache) */
  isOffline: boolean;
}

interface RegistryActions {
  /** Load all servers and UI config */
  loadRegistry: (spaceId?: string) => Promise<void>;
  /** Set a filter value */
  setFilter: (filterId: string, optionId: string) => void;
  /** Set sort option */
  setSort: (sortId: string) => void;
  /** Search servers (client-side) */
  search: (query: string) => void;
  /** Clear all filters */
  clearFilters: () => void;
  /** Install server (create DB record) */
  installServer: (id: string, spaceId?: string) => Promise<void>;
  /** Enable/Disable server */
  toggleServer: (id: string, enabled: boolean) => Promise<void>;
  /** Uninstall server */
  uninstallServer: (id: string) => Promise<void>;
  /** Set active view space */
  setSpaceId: (spaceId: string | null) => void;
  /** Select a server for detail view */
  selectServer: (server: ServerViewModel | null) => void;
  /** Clear error */
  clearError: () => void;
}

// ============================================
// Store Implementation
// ============================================

export const useRegistryStore = create<RegistryState & RegistryActions>((set, get) => ({
  // State
  servers: [],
  displayServers: [],
  uiConfig: null,
  homeConfig: null,
  activeFilters: {},
  activeSort: 'recommended',
  searchQuery: '',
  spaceId: null,
  isLoading: false,
  error: null,
  selectedServer: null,
  isOffline: false,

  // Actions
  loadRegistry: async (spaceId) => {
    const currentSpaceId = spaceId ?? get().spaceId;
    if (!currentSpaceId) {
      console.warn("loadRegistry called without spaceId");
    }

    set({ isLoading: true, error: null, spaceId: currentSpaceId });
    try {
      const [definitions, uiConfig, homeConfig, installedStates, isOffline] = await Promise.all([
        api.discoverServers(),
        api.getRegistryUiConfig(),
        api.getRegistryHomeConfig(),
        currentSpaceId ? api.listInstalledServers(currentSpaceId) : Promise.resolve([]),
        api.isRegistryOffline()
      ]);
      
      const mergedServers = mergeServers(definitions, installedStates);
      const registryServers = mergedServers.filter(s => s.source.type !== 'UserSpace');
      
      // Mark featured servers
      const featuredIds = new Set(homeConfig?.featured_server_ids ?? []);
      const serversWithFeatured = registryServers.map(s => ({
        ...s,
        _featured: featuredIds.has(s.id)
      }));

      set({ 
        servers: serversWithFeatured, 
        uiConfig,
        homeConfig,
        activeSort: uiConfig?.default_sort ?? 'recommended',
        isLoading: false,
        isOffline
      });
      
      // Apply current filters/sort/search
      applyFiltersAndSort(get, set);
    } catch (error) {
      set({ error: String(error), isLoading: false });
    }
  },

  setFilter: (filterId: string, optionId: string) => {
    const { activeFilters } = get();
    set({ 
      activeFilters: { ...activeFilters, [filterId]: optionId },
      searchQuery: '' // Clear search when filtering
    });
    applyFiltersAndSort(get, set);
  },

  setSort: (sortId: string) => {
    set({ activeSort: sortId });
    applyFiltersAndSort(get, set);
  },

  search: (query: string) => {
    set({ searchQuery: query });
    applyFiltersAndSort(get, set);
  },

  clearFilters: () => {
    set({ activeFilters: {}, searchQuery: '' });
    applyFiltersAndSort(get, set);
  },

  installServer: async (id: string, spaceId?: string) => {
    const targetSpaceId = spaceId || get().spaceId;
    if (!targetSpaceId) return;

    try {
      await api.installServer(id, targetSpaceId);
      
      // Update state locally without reloading
      const { servers, displayServers, selectedServer } = get();
      const updatedServers = servers.map(s => 
        s.id === id ? { ...s, is_installed: true } : s
      );
      const updatedDisplayServers = displayServers.map(s => 
        s.id === id ? { ...s, is_installed: true } : s
      );
      const updatedSelectedServer = selectedServer?.id === id 
        ? { ...selectedServer, is_installed: true }
        : selectedServer;
      
      set({ 
        servers: updatedServers, 
        displayServers: updatedDisplayServers,
        selectedServer: updatedSelectedServer
      });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  toggleServer: async (id: string, enabled: boolean) => {
    const { spaceId } = get();
    if (!spaceId) return;

    set({ isLoading: true, error: null });
    try {
      await api.setServerEnabled(id, enabled, spaceId);
      await get().loadRegistry(spaceId);
    } catch (error) {
      set({ error: String(error), isLoading: false });
    }
  },

  uninstallServer: async (id: string) => {
    const { spaceId } = get();
    if (!spaceId) return;

    try {
      await api.uninstallServer(id, spaceId);
      
      // Update state locally without reloading
      const { servers, displayServers, selectedServer } = get();
      const updatedServers = servers.map(s => 
        s.id === id ? { ...s, is_installed: false, enabled: false } : s
      );
      const updatedDisplayServers = displayServers.map(s => 
        s.id === id ? { ...s, is_installed: false, enabled: false } : s
      );
      const updatedSelectedServer = selectedServer?.id === id 
        ? { ...selectedServer, is_installed: false, enabled: false }
        : selectedServer;
      
      set({ 
        servers: updatedServers, 
        displayServers: updatedDisplayServers,
        selectedServer: updatedSelectedServer
      });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  setSpaceId: (spaceId) => {
    set({ spaceId });
  },

  selectServer: (server) => {
    set({ selectedServer: server });
  },

  clearError: () => {
    set({ error: null });
  },
}));

// ============================================
// Helper Functions
// ============================================

type GetState = () => RegistryState & RegistryActions;
type SetState = (partial: Partial<RegistryState>) => void;

/** Apply all filters, search, and sorting to produce displayServers */
function applyFiltersAndSort(get: GetState, set: SetState) {
  const { servers, uiConfig, activeFilters, activeSort, searchQuery } = get();
  
  let result = [...servers];
  
  // 1. Apply search
  if (searchQuery) {
    const q = searchQuery.toLowerCase();
    result = result.filter(s => 
      s.name.toLowerCase().includes(q) ||
      s.description?.toLowerCase().includes(q) ||
      s.id.toLowerCase().includes(q) ||
      s.alias?.toLowerCase().includes(q) ||
      s.categories.some(c => c.toLowerCase().includes(q))
    );
  }
  
  // 2. Apply filters
  if (uiConfig) {
    for (const [filterId, optionId] of Object.entries(activeFilters)) {
      if (optionId === 'all') continue;
      
      const filterDef = uiConfig.filters.find(f => f.id === filterId);
      const option = filterDef?.options.find(o => o.id === optionId);
      
      if (option?.match) {
        result = result.filter(server => matchesFilter(server, option.match!));
      }
    }
  }
  
  // 3. Apply sorting
  if (uiConfig) {
    const sortOption = uiConfig.sort_options.find(s => s.id === activeSort);
    if (sortOption) {
      result = applySorting(result, sortOption);
    }
  }
  
  set({ displayServers: result });
}

/** Check if a server matches a filter rule */
function matchesFilter(server: ServerViewModel, match: FilterMatch): boolean {
  const value = getNestedValue(server, match.field);
  
  switch (match.operator) {
    case 'eq':
      return value === match.value;
    case 'in':
      return Array.isArray(match.value) && (match.value as unknown[]).includes(value);
    case 'contains':
      return Array.isArray(value) && (value as unknown[]).includes(match.value);
    default:
      return true;
  }
}

/** Get a nested value from an object using dot notation */
function getNestedValue<T extends object>(obj: T, path: string): unknown {
  return path.split('.').reduce((o: unknown, k) => {
    if (o && typeof o === 'object' && k in o) {
      return (o as Record<string, unknown>)[k];
    }
    return undefined;
  }, obj as unknown);
}

/** Apply sorting rules to servers */
function applySorting(servers: ServerViewModel[], sortOption: SortOption): ServerViewModel[] {
  return [...servers].sort((a, b) => {
    for (const rule of sortOption.rules) {
      const aVal = getNestedValue(a, rule.field);
      const bVal = getNestedValue(b, rule.field);
      
      // Handle nulls/undefined
      if (aVal == null && bVal == null) continue;
      if (aVal == null) return rule.nulls === 'first' ? -1 : 1;
      if (bVal == null) return rule.nulls === 'first' ? 1 : -1;
      
      // Compare
      let cmp = 0;
      if (typeof aVal === 'boolean' && typeof bVal === 'boolean') {
        cmp = aVal === bVal ? 0 : (aVal ? -1 : 1);
      } else if (typeof aVal === 'string' && typeof bVal === 'string') {
        cmp = aVal.localeCompare(bVal);
      } else if (typeof aVal === 'number' && typeof bVal === 'number') {
        cmp = aVal - bVal;
      }
      
      if (cmp !== 0) {
        return rule.direction === 'desc' ? -cmp : cmp;
      }
    }
    return 0;
  });
}

/** Merge server definitions with installed state */
function mergeServers(defs: ServerDefinition[], states: InstalledServerState[]): ServerViewModel[] {
  const stateMap = new Map(states.map(s => [s.server_id, s]));
  
  return defs.map(def => {
    const state = stateMap.get(def.id);
    
    // Check if any required inputs are missing
    const inputs = def.transport.metadata?.inputs ?? [];
    const inputValues = state?.input_values ?? {};
    const missing_required_inputs = inputs.some((input) => 
      input.required && !inputValues[input.id]
    );
    
    // Calculate initial connection_status based on enabled state
    const connection_status = state?.enabled ? 'connecting' : 'disconnected';
    
    return {
      ...def,
      is_installed: !!state,
      enabled: state?.enabled ?? false,
      oauth_connected: state?.oauth_connected ?? false,
      input_values: inputValues,
      connection_status,
      missing_required_inputs,
      last_error: null
    } as ServerViewModel;
  });
}
