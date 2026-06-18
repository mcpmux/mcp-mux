/**
 * WorkspaceBindingSheet — the "map this folder?" prompt and its disable switch.
 *
 * The sheet pops on a `workspace-needs-binding` event, but only when the
 * "Ask to map new folders" setting is on (default). These tests drive the
 * event through the (globally mocked) Tauri `listen` and assert the sheet
 * honors the setting, plus that the in-sheet "stop asking" link turns it off.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

vi.mock('@/lib/api/workspaceBindings', () => ({
  createWorkspaceBinding: vi.fn(),
}));

vi.mock('@/lib/api/spaces', () => ({
  listSpaces: vi.fn().mockResolvedValue([{ id: 's1', name: 'Default', is_default: true }]),
}));

vi.mock('@/lib/api/featureSets', () => ({
  isStarterFeatureSet: vi.fn(() => true),
  listFeatureSetsBySpace: vi
    .fn()
    .mockResolvedValue([
      { id: 'fs1', name: 'Starter', feature_set_type: 'starter', is_deleted: false },
    ]),
}));

import { WorkspaceBindingSheet } from '@/features/workspaces/WorkspaceBindingSheet';

const TITLE = /This folder is using your Starter set/i;

/** Invoke the captured `workspace-needs-binding` listener with a payload. */
function fireNeedsBinding() {
  const call = vi.mocked(listen).mock.calls.find((c) => c[0] === 'workspace-needs-binding');
  if (!call) throw new Error('workspace-needs-binding listener was not registered');
  const cb = call[1] as (e: { payload: unknown }) => unknown | Promise<unknown>;
  return cb({
    payload: { client_id: 'c', session_id: 's', space_id: 's1', workspace_root: '/home/u/proj' },
  });
}

function mockPromptEnabled(enabled: boolean) {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    if (cmd === 'get_workspace_mapping_prompt_enabled') return enabled;
    return undefined;
  });
}

describe('WorkspaceBindingSheet – mapping prompt toggle', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it('shows the sheet when the prompt setting is enabled', async () => {
    mockPromptEnabled(true);
    render(<WorkspaceBindingSheet />);
    await fireNeedsBinding();
    expect(await screen.findByText(TITLE)).toBeTruthy();
  });

  it('does NOT show the sheet when the prompt setting is disabled', async () => {
    mockPromptEnabled(false);
    render(<WorkspaceBindingSheet />);
    await fireNeedsBinding();
    await waitFor(() => expect(screen.queryByText(TITLE)).toBeNull());
  });

  it('the in-sheet "stop asking" link disables the setting and closes', async () => {
    const user = userEvent.setup();
    mockPromptEnabled(true);
    render(<WorkspaceBindingSheet />);
    await fireNeedsBinding();
    await screen.findByText(TITLE);

    await user.click(screen.getByTestId('workspace-binding-disable-prompt'));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('set_workspace_mapping_prompt_enabled', {
        enabled: false,
      })
    );
    await waitFor(() => expect(screen.queryByText(TITLE)).toBeNull());
  });
});
