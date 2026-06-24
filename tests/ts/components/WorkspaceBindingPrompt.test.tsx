/**
 * WorkspaceBindingSheet — the "map this folder?" prompt and its disable switch.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { invoke } from '@tauri-apps/api/core';
import { renderWithI18n } from '../render-with-i18n.helpers';

const { workspaceHandlers } = vi.hoisted(() => ({
  workspaceHandlers: new Map<string, (payload: unknown) => void>(),
}));

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

vi.mock('@/lib/backend/events', () => ({
  useWorkspaceEvents: () => ({
    subscribe: (channel: string, cb: (payload: unknown) => void) => {
      workspaceHandlers.set(channel, cb);
      return () => workspaceHandlers.delete(channel);
    },
    subscribeMany: vi.fn(() => () => {}),
  }),
}));

import { WorkspaceBindingSheet } from '@/features/workspaces/WorkspaceBindingSheet';

const TITLE = /This folder is using your Starter set/i;

/** Invoke the captured `workspace-needs-binding` listener with a payload. */
function fireNeedsBinding(overrides: Record<string, unknown> = {}) {
  const cb = workspaceHandlers.get('workspace-needs-binding');
  if (!cb) throw new Error('workspace-needs-binding listener was not registered');
  return cb({
    client_id: 'c',
    session_id: 's',
    space_id: 's1',
    workspace_root: '/home/u/proj',
    ...overrides,
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
    workspaceHandlers.clear();
    vi.mocked(invoke).mockReset();
  });

  it('shows the sheet when the prompt setting is enabled', async () => {
    mockPromptEnabled(true);
    renderWithI18n(<WorkspaceBindingSheet />);
    await fireNeedsBinding();
    expect(await screen.findByText(TITLE)).toBeTruthy();
  });

  it('does NOT show the sheet when the prompt setting is disabled', async () => {
    mockPromptEnabled(false);
    renderWithI18n(<WorkspaceBindingSheet />);
    await fireNeedsBinding();
    await waitFor(() => expect(screen.queryByText(TITLE)).toBeNull());
  });

  it('the in-sheet "stop asking" link disables the setting and closes', async () => {
    const user = userEvent.setup();
    mockPromptEnabled(true);
    renderWithI18n(<WorkspaceBindingSheet />);
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

  it('locks the Space picker when the folder is base-dir scoped', async () => {
    mockPromptEnabled(true);
    renderWithI18n(<WorkspaceBindingSheet />);
    await fireNeedsBinding({ space_locked: true });
    await screen.findByText(TITLE);

    const picker = screen.getByTestId('workspace-binding-space-picker') as HTMLSelectElement;
    expect(picker.disabled).toBe(true);
  });

  it('leaves the Space picker editable for an ordinary unmapped folder', async () => {
    mockPromptEnabled(true);
    renderWithI18n(<WorkspaceBindingSheet />);
    await fireNeedsBinding({ space_locked: false });
    await screen.findByText(TITLE);

    const picker = screen.getByTestId('workspace-binding-space-picker') as HTMLSelectElement;
    expect(picker.disabled).toBe(false);
  });
});
