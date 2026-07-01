/**
 * Workspaces tab — "Mapped" filter segment.
 *
 * The Workspaces list unions live-reported roots with saved bindings. The
 * segmented filter lets the user narrow to a slice; this covers the new
 * "Mapped" segment (folders that have an explicit binding), alongside the
 * existing "Unmapped" one, so the two stay mutually exclusive.
 *
 * `@mcpmux/ui` is aliased to the real source in vitest.config so the real
 * controls render.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const { listWorkspaceBindingsMock, listReportedWorkspaceRootsMock } = vi.hoisted(() => ({
  listWorkspaceBindingsMock: vi.fn(),
  listReportedWorkspaceRootsMock: vi.fn(),
}));

vi.mock('@/lib/api/workspaceBindings', () => ({
  listWorkspaceBindings: listWorkspaceBindingsMock,
  listReportedWorkspaceRoots: listReportedWorkspaceRootsMock,
  clearUnmappedReportedRoots: vi.fn(),
  createWorkspaceBinding: vi.fn(),
  updateWorkspaceBinding: vi.fn(),
  deleteWorkspaceBinding: vi.fn(),
  getWorkspaceEffectiveFeatures: vi.fn(),
  validateWorkspaceRoot: vi.fn(),
}));

vi.mock('@/lib/api/featureSets', () => ({
  listFeatureSets: vi
    .fn()
    .mockResolvedValue([
      { id: 'fs1', name: 'Set One', feature_set_type: 'custom', members: [] },
    ]),
  isStarterFeatureSet: vi.fn(() => false),
}));

vi.mock('@/lib/api/workspaceAppearances', () => ({
  listWorkspaceAppearances: vi.fn().mockResolvedValue([]),
  deleteWorkspaceAppearance: vi.fn(),
  upsertWorkspaceAppearance: vi.fn(),
  uploadWorkspaceIcon: vi.fn(),
}));

vi.mock('@/lib/api/machines', () => ({
  listMachines: vi.fn().mockResolvedValue([]),
  getLocalMachineId: vi.fn().mockResolvedValue(null),
}));

vi.mock('@/lib/backend/events', () => ({
  useWorkspaceEvents: () => ({
    subscribe: vi.fn(() => () => {}),
    subscribeMany: vi.fn(() => () => {}),
  }),
  useWorkspaceEventListener: vi.fn(),
}));

vi.mock('@/stores', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/stores')>();
  return {
    ...actual,
    useSpaces: () => [{ id: 's1', name: 'Space One' }],
    usePendingWorkspaceNew: () => false,
    useSetPendingWorkspaceNew: () => () => {},
  };
});

vi.mock('@/hooks/use-viewer-identity.hook', () => ({
  useViewerIdentity: () => ({ machineId: null, isLoading: false }),
  ViewerIdentityProvider: ({ children }: { children: React.ReactNode }) => children,
}));

import { renderWithI18n } from '../render-with-i18n.helpers';
import { WorkspacesPage } from '@/features/workspaces/WorkspacesPage';

const MAPPED_ROOT = '/home/u/mapped';
const UNMAPPED_ROOT = '/home/u/unmapped';
const MAPPED_TESTID = 'workspace-entry-b1';
const UNMAPPED_TESTID = `workspace-entry-live:${UNMAPPED_ROOT}`;

describe('WorkspacesPage – Mapped/Unmapped filter', () => {
  beforeEach(() => {
    // One folder with a binding (mapped) and one live-reported folder with no
    // binding (unmapped). Both are live, so the "Live" filter keeps both.
    listWorkspaceBindingsMock.mockResolvedValue([
      { id: 'b1', workspace_root: MAPPED_ROOT, space_id: 's1', feature_set_ids: ['fs1'] },
    ]);
    listReportedWorkspaceRootsMock.mockResolvedValue([MAPPED_ROOT, UNMAPPED_ROOT]);
  });

  it('shows both entries under the default "All" filter', async () => {
    renderWithI18n(<WorkspacesPage />);
    expect(await screen.findByTestId(MAPPED_TESTID)).toBeTruthy();
    expect(screen.getByTestId(UNMAPPED_TESTID)).toBeTruthy();
  });

  it('"Mapped" shows only the bound folder', async () => {
    const user = userEvent.setup();
    renderWithI18n(<WorkspacesPage />);
    await screen.findByTestId(MAPPED_TESTID);

    await user.click(screen.getByTestId('workspace-filter-mapped'));

    expect(screen.getByTestId(MAPPED_TESTID)).toBeTruthy();
    await waitFor(() => expect(screen.queryByTestId(UNMAPPED_TESTID)).toBeNull());
  });

  it('"Unmapped" shows only the folder without a binding', async () => {
    const user = userEvent.setup();
    renderWithI18n(<WorkspacesPage />);
    await screen.findByTestId(UNMAPPED_TESTID);

    await user.click(screen.getByTestId('workspace-filter-unmapped'));

    expect(screen.getByTestId(UNMAPPED_TESTID)).toBeTruthy();
    await waitFor(() => expect(screen.queryByTestId(MAPPED_TESTID)).toBeNull());
  });
});
