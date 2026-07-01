/**
 * WorkspaceBindingPanel — the "map this folder?" prompt and its disable switch.
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
  updateWorkspaceBinding: vi.fn().mockResolvedValue(undefined),
  listWorkspaceBindings: vi.fn().mockResolvedValue([]),
  validateWorkspaceRoot: vi.fn().mockResolvedValue('/home/u/proj'),
  getWorkspaceEffectiveFeatures: vi.fn().mockResolvedValue({
    workspace_root: '/home/u/proj',
    source: 'unbound',
    binding_id: null,
    space_id: 's1',
    space_name: 'Default',
    feature_sets: [{ id: 'fs1', name: 'Starter', feature_set_type: 'starter' }],
    tools: [],
    prompts: [],
    resources: [],
    server_totals: {},
  }),
}));

vi.mock('@/lib/api/spaces', () => ({
  listSpaces: vi.fn().mockResolvedValue([{ id: 's1', name: 'Default', is_default: true }]),
}));

vi.mock('@/lib/api/featureSets', () => ({
  isStarterFeatureSet: vi.fn(() => true),
  listFeatureSets: vi
    .fn()
    .mockResolvedValue([
      { id: 'fs1', name: 'Starter', feature_set_type: 'starter', space_id: 's1', is_deleted: false },
    ]),
}));

vi.mock('@/lib/api/machines', () => ({
  listMachines: vi.fn().mockResolvedValue([
    {
      id: 'm1',
      name: 'Rohan',
      icon: '⚔️',
      hostname: null,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    },
  ]),
  getLocalMachineId: vi.fn().mockResolvedValue(null),
  getClientMachineId: vi.fn().mockResolvedValue(null),
}));

vi.mock('@/lib/backend/events', () => ({
  useWorkspaceEvents: () => ({
    subscribe: (channel: string, cb: (payload: unknown) => void) => {
      workspaceHandlers.set(channel, cb);
      return () => workspaceHandlers.delete(channel);
    },
    subscribeMany: vi.fn(() => () => {}),
  }),
  useWorkspaceEventListener: vi.fn(),
}));

vi.mock('@/hooks/use-viewer-identity.hook', () => ({
  useViewerIdentity: () => ({ machineId: null, isLoading: false }),
  ViewerIdentityProvider: ({ children }: { children: React.ReactNode }) => children,
}));

import { WorkspaceBindingPanel } from '@/features/workspaces/workspace-binding-panel.component';
import { useBindingPanelStore } from '@/stores/bindingPanelStore';
import { updateWorkspaceBinding, validateWorkspaceRoot } from '@/lib/api/workspaceBindings';
import type { WorkspaceBinding } from '@/lib/api/workspaceBindings';

const EDIT_BINDING: WorkspaceBinding = {
  id: 'b1',
  workspace_root: '/home/u/proj',
  machine_id: 'm1',
  label: 'My Project',
  icon: '🫠',
  space_id: 's1',
  feature_set_ids: ['fs1'],
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
};

/** Invoke the captured `workspace-needs-binding` listener with a payload. */
async function fireNeedsBinding(overrides: Record<string, unknown> = {}) {
  const cb = workspaceHandlers.get('workspace-needs-binding');
  if (!cb) throw new Error('workspace-needs-binding listener was not registered');
  await cb({
    client_id: 'c',
    session_id: 's',
    space_id: 's1',
    workspace_root: '/home/u/proj',
    ...overrides,
  });
}

/** Simulate backend `workspace-binding-changed` after a binding write. */
function fireBindingChanged(overrides: Record<string, unknown> = {}) {
  const cb = workspaceHandlers.get('workspace-binding-changed');
  if (cb) {
    cb({
      workspace_root: '/home/u/proj',
      ...overrides,
    });
  }
}

