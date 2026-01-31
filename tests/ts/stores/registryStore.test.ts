import { describe, it, expect, beforeEach, vi } from 'vitest';
import { useRegistryStore } from '../../../apps/desktop/src/stores/registryStore';

// Mock the API module
vi.mock('../../../apps/desktop/src/lib/api/registry', () => ({
  discoverServers: vi.fn(),
  getRegistryUiConfig: vi.fn(),
  getRegistryHomeConfig: vi.fn(),
  listInstalledServers: vi.fn(),
  isRegistryOffline: vi.fn(),
  installServer: vi.fn(),
  setServerEnabled: vi.fn(),
  uninstallServer: vi.fn(),
}));

import * as api from '../../../apps/desktop/src/lib/api/registry';

// Helper to create test servers
function createTestServer(id: string, overrides: Record<string, unknown> = {}) {
  return {
    id,
    name: `Server ${id}`,
    description: `Description for ${id}`,
    categories: ['test'],
    transport: { type: 'stdio' as const, metadata: {} },
    source: { type: 'Registry' as const, bundle_id: 'test-bundle' },
    is_installed: false,
    enabled: false,
    oauth_connected: false,
    input_values: {},
    connection_status: 'disconnected' as const,
    missing_required_inputs: false,
    last_error: null,
    ...overrides,
  };
}

function createTestUiConfig() {
  return {
    filters: [
      {
        id: 'category',
        label: 'Category',
        options: [
          { id: 'all', label: 'All' },
          { id: 'ai', label: 'AI', match: { field: 'categories', operator: 'contains' as const, value: 'ai' } },
        ],
      },
    ],
    sort_options: [
      {
        id: 'recommended',
        label: 'Recommended',
        rules: [{ field: '_featured', direction: 'desc' as const }],
      },
      {
        id: 'name',
        label: 'Name',
        rules: [{ field: 'name', direction: 'asc' as const }],
      },
    ],
    default_sort: 'recommended',
  };
}

