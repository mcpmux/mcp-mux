/**
 * FeatureSets tab — arriving from another tab's "New feature set" link.
 *
 * The Mapping form, setup wizard, and new-folder sheet all link here to create
 * a missing feature set. On arrival the create dialog opens, and a banner
 * offers the way back once the set exists.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const { navigateToMock, clearPendingMock, setViewSpaceMock, storeState } = vi.hoisted(() => ({
  navigateToMock: vi.fn(),
  clearPendingMock: vi.fn(),
  setViewSpaceMock: vi.fn(),
  storeState: { pending: null as null | { returnTo: string; restoreSpaceId?: string | null } },
}));

vi.mock('@/lib/api/featureSets', () => ({
  listFeatureSetsBySpace: vi.fn().mockResolvedValue([]),
  createFeatureSet: vi.fn(),
  deleteFeatureSet: vi.fn(),
  getFeatureSetWithMembers: vi.fn(),
  isStarterFeatureSet: () => false,
  getStarterToolSummary: vi.fn().mockResolvedValue(null),
}));

vi.mock('@/features/featuresets/FeatureSetPanel', () => ({ FeatureSetPanel: () => null }));

vi.mock('@/stores', () => ({
  useViewSpace: () => ({ id: 's1', name: 'Space One', is_default: true }),
  useNavigateTo: () => navigateToMock,
  usePendingFeatureSetCreate: () => storeState.pending,
  useSetPendingFeatureSetCreate: () => clearPendingMock,
  useSetViewSpace: () => setViewSpaceMock,
}));

import { FeatureSetsPage } from '@/features/featuresets/FeatureSetsPage';

describe('FeatureSetsPage – create redirect', () => {
  beforeEach(() => {
    navigateToMock.mockReset();
    clearPendingMock.mockReset();
    setViewSpaceMock.mockReset();
    storeState.pending = null;
  });

  it('opens the create dialog and offers a way back to Mapping', async () => {
    storeState.pending = { returnTo: 'workspaces' };
    const user = userEvent.setup();
    render(<FeatureSetsPage />);

    // Create dialog is up (its name field is present) and the intent is consumed.
    expect(await screen.findByPlaceholderText('e.g., GitHub Read Only')).toBeTruthy();
    expect(clearPendingMock).toHaveBeenCalledWith(null);

    const back = screen.getByTestId('featuresets-return-btn');
    expect(back.textContent).toContain('Back to Mapping');
    await user.click(back);
    expect(navigateToMock).toHaveBeenCalledWith('workspaces');
    // No Space switch happened on the way here, so none to undo.
    expect(setViewSpaceMock).not.toHaveBeenCalled();
  });

  it('restores the previously viewed Space on the way back', async () => {
    // The link switched the Space switcher to the mapping's Space; going back
    // puts it where the user had it.
    storeState.pending = { returnTo: 'workspaces', restoreSpaceId: 's0' };
    const user = userEvent.setup();
    render(<FeatureSetsPage />);
    await user.click(await screen.findByTestId('featuresets-return-btn'));
    expect(setViewSpaceMock).toHaveBeenCalledWith('s0');
    expect(navigateToMock).toHaveBeenCalledWith('workspaces');
  });

  it('shows no banner on a normal visit', async () => {
    render(<FeatureSetsPage />);
    await screen.findByTestId('featuresets-page');
    expect(screen.queryByTestId('featuresets-return-banner')).toBeNull();
  });
});
