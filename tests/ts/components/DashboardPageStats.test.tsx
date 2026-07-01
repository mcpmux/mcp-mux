/**
 * Dashboard stat tiles must reflect changes made anywhere — including a
 * FeatureSet composed by an MCP client via `mcpmux_manage_feature_set`, which
 * arrives as a `feature-set-changed` domain event.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, act, waitFor } from '@testing-library/react';

const { handlers, mockListClients, mockListFS, mockGatewayStatus, mockListInstalled, mockListBindings, mockServerStatuses } =
  vi.hoisted(() => ({
    handlers: new Map<string, ((e: { payload: unknown }) => void)[]>(),
    mockListClients: vi.fn(),
    mockListFS: vi.fn(),
    mockGatewayStatus: vi.fn(),
    mockListInstalled: vi.fn(),
    mockListBindings: vi.fn(),
    mockServerStatuses: vi.fn(),
  }));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, cb: (e: { payload: unknown }) => void) => {
    const arr = handlers.get(name) ?? [];
    arr.push(cb);
    handlers.set(name, arr);
    return Promise.resolve(() => {});
  }),
}));
vi.mock('@/lib/api/clients', () => ({ listClients: mockListClients }));
vi.mock('@/lib/api/featureSets', () => ({
  listFeatureSetsBySpace: mockListFS,
  listFeatureSets: mockListFS,
}));
vi.mock('@/lib/api/gateway', () => ({ getGatewayStatus: mockGatewayStatus }));
vi.mock('@/lib/api/registry', () => ({ listInstalledServers: mockListInstalled }));
vi.mock('@/lib/api/workspaceBindings', () => ({ listWorkspaceBindings: mockListBindings }));
vi.mock('@/lib/api/serverManager', () => ({ getServerStatuses: mockServerStatuses }));
vi.mock('@/stores', () => ({
  useViewSpace: () => ({ id: 'space-1', name: 'My Space' }),
  useSetPendingWorkspaceNew: () => () => {},
  useSpaces: () => [{ id: 'space-1' }],
  useIsLoading: () => false,
}));
vi.mock('@/hooks/use-navigate.hook', () => ({ useNavigate: () => () => {} }));
vi.mock('@/components/ConnectionCard', () => ({ ConnectionCard: () => null }));
vi.mock('@/hooks/useMetaToolEvents', () => ({ useMetaToolEventListener: () => {} }));

import { DashboardPage } from '@/features/dashboard/DashboardPage';

function emit(channel: string, payload: unknown) {
  act(() => {
    handlers.get(channel)?.forEach((cb) => cb({ payload }));
  });
}

describe('DashboardPage stats', () => {
  beforeEach(() => {
    handlers.clear();
    mockListClients.mockReset().mockResolvedValue([]);
    mockGatewayStatus.mockReset().mockResolvedValue({ connected_backends: 0 });
    mockListInstalled.mockReset().mockResolvedValue([{}]);
    mockListBindings.mockReset().mockResolvedValue([]);
    mockServerStatuses.mockReset().mockResolvedValue([]);
    mockListFS.mockReset();
  });

  it('refreshes the FeatureSets count when a feature-set-changed event arrives', async () => {
    mockListFS.mockResolvedValueOnce([{}]).mockResolvedValue([{}, {}]);

    render(<DashboardPage />);
    await waitFor(() =>
      expect(screen.getByTestId('stat-featuresets-value')).toHaveTextContent('1')
    );

    emit('feature-set-changed', { action: 'created' });

    await waitFor(() =>
      expect(screen.getByTestId('stat-featuresets-value')).toHaveTextContent('2')
    );
  });
});
