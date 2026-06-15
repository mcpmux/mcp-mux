/**
 * Dashboard stat tiles must reflect changes made anywhere — including a
 * FeatureSet composed by an MCP client via `mcpmux_manage_feature_set`, which
 * arrives as a `feature-set-changed` domain event. Guards the fix for
 * "created a FeatureSet via the tool but the count didn't update live".
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, act, waitFor } from '@testing-library/react';

const { handlers, mockListClients, mockListFS, mockGatewayStatus, mockListInstalled } = vi.hoisted(
  () => ({
    handlers: new Map<string, ((e: { payload: unknown }) => void)[]>(),
    mockListClients: vi.fn(),
    mockListFS: vi.fn(),
    mockGatewayStatus: vi.fn(),
    mockListInstalled: vi.fn(),
  })
);

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
vi.mock('@/stores', () => ({
  useViewSpace: () => ({ id: 'space-1', name: 'My Space' }),
  useNavigateTo: () => () => {},
}));
vi.mock('@/components/ConnectionCard', () => ({ ConnectionCard: () => null }));

import { HomePage } from '@/features/home/HomePage';

function emit(channel: string, payload: unknown) {
  act(() => {
    handlers.get(channel)?.forEach((cb) => cb({ payload }));
  });
}

describe('HomePage dashboard stats', () => {
  beforeEach(() => {
    handlers.clear();
    mockListClients.mockReset().mockResolvedValue([]);
    mockGatewayStatus.mockReset().mockResolvedValue({ connected_backends: 0 });
    mockListInstalled.mockReset().mockResolvedValue([{}]); // 1 server → skip onboarding strip
    mockListFS.mockReset();
  });

  it('refreshes the FeatureSets count when a feature-set-changed event arrives', async () => {
    // First load → 1 FeatureSet; after the event → 2.
    mockListFS.mockResolvedValueOnce([{}]).mockResolvedValue([{}, {}]);

    render(<HomePage />);
    await waitFor(() =>
      expect(screen.getByTestId('stat-featuresets-value')).toHaveTextContent('1')
    );

    emit('feature-set-changed', { action: 'created' });

    await waitFor(() =>
      expect(screen.getByTestId('stat-featuresets-value')).toHaveTextContent('2')
    );
  });
});