describe('registryStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset store state
    useRegistryStore.setState({
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
    });
  });

  describe('initial state', () => {
    it('should have correct initial values', () => {
      const state = useRegistryStore.getState();

      expect(state.servers).toEqual([]);
      expect(state.displayServers).toEqual([]);
      expect(state.uiConfig).toBeNull();
      expect(state.activeFilters).toEqual({});
      expect(state.activeSort).toBe('recommended');
      expect(state.searchQuery).toBe('');
      expect(state.isLoading).toBe(false);
      expect(state.error).toBeNull();
    });
  });

  describe('setFilter', () => {
    it('should set a filter value', () => {
      const servers = [createTestServer('1'), createTestServer('2')];
      useRegistryStore.setState({
        servers,
        displayServers: servers,
        uiConfig: createTestUiConfig(),
      });

      useRegistryStore.getState().setFilter('category', 'ai');

      expect(useRegistryStore.getState().activeFilters).toEqual({ category: 'ai' });
    });

    it('should clear search query when filtering', () => {
      useRegistryStore.setState({
        searchQuery: 'test query',
        servers: [createTestServer('1')],
        uiConfig: createTestUiConfig(),
      });

      useRegistryStore.getState().setFilter('category', 'ai');

      expect(useRegistryStore.getState().searchQuery).toBe('');
    });
  });

  describe('setSort', () => {
    it('should set sort option', () => {
      const servers = [createTestServer('b'), createTestServer('a')];
      useRegistryStore.setState({
        servers,
        displayServers: servers,
        uiConfig: createTestUiConfig(),
      });

      useRegistryStore.getState().setSort('name');

      expect(useRegistryStore.getState().activeSort).toBe('name');
    });
  });

  describe('search', () => {
    it('should set search query', () => {
      useRegistryStore.setState({
        servers: [createTestServer('1')],
        displayServers: [createTestServer('1')],
        uiConfig: createTestUiConfig(),
      });

      useRegistryStore.getState().search('test');

      expect(useRegistryStore.getState().searchQuery).toBe('test');
    });

    it('should filter servers by name', () => {
      const servers = [
        createTestServer('1', { name: 'Alpha Server' }),
        createTestServer('2', { name: 'Beta Server' }),
        createTestServer('3', { name: 'Gamma Alpha' }),
      ];
      useRegistryStore.setState({
        servers,
        displayServers: servers,
        uiConfig: createTestUiConfig(),
      });

      useRegistryStore.getState().search('alpha');

      const results = useRegistryStore.getState().displayServers;
      expect(results).toHaveLength(2);
      expect(results.map((s) => s.id)).toContain('1');
      expect(results.map((s) => s.id)).toContain('3');
    });

    it('should filter servers by description', () => {
      const servers = [
        createTestServer('1', { description: 'A unique description' }),
        createTestServer('2', { description: 'Another one' }),
      ];
      useRegistryStore.setState({
        servers,
        displayServers: servers,
        uiConfig: createTestUiConfig(),
      });

      useRegistryStore.getState().search('unique');

      expect(useRegistryStore.getState().displayServers).toHaveLength(1);
      expect(useRegistryStore.getState().displayServers[0].id).toBe('1');
    });

    it('should filter servers by category', () => {
      const servers = [
        createTestServer('1', { categories: ['ai', 'ml'] }),
        createTestServer('2', { categories: ['database'] }),
      ];
      useRegistryStore.setState({
        servers,
        displayServers: servers,
        uiConfig: createTestUiConfig(),
      });

      useRegistryStore.getState().search('ai');

      expect(useRegistryStore.getState().displayServers).toHaveLength(1);
      expect(useRegistryStore.getState().displayServers[0].id).toBe('1');
    });
  });

  describe('clearFilters', () => {
    it('should reset filters and search', () => {
      useRegistryStore.setState({
        activeFilters: { category: 'ai' },
        searchQuery: 'test',
        servers: [createTestServer('1')],
        uiConfig: createTestUiConfig(),
      });

      useRegistryStore.getState().clearFilters();

      expect(useRegistryStore.getState().activeFilters).toEqual({});
      expect(useRegistryStore.getState().searchQuery).toBe('');
    });
  });

  describe('installServer', () => {
    it('should call API and update local state', async () => {
      const server = createTestServer('1');
      vi.mocked(api.installServer).mockResolvedValue(undefined);

      useRegistryStore.setState({
        servers: [server],
        displayServers: [server],
        spaceId: 'space-1',
      });

      await useRegistryStore.getState().installServer('1');

      expect(api.installServer).toHaveBeenCalledWith('1', 'space-1');
      expect(useRegistryStore.getState().servers[0].is_installed).toBe(true);
      expect(useRegistryStore.getState().displayServers[0].is_installed).toBe(true);
    });

    it('should update selectedServer if matches', async () => {
      const server = createTestServer('1');
      vi.mocked(api.installServer).mockResolvedValue(undefined);

      useRegistryStore.setState({
        servers: [server],
        displayServers: [server],
        selectedServer: server,
        spaceId: 'space-1',
      });

      await useRegistryStore.getState().installServer('1');

      expect(useRegistryStore.getState().selectedServer?.is_installed).toBe(true);
    });

    it('should not call API without spaceId', async () => {
      useRegistryStore.setState({
        servers: [createTestServer('1')],
        spaceId: null,
      });

      await useRegistryStore.getState().installServer('1');

      expect(api.installServer).not.toHaveBeenCalled();
    });
  });

  describe('uninstallServer', () => {
    it('should call API and update local state', async () => {
      const server = createTestServer('1', { is_installed: true, enabled: true });
      vi.mocked(api.uninstallServer).mockResolvedValue(undefined);

      useRegistryStore.setState({
        servers: [server],
        displayServers: [server],
        spaceId: 'space-1',
      });

      await useRegistryStore.getState().uninstallServer('1');

      expect(api.uninstallServer).toHaveBeenCalledWith('1', 'space-1');
      expect(useRegistryStore.getState().servers[0].is_installed).toBe(false);
      expect(useRegistryStore.getState().servers[0].enabled).toBe(false);
    });
  });

  describe('setSpaceId', () => {
    it('should set the space id', () => {
      useRegistryStore.getState().setSpaceId('space-123');

      expect(useRegistryStore.getState().spaceId).toBe('space-123');
    });
  });

  describe('selectServer', () => {
    it('should set selected server', () => {
      const server = createTestServer('1');

      useRegistryStore.getState().selectServer(server);

      expect(useRegistryStore.getState().selectedServer).toEqual(server);
    });

    it('should clear selected server', () => {
      useRegistryStore.setState({ selectedServer: createTestServer('1') });

      useRegistryStore.getState().selectServer(null);

      expect(useRegistryStore.getState().selectedServer).toBeNull();
    });
  });

  describe('clearError', () => {
    it('should clear error message', () => {
      useRegistryStore.setState({ error: 'Some error' });

      useRegistryStore.getState().clearError();

      expect(useRegistryStore.getState().error).toBeNull();
    });
  });
});
