/**
 * Clients tab — the client → mapping link.
 *
 * Clients are shown by name (never by their raw `mcp_…` id), each API-key
 * client's card summarises the tools its mapping gives it, and "Configure
 * mapping" deep-links to that client's mapping in the Mapping tab.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const { navigateToMock, setPendingMappingMock } = vi.hoisted(() => ({
  navigateToMock: vi.fn(),
  setPendingMappingMock: vi.fn(),
}));

const CLIENT_ID = 'mcp_ab12cd34';

vi.mock('@/lib/api/gateway', () => ({
  getGatewayStatus: vi.fn().mockResolvedValue({
    running: true,
    url: 'http://localhost:45818',
    active_sessions: 0,
    connected_backends: 0,
  }),
  listOAuthClients: vi.fn().mockResolvedValue([
    {
      client_id: 'mcp_ab12cd34',
      registration_type: 'preregistered',
      client_name: 'CI runner',
      client_alias: null,
      redirect_uris: [],
      scope: null,
      approved: true,
      last_seen: null,
      created_at: '',
      reports_roots: false,
      roots_capability_known: false,
      locked_space_id: null,
    },
  ]),
  updateOAuthClient: vi.fn(),
  deleteOAuthClient: vi.fn(),
  getOAuthClientGrants: vi.fn().mockResolvedValue([]),
  grantOAuthClientFeatureSet: vi.fn(),
  revokeOAuthClientFeatureSet: vi.fn(),
  listClientApiKeys: vi.fn().mockResolvedValue([]),
  createClientApiKey: vi.fn(),
  revokeClientApiKey: vi.fn(),
  registerApiKeyClient: vi.fn(),
}));

vi.mock('@/lib/api/workspaceBindings', () => ({
  listWorkspaceBindings: vi.fn().mockResolvedValue([
    {
      id: 'b1',
      workspace_root: 'mcp_ab12cd34',
      binding_type: 'id',
      space_id: 's1',
      feature_set_ids: ['fs_docs'],
      created_at: '',
      updated_at: '',
    },
  ]),
}));

vi.mock('@/lib/api/featureSets', () => ({
  isStarterFeatureSet: () => false,
  listFeatureSets: vi
    .fn()
    .mockResolvedValue([
      { id: 'fs_docs', name: 'Docs', space_id: 's1', feature_set_type: 'custom' },
    ]),
  listFeatureSetsBySpace: vi.fn().mockResolvedValue([]),
}));

vi.mock('@/components/ConnectIDEs', () => ({ ConnectIDEs: () => null }));

vi.mock('@/stores', () => ({
  useDefaultSpace: () => ({ id: 's1', name: 'Space One', is_default: true }),
  useSpaces: () => [{ id: 's1', name: 'Space One', is_default: true }],
  useNavigateTo: () => navigateToMock,
  usePendingClientId: () => null,
  useSetPendingClientId: () => () => {},
  useSetPendingMapping: () => setPendingMappingMock,
  useSetPendingFeatureSetCreate: () => () => {},
  useSetViewSpace: () => () => {},
  useViewSpaceId: () => 's1',
}));

import ClientsPage from '@/features/clients/ClientsPage';

describe('ClientsPage – client mapping', () => {
  beforeEach(() => {
    navigateToMock.mockReset();
    setPendingMappingMock.mockReset();
  });

  it("shows the client by name with its mapping's tools on the card", async () => {
    render(<ClientsPage />);
    const card = await screen.findByTestId(`client-card-${CLIENT_ID}`);
    expect(within(card).getByRole('heading', { name: 'CI runner' })).toBeTruthy();
    expect(within(card).queryByText(CLIENT_ID)).toBeNull();
    const mapping = await within(card).findByTestId(`client-card-mapping-${CLIENT_ID}`);
    expect(mapping.textContent).toContain('Docs');
    expect(mapping.textContent).toContain('Space One');
  });

  it('opens the panel by name and deep-links "Configure mapping" to this client', async () => {
    const user = userEvent.setup();
    render(<ClientsPage />);
    await user.click(await screen.findByTestId(`client-card-${CLIENT_ID}`));

    // The panel subtitle describes the client instead of echoing its raw id.
    expect(screen.getByText('API-key client', { selector: 'h2 + div p' })).toBeTruthy();

    const tools = await screen.findByTestId('client-tools-section');
    expect(tools.textContent).toContain('Every API key of CI runner gets');
    await user.click(within(tools).getByTestId('clients-panel-configure-mapping'));

    expect(setPendingMappingMock).toHaveBeenCalledWith({ key: CLIENT_ID, bindingType: 'id' });
    expect(navigateToMock).toHaveBeenCalledWith('workspaces');
  });

  it('header "Configure mapping" goes to the Mapping tab', async () => {
    const user = userEvent.setup();
    render(<ClientsPage />);
    await user.click(await screen.findByTestId('clients-configure-mapping-btn'));
    expect(navigateToMock).toHaveBeenCalledWith('workspaces');
  });
});
