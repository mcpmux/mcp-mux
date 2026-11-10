import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const { findPriorCredentialMock, getPriorCredentialValuesMock } = vi.hoisted(() => ({
  findPriorCredentialMock: vi.fn(),
  getPriorCredentialValuesMock: vi.fn(),
}));

vi.mock('@/stores', () => ({
  useViewSpace: () => ({ id: 'new-space', name: 'New Space' }),
  useNavigateTo: () => vi.fn(),
}));

vi.mock('@/hooks/useServerManager', () => ({
  useServerManager: () => ({
    statuses: {},
    authProgress: {},
    enable: vi.fn(),
    disable: vi.fn(),
    connect: vi.fn(),
    cancel: vi.fn(),
    retry: vi.fn(),
  }),
}));
vi.mock('@/features/gateway/useGatewayControl', () => ({ useGatewayControl: () => ({}) }));
vi.mock('@/hooks/useDomainEvents', () => ({
  useGatewayEvents: () => {},
  useDomainEvents: () => ({ subscribe: () => () => {} }),
}));
vi.mock('@/lib/api/credentials', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api/credentials')>()),
  findPriorCredential: findPriorCredentialMock,
  getPriorCredentialValues: getPriorCredentialValuesMock,
}));
vi.mock('@/lib/api/registry', () => ({
  listInstalledServers: async () => [
    {
      server_id: 'example/server',
      server_name: 'Example Server',
      input_values: { TOKEN: '' },
      enabled: true,
      oauth_connected: false,
      source: 'registry',
      env_overrides: {},
      args_append: [],
      extra_headers: {},
    },
  ],
  discoverServers: async () => [
    {
      id: 'example/server',
      name: 'Example Server',
      description: 'Example',
      categories: [],
      source: { type: 'Bundled' },
      transport: {
        type: 'stdio',
        command: 'example',
        args: [],
        env: {},
        metadata: {
          inputs: [
            { id: 'TOKEN', label: 'Token', required: true },
            { id: 'ENDPOINT', label: 'Endpoint', required: true },
          ],
        },
      },
    },
  ],
}));
vi.mock('@/lib/api/gateway', () => ({
  getGatewayStatus: async () => ({ running: false, url: null }),
}));
vi.mock('@/lib/api/serverManager', () => ({ getServerStatuses: async () => ({}) }));
vi.mock('@/lib/api/serverFeatures', () => ({ listServerFeaturesByServer: async () => [] }));
vi.mock('@/components/ServerActionMenu', () => ({ ServerActionMenu: () => null }));
vi.mock('@/components/ServerLogViewer', () => ({ ServerLogViewer: () => null }));
vi.mock('@/components/ConfigEditorModal', () => ({ ConfigEditorModal: () => null }));
vi.mock('@/components/ServerDefinitionModal', () => ({ ServerDefinitionModal: () => null }));
vi.mock('@/components/SourceBadge', () => ({ SourceBadge: () => null }));

import { ServersPage } from '@/features/servers/ServersPage';

describe('ServersPage credential reuse', () => {
  it('fills only blank fields when reusing credentials from another Space', async () => {
    findPriorCredentialMock.mockResolvedValue({
      source_space_id: 'prior-space',
      source_space_name: 'Prior Space',
    });
    getPriorCredentialValuesMock.mockResolvedValue({
      TOKEN: 'prior-token',
      ENDPOINT: 'https://prior.example.test',
    });

    const user = userEvent.setup();
    render(<ServersPage />);

    await user.click(await screen.findByRole('button', { name: 'Configure' }));
    await user.type(screen.getByTestId('config-input-TOKEN'), 'user-entered-token');
    await screen.findByTestId('prior-credential-nudge');
    await user.click(screen.getByTestId('prior-credential-use-btn'));

    await waitFor(() => {
      expect(screen.getByTestId('config-input-TOKEN')).toHaveValue('user-entered-token');
      expect(screen.getByTestId('config-input-ENDPOINT')).toHaveValue('https://prior.example.test');
    });
  });
});
