/**
 * Mapping tab — client (id-keyed) mappings.
 *
 * An API-key client is auto-mapped by its client id (`mcp_xxxxxxxx`). The
 * Mapping tab must show such a mapping by the client's NAME, look up its
 * effective features as an id (not validate it as a folder path — that was
 * the "workspace path" error), hide the folder-only "Connect apps" section,
 * pin the Space for a locked client, and open the right mapping (or a
 * prefilled create flow) when the Clients tab deep-links here.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const {
  listWorkspaceBindingsMock,
  getEffectiveMock,
  listOAuthClientsMock,
  setPendingMappingMock,
  storeState,
} = vi.hoisted(() => ({
  listWorkspaceBindingsMock: vi.fn(),
  getEffectiveMock: vi.fn(),
  listOAuthClientsMock: vi.fn(),
  setPendingMappingMock: vi.fn(),
  storeState: {
    pendingMapping: null as null | { key: string; bindingType: 'path' | 'id' },
  },
}));

vi.mock('@/lib/api/workspaceBindings', () => ({
  listWorkspaceBindings: listWorkspaceBindingsMock,
  listReportedWorkspaceRoots: vi.fn().mockResolvedValue([]),
  clearUnmappedReportedRoots: vi.fn(),
  createWorkspaceBinding: vi.fn(),
  updateWorkspaceBinding: vi.fn(),
  deleteWorkspaceBinding: vi.fn(),
  getWorkspaceEffectiveFeatures: getEffectiveMock,
  validateWorkspaceRoot: vi.fn(),
}));

vi.mock('@/lib/api/featureSets', () => ({
  listFeatureSets: vi.fn().mockResolvedValue([
    { id: 'fs1', name: 'Starter', space_id: 's1', feature_set_type: 'starter', members: [] },
    { id: 'fs2', name: 'Locked Starter', space_id: 's2', feature_set_type: 'starter', members: [] },
  ]),
  isStarterFeatureSet: (fs: { feature_set_type: string }) => fs.feature_set_type === 'starter',
}));

vi.mock('@/lib/api/gateway', () => ({
  listOAuthClients: listOAuthClientsMock,
}));

vi.mock('@/features/workspaces/WorkspaceInstallPanel', () => ({
  WorkspaceInstallPanel: () => null,
}));

vi.mock('@/stores', () => ({
  useSpaces: () => [
    { id: 's1', name: 'Space One', is_default: true },
    { id: 's2', name: 'Locked Space', is_default: false },
  ],
  usePendingWorkspaceNew: () => false,
  useSetPendingWorkspaceNew: () => () => {},
  usePendingMapping: () => storeState.pendingMapping,
  useSetPendingMapping: () => setPendingMappingMock,
  useNavigateTo: () => () => {},
  useSetPendingClientId: () => () => {},
  useSetViewSpace: () => () => {},
  useViewSpaceId: () => 's1',
  useSetPendingFeatureSetCreate: () => () => {},
}));

import { WorkspacesPage } from '@/features/workspaces/WorkspacesPage';

const CLIENT_ID = 'mcp_ab12cd34';
const client = (over: Record<string, unknown> = {}) => ({
  client_id: CLIENT_ID,
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
  ...over,
});
const clientBinding = (spaceId = 's1', fsIds = ['fs1']) => ({
  id: 'b1',
  workspace_root: CLIENT_ID,
  binding_type: 'id',
  space_id: spaceId,
  feature_set_ids: fsIds,
  created_at: '',
  updated_at: '',
});

describe('WorkspacesPage – client mappings', () => {
  beforeEach(() => {
    storeState.pendingMapping = null;
    setPendingMappingMock.mockReset();
    listWorkspaceBindingsMock.mockResolvedValue([clientBinding()]);
    listOAuthClientsMock.mockResolvedValue([client()]);
    getEffectiveMock.mockResolvedValue({
      workspace_root: CLIENT_ID,
      source: 'binding',
      binding_id: 'b1',
      space_id: 's1',
      space_name: 'Space One',
      feature_sets: [],
      tools: [],
      prompts: [],
      resources: [],
      server_totals: {},
    });
  });

  it('shows a client-id mapping by the client name, not the raw id', async () => {
    render(<WorkspacesPage />);
    const card = await screen.findByTestId('workspace-entry-b1');
    expect(within(card).getByRole('heading', { name: 'CI runner' })).toBeTruthy();
    expect(within(card).getByText('Client')).toBeTruthy();
  });

  it('finds a client mapping by searching the client name', async () => {
    const user = userEvent.setup();
    render(<WorkspacesPage />);
    await screen.findByTestId('workspace-entry-b1');
    await user.type(screen.getByTestId('workspace-binding-search'), 'ci run');
    expect(screen.getByTestId('workspace-entry-b1')).toBeTruthy();
  });

  it('loads effective features as an id and hides the folder-only install section', async () => {
    const user = userEvent.setup();
    render(<WorkspacesPage />);
    await user.click(await screen.findByTestId('workspace-entry-b1'));

    await waitFor(() => expect(getEffectiveMock).toHaveBeenCalledWith(CLIENT_ID, 'id'));
    expect(screen.queryByTestId('workspace-install-section')).toBeNull();
    // The key isn't editable — the mapping shows the client by name.
    expect(
      within(screen.getByTestId('workspace-binding-client')).getByText('CI runner')
    ).toBeTruthy();
  });

  it('pins the Space picker for a client locked to a Space', async () => {
    listWorkspaceBindingsMock.mockResolvedValue([clientBinding('s2', ['fs2'])]);
    listOAuthClientsMock.mockResolvedValue([client({ locked_space_id: 's2' })]);
    const user = userEvent.setup();
    render(<WorkspacesPage />);
    await user.click(await screen.findByTestId('workspace-entry-b1'));

    const space = (await screen.findByTestId('workspace-binding-space')) as HTMLSelectElement;
    expect(space.disabled).toBe(true);
    expect(space.value).toBe('s2');
    expect(screen.getByTestId('workspace-binding-space-locked')).toBeTruthy();
  });

  it('opens the existing mapping when deep-linked from a client', async () => {
    storeState.pendingMapping = { key: CLIENT_ID, bindingType: 'id' };
    render(<WorkspacesPage />);

    await waitFor(() => expect(getEffectiveMock).toHaveBeenCalledWith(CLIENT_ID, 'id'));
    expect(screen.getByTestId('workspace-binding-client')).toBeTruthy();
    expect(setPendingMappingMock).toHaveBeenCalledWith(null);
  });

  it('starts a prefilled client mapping when the deep-linked client has none', async () => {
    listWorkspaceBindingsMock.mockResolvedValue([]);
    storeState.pendingMapping = { key: CLIENT_ID, bindingType: 'id' };
    render(<WorkspacesPage />);

    await screen.findByTestId('workspace-setup-wizard');
    const input = screen.getByTestId('wizard-id-input') as HTMLInputElement;
    expect(input.value).toBe(CLIENT_ID);
    // The wizard names the client and lets the user continue straight away.
    expect(screen.getByTestId('wizard-next')).toHaveProperty('disabled', false);
  });
});
