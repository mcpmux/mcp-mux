/**
 * Workspaces tab — "Clear unmapped" bulk action.
 *
 * Unmapped (amber) cards are live-reported roots with no binding. The bulk
 * "Clear unmapped" button forgets them all in one go (so the gateway offers
 * the "map this folder?" prompt again next time). These tests cover:
 *   - the button only appears while there are unmapped folders, and
 *   - confirming it calls `clearUnmappedReportedRoots`.
 *
 * `@mcpmux/ui` is aliased to the real source in vitest.config, so the real
 * `useConfirm` dialog renders — we drive it via `confirm-dialog-confirm`.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const {
  listWorkspaceBindingsMock,
  listReportedWorkspaceRootsMock,
  clearUnmappedReportedRootsMock,
} = vi.hoisted(() => ({
  listWorkspaceBindingsMock: vi.fn(),
  listReportedWorkspaceRootsMock: vi.fn(),
  clearUnmappedReportedRootsMock: vi.fn(),
}));

vi.mock('@/lib/api/workspaceBindings', () => ({
  listWorkspaceBindings: listWorkspaceBindingsMock,
  listReportedWorkspaceRoots: listReportedWorkspaceRootsMock,
  clearUnmappedReportedRoots: clearUnmappedReportedRootsMock,
  createWorkspaceBinding: vi.fn(),
  updateWorkspaceBinding: vi.fn(),
  deleteWorkspaceBinding: vi.fn(),
  getWorkspaceEffectiveFeatures: vi.fn(),
  validateWorkspaceRoot: vi.fn(),
}));

vi.mock('@/lib/api/featureSets', () => ({
  listFeatureSets: vi.fn().mockResolvedValue([]),
  isStarterFeatureSet: vi.fn(() => false),
}));

vi.mock('@/stores', () => ({
  useSpaces: () => [],
}));

import { WorkspacesPage } from '@/features/workspaces/WorkspacesPage';

describe('WorkspacesPage – clear unmapped', () => {
  beforeEach(() => {
    listWorkspaceBindingsMock.mockResolvedValue([]);
    listReportedWorkspaceRootsMock.mockResolvedValue([]);
    clearUnmappedReportedRootsMock.mockResolvedValue(0);
  });

  it('hides the "Clear unmapped" button when nothing is unmapped', async () => {
    render(<WorkspacesPage />);
    await waitFor(() => expect(listReportedWorkspaceRootsMock).toHaveBeenCalled());
    expect(screen.queryByTestId('workspaces-clear-unmapped')).toBeNull();
  });

  it('shows the button and clears unmapped roots after confirming', async () => {
    listReportedWorkspaceRootsMock.mockResolvedValue(['/home/u/unbound-folder']);
    clearUnmappedReportedRootsMock.mockResolvedValue(1);
    const user = userEvent.setup();

    render(<WorkspacesPage />);

    // Button shows because one live-reported root has no binding.
    const clearBtn = await screen.findByTestId('workspaces-clear-unmapped');
    await user.click(clearBtn);

    // Real confirm dialog — accept it.
    await user.click(await screen.findByTestId('confirm-dialog-confirm'));

    await waitFor(() => expect(clearUnmappedReportedRootsMock).toHaveBeenCalledTimes(1));
  });
});
