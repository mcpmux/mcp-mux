/**
 * Workspaces — "Set up a mapping" walkthrough.
 *
 * The step sequence is binding-type-aware. A PROJECT mapping is the full 3-step
 * flow: select a project (step 1, required), advance through the optional
 * connect-apps step (2), and on the tools step (3) Finish creates a binding with
 * the folder path, chosen Space, and the default Starter feature set. An
 * ID/virtual mapping skips the connect-apps step (step 1's ConnectPreview
 * already shows the config to paste), so it's a 2-step flow: identify → tools.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const { validateMock, openDialogMock } = vi.hoisted(() => ({
  validateMock: vi.fn(),
  openDialogMock: vi.fn(),
}));

// `@tauri-apps/plugin-dialog` is mocked globally in setup.ts (open: vi.fn()), but
// that shared instance isn't reachable from the test (a static import of this
// mocked-only package isn't Vite-resolvable). Re-declare the mock here with a
// hoisted fn we can drive directly (mirrors the validateMock pattern below).
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: openDialogMock }));
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
    openDialogMock.mockReset();
  });

  it('walks folder → apps → tools and Finish creates the binding', async () => {
    const user = userEvent.setup();
    const p = props();
    render(<WorkspaceSetupWizard {...p} />);

    // Project mode is the full 3-step sequence (identify → apps → tools).
    expect(screen.getByTestId('workspace-setup-wizard').textContent).toContain('Step 1 of 3');

    // Step 1: Next is disabled until a folder is chosen.
    expect(screen.getByTestId('wizard-step-folder')).toBeTruthy();
    expect(screen.getByTestId('wizard-next')).toHaveProperty('disabled', true);

    // Quick-pick the detected folder.
    await user.click(screen.getByRole('button', { name: /proj\/app/ }));
    expect(screen.getByTestId('wizard-next')).toHaveProperty('disabled', false);
    await user.click(screen.getByTestId('wizard-next'));

    // Step 2: connect apps (the install panel, stubbed) → Next.
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
      binding_type: 'path',
    });
    // The parent navigates to the new mapping's inspector (effective features);
    // the wizard itself does not close.
    expect(p.onClose).not.toHaveBeenCalled();
  });

  it('skips the connect-apps step for an Identifier mapping (identify → tools, "of 2")', async () => {
    // An ID/virtual mapping has no project to write app config for, and step 1's
    // ConnectPreview already shows the config to paste — so the connect-apps step
    // is dropped and the flow is just identify → tools (a 2-step sequence).
    const user = userEvent.setup();
    const p = props({ reportedRoots: [] });
    render(<WorkspaceSetupWizard {...p} />);
    // Flush ConnectPreview's gateway-status fetch inside act() before driving.
    await screen.findByTestId('wizard-connect-preview');

    // Switch to Identifier mode → the sequence is now 2 steps, not 3.
    await user.click(screen.getByTestId('wizard-type-id'));
    expect(screen.getByTestId('workspace-setup-wizard').textContent).toContain('Step 1 of 2');

    // Enter an identifier; Next is enabled and leads straight to the tools step.
    await user.type(screen.getByTestId('wizard-id-input'), 'my-id');
    expect(screen.getByTestId('wizard-next')).toHaveProperty('disabled', false);
    await user.click(screen.getByTestId('wizard-next'));

    // No connect-apps step: we land directly on tools, now "Step 2 of 2".
    expect(screen.queryByTestId('wizard-step-apps')).toBeNull();
    expect(screen.getByTestId('wizard-step-tools')).toBeTruthy();
    expect(screen.getByTestId('workspace-setup-wizard').textContent).toContain('Step 2 of 2');

    // Back returns to step 1 (identify), skipping the absent apps step.
    await user.click(screen.getByTestId('wizard-back'));
    expect(screen.getByTestId('wizard-step-folder')).toBeTruthy();
    expect(screen.getByTestId('workspace-setup-wizard').textContent).toContain('Step 1 of 2');

    // Finish from the tools step creates an id-typed binding.
    await user.click(screen.getByTestId('wizard-next'));
    await user.click(screen.getByTestId('wizard-finish'));
    await waitFor(() => expect(p.onCreate).toHaveBeenCalledTimes(1));
    expect(p.onCreate).toHaveBeenCalledWith({
      workspace_root: 'my-id',
      space_id: 's1',
      feature_set_ids: ['fs_starter'],
      binding_type: 'id',
    });
  });

  it('does not offer an already-mapped folder in the detected list', async () => {
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
    // Step 1 now renders the connect preview, whose gateway-status fetch settles
    // async — await it so that state update lands inside act(), not after.
    await screen.findByTestId('wizard-connect-preview');
    expect(screen.queryByRole('button', { name: /\/proj\/app$/ })).toBeNull();
    expect(screen.getByRole('button', { name: /\/proj\/other$/ })).toBeTruthy();
  });

  it('resets the value and warning when switching the binding-type tab', async () => {
    // Switching Folder ↔ Identifier must start the new tab clean: the entered
    // value AND any "already mapped" warning it raised must be cleared, in both
    // directions. (Regression: the toggle used to only flip the mode, leaving
    // the stale value + warning visible on the other tab.)
    const user = userEvent.setup();
    render(
      <WorkspaceSetupWizard
        {...props({
          reportedRoots: [],
          existingBindings: [
            { id: 'b1', workspace_root: 'dup-id', space_id: 's1', feature_set_ids: ['fs_starter'] },
            {
              id: 'b2',
              workspace_root: '/dup/folder',
              space_id: 's1',
              feature_set_ids: ['fs_starter'],
            },
          ],
        })}
      />
    );

    // Direction A: Identifier → Folder. Type an already-mapped identifier so the
    // warning surfaces, then switch tabs and confirm both value and warning clear.
    await user.click(screen.getByTestId('wizard-type-id'));
    const idInput = screen.getByTestId('wizard-id-input') as HTMLInputElement;
    await user.type(idInput, 'dup-id');
    expect(idInput.value).toBe('dup-id');
    expect(screen.getByTestId('wizard-folder-mapped-error')).toBeTruthy();

    await user.click(screen.getByTestId('wizard-type-path'));
    // On the Folder tab the warning is gone and Next is disabled (empty value).
    expect(screen.queryByTestId('wizard-folder-mapped-error')).toBeNull();
    expect(screen.getByTestId('wizard-next')).toHaveProperty('disabled', true);
    // Switching back shows the identifier input empty — the value was cleared.
    await user.click(screen.getByTestId('wizard-type-id'));
    expect((screen.getByTestId('wizard-id-input') as HTMLInputElement).value).toBe('');

    // Direction B: Folder → Identifier. Pick an already-mapped folder via the OS
    // dialog so the warning surfaces, then switch tabs and confirm it clears.
    openDialogMock.mockResolvedValue('/dup/folder');
    validateMock.mockResolvedValue('/dup/folder');

    await user.click(screen.getByTestId('wizard-type-path'));
    await user.click(screen.getByRole('button', { name: /Select a project/ }));
    await waitFor(() => expect(screen.getByTestId('wizard-folder-mapped-error')).toBeTruthy());

    await user.click(screen.getByTestId('wizard-type-id'));
    expect(screen.queryByTestId('wizard-folder-mapped-error')).toBeNull();
    expect((screen.getByTestId('wizard-id-input') as HTMLInputElement).value).toBe('');
  });

  it('previews the client config live — workspace header reflects the mode and entered value', async () => {
    // Step 1 fills its empty space with a "how a client connects" preview of the
    // exact MCP config. It's derived from the binding type + entered value, so it
    // pins the X-Mcpmux-Workspace header (a project path or identifier) and
    // updates as the user types.
    const user = userEvent.setup();
    render(<WorkspaceSetupWizard {...props({ reportedRoots: [] })} />);

    // Project mode (default): a templated project-path placeholder until one is
    // selected, plus the "three ways to route" note.
    const folderJson = screen.getByTestId('wizard-connect-preview-json');
    expect(folderJson.textContent).toContain('http://localhost:45818/mcp');
    expect(folderJson.textContent).toContain('"X-Mcpmux-Workspace": "<your-project-path>"');
    expect(screen.getByTestId('wizard-connect-preview').textContent).toMatch(
      /Route here with either the OAuth approval flow/
    );

    // Identifier mode, empty: an identifier placeholder, not a broken/empty header.
    await user.click(screen.getByTestId('wizard-type-id'));
    expect(screen.getByTestId('wizard-connect-preview-json').textContent).toContain(
      '"X-Mcpmux-Workspace": "<your-identifier>"'
    );

    // Updates live as the identifier is typed.
    await user.type(screen.getByTestId('wizard-id-input'), 'my-workspace');
    expect(screen.getByTestId('wizard-connect-preview-json').textContent).toContain(
      '"X-Mcpmux-Workspace": "my-workspace"'
    );

    // Switching back to Project clears the value, so the header falls back to the
    // project-path placeholder (and no longer shows the typed identifier).
    await user.click(screen.getByTestId('wizard-type-path'));
    const backJson = screen.getByTestId('wizard-connect-preview-json');
    expect(backJson.textContent).toContain('"X-Mcpmux-Workspace": "<your-project-path>"');
    expect(backJson.textContent).not.toContain('my-workspace');
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
