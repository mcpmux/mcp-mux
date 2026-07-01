/**
 * Search-analytics capture for the registry page.
 *
 * Guards the design we settled on: PostHog must receive ONE `registry_search`
 * event per *settled* query — never one per keystroke — and that event must
 * carry the result count so zero-result searches (the strongest signal for
 * which servers users want) are distinguishable from hits.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, act, fireEvent } from '@testing-library/react';
import type { ServerViewModel } from '@/types/registry';

const { mockCapture } = vi.hoisted(() => ({ mockCapture: vi.fn() }));

vi.mock('@/lib/analytics', () => ({
  capture: mockCapture,
  initAnalytics: vi.fn(),
  optIn: vi.fn(),
  optOut: vi.fn(),
  hasOptedOut: () => false,
}));

// The real registry store runs against these — resolve empty so mount-time
// loadRegistry settles instantly; we seed servers directly afterwards.
vi.mock('@/lib/api/registry', () => ({
  discoverServers: vi.fn().mockResolvedValue([]),
  getRegistryUiConfig: vi.fn().mockResolvedValue(null),
  getRegistryHomeConfig: vi.fn().mockResolvedValue(null),
  listInstalledServers: vi.fn().mockResolvedValue([]),
  isRegistryOffline: vi.fn().mockResolvedValue(false),
  installServer: vi.fn().mockResolvedValue(undefined),
  uninstallServer: vi.fn().mockResolvedValue(undefined),
  setServerEnabled: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@/stores', () => ({
  useViewSpace: () => ({ id: 'space-1', name: 'My Space' }),
}));
vi.mock('@/hooks/use-navigate.hook', () => ({ useNavigate: () => () => {} }));

// Leaf children irrelevant to search analytics — stub to keep the test focused.
vi.mock('@/features/registry/ServerCard', () => ({ ServerCard: () => null }));
vi.mock('@/features/registry/ServerDetailModal', () => ({ ServerDetailModal: () => null }));
vi.mock('@/components/Contribute', () => ({
  RequestServerCTA: () => null,
  ContributeMenu: () => null,
}));

import { RegistryPage } from '@/features/registry/RegistryPage';
import { useRegistryStore } from '@/stores/registryStore';
import { renderWithI18n } from '../render-with-i18n.helpers';

function makeServer(overrides: Partial<ServerViewModel> = {}): ServerViewModel {
  return {
    id: 'com.test-server',
    name: 'Test Server',
    description: 'A test MCP server',
    alias: 'test',
    icon: null,
    auth: { type: 'none' },
    transport: {
      type: 'http',
      url: 'https://example.com/mcp',
      headers: {},
      metadata: { inputs: [] },
    },
    categories: ['developer-tools'],
    publisher: null,
    source: { type: 'Registry', url: 'https://registry.mcpmux.com', name: 'McpMux Registry' },
    is_installed: false,
    enabled: false,
    oauth_connected: false,
    input_values: {},
    connection_status: 'disconnected',
    missing_required_inputs: false,
    last_error: null,
    ...overrides,
  };
}

const SERVERS: ServerViewModel[] = [
  makeServer({ id: 'io.github.server', name: 'GitHub', description: 'GitHub MCP server' }),
  makeServer({ id: 'com.slack', name: 'Slack', description: 'Slack MCP server' }),
  makeServer({ id: 'com.notion', name: 'Notion', description: 'Notion MCP server' }),
];

/** Render, flush the async mount-time loadRegistry, then seed real servers. */
async function renderSeeded() {
  renderWithI18n(<RegistryPage />);
  // loadRegistry resolves on the microtask queue (api mocks resolve immediately).
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  act(() => {
    useRegistryStore.setState({ servers: SERVERS, displayServers: [] });
  });
}

function typeSearch(value: string) {
  const input = screen.getByTestId('search-input') as HTMLInputElement;
  fireEvent.change(input, { target: { value } });
}

/** Advance past both the 300ms search debounce and the 1200ms analytics debounce. */
function settle() {
  act(() => {
    vi.advanceTimersByTime(300); // search debounce → store updates, count settles
  });
  act(() => {
    vi.advanceTimersByTime(1300); // analytics debounce → capture fires
  });
}

describe('RegistryPage search analytics', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockCapture.mockClear();
    useRegistryStore.setState({ servers: [], displayServers: [], searchQuery: '' });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('does not capture on every keystroke', async () => {
    await renderSeeded();
    typeSearch('g');
    typeSearch('gi');
    typeSearch('git');
    // No debounce elapsed yet — nothing should have been sent.
    expect(mockCapture).not.toHaveBeenCalled();
  });

  it('captures once for a settled query, with the result count', async () => {
    await renderSeeded();
    typeSearch('github');
    settle();

    expect(mockCapture).toHaveBeenCalledTimes(1);
    expect(mockCapture).toHaveBeenCalledWith('registry_search', {
      query: 'github',
      query_length: 6,
      results_count: 1,
      has_results: true,
    });
  });

  it('flags zero-result searches', async () => {
    await renderSeeded();
    typeSearch('zzzznotathing');
    settle();

    expect(mockCapture).toHaveBeenCalledTimes(1);
    expect(mockCapture).toHaveBeenCalledWith('registry_search', {
      query: 'zzzznotathing',
      query_length: 13,
      results_count: 0,
      has_results: false,
    });
  });

  it('coalesces rapid keystrokes into a single event for the final query', async () => {
    await renderSeeded();
    // Each keystroke lands well within the 1.2s analytics window, so the timer
    // keeps resetting and only the final query is ever sent.
    typeSearch('s');
    act(() => vi.advanceTimersByTime(200));
    typeSearch('sl');
    act(() => vi.advanceTimersByTime(200));
    typeSearch('sla');
    act(() => vi.advanceTimersByTime(200));
    typeSearch('slack');
    settle();

    expect(mockCapture).toHaveBeenCalledTimes(1);
    expect(mockCapture).toHaveBeenCalledWith('registry_search', {
      query: 'slack',
      query_length: 5,
      results_count: 1,
      has_results: true,
    });
  });

  it('ignores a whitespace-only query', async () => {
    await renderSeeded();
    typeSearch('   ');
    settle();
    expect(mockCapture).not.toHaveBeenCalled();
  });
});