/** Open the panel in edit mode and wait for async data load. */
async function openEditPanel() {
  useBindingPanelStore.getState().open({
    mode: 'edit',
    binding: EDIT_BINDING,
  });
  await screen.findByTestId('workspace-binding-panel');
  await screen.findByTestId('workspace-binding-icon-clear');
  await waitFor(() => expect(validateWorkspaceRoot).toHaveBeenCalled());
  await screen.findByText(/Ready to save/i);
}

function mockPromptEnabled(enabled: boolean) {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    if (cmd === 'get_workspace_mapping_prompt_enabled') return enabled;
    return undefined;
  });
}

describe('WorkspaceBindingPanel – mapping prompt toggle', () => {
  beforeEach(() => {
    workspaceHandlers.clear();
    vi.mocked(invoke).mockReset();
    useBindingPanelStore.getState().close();
  });

  it('shows the panel when the prompt setting is enabled', async () => {
    mockPromptEnabled(true);
    renderWithI18n(<WorkspaceBindingPanel />);
    await fireNeedsBinding();
    expect(await screen.findByTestId('workspace-binding-panel')).toBeTruthy();
    expect(await screen.findByTestId('workspace-binding-no-tools-banner')).toBeTruthy();
  });

  it('does NOT show the panel when the prompt setting is disabled', async () => {
    mockPromptEnabled(false);
    renderWithI18n(<WorkspaceBindingPanel />);
    await fireNeedsBinding();
    await waitFor(() => expect(screen.queryByTestId('workspace-binding-panel')).toBeNull());
  });

  it('the in-panel "stop asking" link disables the setting and closes', async () => {
    const user = userEvent.setup();
    mockPromptEnabled(true);
    renderWithI18n(<WorkspaceBindingPanel />);
    await fireNeedsBinding();
    await screen.findByTestId('workspace-binding-panel');

    await user.click(screen.getByTestId('workspace-binding-disable-prompt'));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('set_workspace_mapping_prompt_enabled', {
        enabled: false,
      }),
    );
    await waitFor(() => expect(screen.queryByTestId('workspace-binding-panel')).toBeNull());
  });

  it('locks the Space picker when the folder is base-dir scoped', async () => {
    mockPromptEnabled(true);
    renderWithI18n(<WorkspaceBindingPanel />);
    await fireNeedsBinding({ space_locked: true });
    await screen.findByTestId('workspace-binding-panel');

    const picker = screen.getByTestId('workspace-binding-space-picker') as HTMLSelectElement;
    expect(picker.disabled).toBe(true);
  });

  it('leaves the Space picker editable for an ordinary unmapped folder', async () => {
    mockPromptEnabled(true);
    renderWithI18n(<WorkspaceBindingPanel />);
    await fireNeedsBinding({ space_locked: false });
    await screen.findByTestId('workspace-binding-panel');

    const picker = screen.getByTestId('workspace-binding-space-picker') as HTMLSelectElement;
    expect(picker.disabled).toBe(false);
  });
});

describe('WorkspaceBindingPanel – edit mode stays open on save', () => {
  beforeEach(() => {
    workspaceHandlers.clear();
    vi.mocked(invoke).mockReset();
    vi.mocked(updateWorkspaceBinding).mockClear();
    vi.mocked(validateWorkspaceRoot).mockClear();
    useBindingPanelStore.getState().close();
  });

  it('does not close when workspace-binding-changed fires for the open binding', async () => {
    renderWithI18n(<WorkspaceBindingPanel />);
    await openEditPanel();

    fireBindingChanged();

    expect(screen.getByTestId('workspace-binding-panel')).toBeTruthy();
  });

  it('keeps the panel open after icon Clear persists in edit mode', async () => {
    const user = userEvent.setup();
    renderWithI18n(<WorkspaceBindingPanel />);
    await openEditPanel();

    await user.click(screen.getByTestId('workspace-binding-icon-clear'));

    await waitFor(() => expect(updateWorkspaceBinding).toHaveBeenCalled());
    fireBindingChanged();

    expect(screen.getByTestId('workspace-binding-panel')).toBeTruthy();
  });
});
