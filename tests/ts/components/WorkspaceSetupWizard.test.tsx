/**
 * Workspaces — "Set up a folder" walkthrough.
 *
 * Verifies the 3-step create flow: pick a folder (step 1, required), advance
 * through the optional connect-apps step (2), and on the tools step (3) Finish
 * creates a binding with the folder path, chosen Space, and the default Starter
 * feature set pre-selected.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const { validateMock } = vi.hoisted(() => ({
  validateMock: vi.fn(),
}));

// `@tauri-apps/plugin-dialog` is mocked globally in setup.ts (open: vi.fn()).
// We reconfigure that shared mock per-test via vi.importMock (a static import
// of this mocked-only package isn't Vite-resolvable from the test).
vi.mock('@/lib/api/workspaceBindings', () => ({ validateWorkspaceRoot: validateMock }));
vi.mock('@/lib/api/featureSets', () => ({
  isStarterFeatureSet: (fs: { feature_set_type: string }) =>
    fs.feature_set_type === 'starter' || fs.feature_set_type === 'default',
}));
// Step 2 embeds the install panel; stub it out — it has its own tests.
vi.mock('@/features/workspaces/WorkspaceInstallPanel', () => ({
  WorkspaceInstallPanel: () => null,
}));

import { WorkspaceSetupWizard } from '@/features/workspaces/WorkspaceSetupWizard';

const SPACES = [
  { id: 's1', name: 'Default', icon: '', description: null, is_default: true, sort_order: 0, created_at: '', updated_at: '' },
];
const FEATURE_SETS = [
  { id: 'fs_starter', name: 'Starter', space_id: 's1', feature_set_type: 'starter' },
  { id: 'fs_a', name: 'Custom A', space_id: 's1', feature_set_type: 'custom' },
];
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const props = (over: any = {}) => ({
  spaces: SPACES as any,
  featureSets: FEATURE_SETS as any,
  reportedRoots: ['/proj/app'],
  existingBindings: [],
  onClose: vi.fn(),
  onCreate: vi.fn().mockResolvedValue({ id: 'b1' }),
  onError: vi.fn(),
  ...over,
});

describe('WorkspaceSetupWizard', () => {
  beforeEach(() => {
    validateMock.mockReset();
  });

  it('walks folder → apps → tools and Finish creates the binding', async () => {
    const user = userEvent.setup();
    const p = props();
    render(<WorkspaceSetupWizard {...p} />);

    // Step 1: Next is disabled until a folder is chosen.
    expect(screen.getByTestId('wizard-step-folder')).toBeTruthy();
    expect(screen.getByTestId('wizard-next')).toHaveProperty('disabled', true);

    // Quick-pick the detected folder.
    await user.click(screen.getByRole('button', { name: /proj\/app/ }));
    expect(screen.getByTestId('wizard-next')).toHaveProperty('disabled', false);
    await user.click(screen.getByTestId('wizard-next'));

    // Step 2: connect apps (stubbed) → Next.
    expect(screen.getByTestId('wizard-step-apps')).toBeTruthy();
    await user.click(screen.getByTestId('wizard-next'));

    // Step 3: Starter is pre-selected; Finish creates the binding.
    expect(screen.getByTestId('wizard-step-tools')).toBeTruthy();
    await user.click(screen.getByTestId('wizard-finish'));

    await waitFor(() => expect(p.onCreate).toHaveBeenCalledTimes(1));
    expect(p.onCreate).toHaveBeenCalledWith({
      workspace_root: '/proj/app',
      space_id: 's1',
      feature_set_ids: ['fs_starter'],
    });
    // The parent navigates to the new mapping's inspector (effective features);
    // the wizard itself does not close.
    expect(p.onClose).not.toHaveBeenCalled();
  });

  it('does not offer an already-mapped folder in the detected list', () => {
    // The quick-pick list filters out folders that already have a binding, so a
    // mapped folder can't be re-picked there; an unmapped one is still offered.
    // (Picking a mapped folder via the OS dialog is guarded separately by the
    // alreadyMapped check, which disables Next and shows an inline error.)
    render(
      <WorkspaceSetupWizard
        {...props({
          reportedRoots: ['/proj/app', '/proj/other'],
          existingBindings: [
            { id: 'b1', workspace_root: '/proj/app', space_id: 's1', feature_set_ids: ['fs_starter'] },
          ],
        })}
      />
    );
    expect(screen.queryByRole('button', { name: /\/proj\/app$/ })).toBeNull();
    expect(screen.getByRole('button', { name: /\/proj\/other$/ })).toBeTruthy();
  });

  it('lets you go Back from a later step', async () => {
    const user = userEvent.setup();
    render(<WorkspaceSetupWizard {...props()} />);
    await user.click(screen.getByRole('button', { name: /proj\/app/ }));
    await user.click(screen.getByTestId('wizard-next')); // → step 2
    expect(screen.getByTestId('wizard-step-apps')).toBeTruthy();
    await user.click(screen.getByTestId('wizard-back')); // → step 1
    expect(screen.getByTestId('wizard-step-folder')).toBeTruthy();
  });
});
