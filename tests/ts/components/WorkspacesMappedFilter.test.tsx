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

vi.mock('@/stores', () => ({
  useSpaces: () => [{ id: 's1', name: 'Space One' }],
  usePendingWorkspaceNew: () => false,
  useSetPendingWorkspaceNew: () => () => {},
}));

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
    render(<WorkspacesPage />);
    expect(await screen.findByTestId(MAPPED_TESTID)).toBeTruthy();
    expect(screen.getByTestId(UNMAPPED_TESTID)).toBeTruthy();
  });

  it('"Mapped" shows only the bound folder', async () => {
    const user = userEvent.setup();
    render(<WorkspacesPage />);
    await screen.findByTestId(MAPPED_TESTID);

    await user.click(screen.getByTestId('workspace-filter-mapped'));

    expect(screen.getByTestId(MAPPED_TESTID)).toBeTruthy();
    await waitFor(() => expect(screen.queryByTestId(UNMAPPED_TESTID)).toBeNull());
  });

  it('"Unmapped" shows only the folder without a binding', async () => {
    const user = userEvent.setup();
    render(<WorkspacesPage />);
    await screen.findByTestId(UNMAPPED_TESTID);

    await user.click(screen.getByTestId('workspace-filter-unmapped'));

    expect(screen.getByTestId(UNMAPPED_TESTID)).toBeTruthy();
    await waitFor(() => expect(screen.queryByTestId(MAPPED_TESTID)).toBeNull());
  });
});
