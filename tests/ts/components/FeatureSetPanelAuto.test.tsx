/**
 * FeatureSet panel in auto mode ("every server's tools"): everything shows as
 * included, there's nothing to save until the user edits, saving an edit
 * makes it a manual selection, and the switch flips auto mode directly.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const { mockSetMembers, mockSetAuto, mockListFeatures } = vi.hoisted(() => ({
  mockSetMembers: vi.fn(),
  mockSetAuto: vi.fn(),
  mockListFeatures: vi.fn(),
}));

vi.mock('@/lib/api/featureSets', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api/featureSets')>()),
  setFeatureSetMembers: mockSetMembers,
  setFeatureSetAutoInclude: mockSetAuto,
  getStarterToolSummary: vi.fn().mockResolvedValue({
    feature_set_id: 'fs_default_s1',
    auto_include: true,
    tool_count: 2,
    server_count: 1,
    threshold: 80,
    over_threshold: false,
  }),
}));
vi.mock('@/lib/api/serverFeatures', () => ({ listServerFeatures: mockListFeatures }));

import { FeatureSetPanel } from '@/features/featuresets/FeatureSetPanel';
import type { FeatureSet } from '@/lib/api/featureSets';

const starter: FeatureSet = {
  id: 'fs_default_s1',
  name: 'Starter',
  description: null,
  icon: null,
  space_id: 's1',
  feature_set_type: 'starter',
  server_id: null,
  is_builtin: true,
  is_deleted: false,
  auto_include: true,
  members: [],
};

function feature(id: string, name: string) {
  return {
    id,
    space_id: 's1',
    server_id: 'github',
    feature_type: 'tool' as const,
    feature_name: name,
    display_name: null,
    description: null,
    input_schema: null,
    discovered_at: '',
    last_seen_at: '',
    is_available: true,
  };
}

async function renderPanel(fs: FeatureSet = starter) {
  const onUpdate = vi.fn();
  render(<FeatureSetPanel featureSet={fs} spaceId="s1" onClose={() => {}} onUpdate={onUpdate} />);
  await waitFor(() => expect(screen.getByText('2 / 2 selected')).toBeInTheDocument());
  return { onUpdate };
}

describe('FeatureSetPanel — auto mode', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockListFeatures.mockResolvedValue([
      feature('f1', 'create_issue'),
      feature('f2', 'list_repos'),
    ]);
    mockSetMembers.mockResolvedValue({ ...starter, auto_include: false });
    mockSetAuto.mockResolvedValue({ ...starter, auto_include: false });
  });

  it('shows every feature as included and has nothing to save until edited', async () => {
    await renderPanel();

    expect(screen.getByTestId('featureset-auto-switch')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('featureset-save')).toBeDisabled();
    expect(screen.queryByTestId('featureset-save-leaves-auto')).not.toBeInTheDocument();
  });

  it('saving an edit switches the set to a manual selection', async () => {
    const user = userEvent.setup();
    const { onUpdate } = await renderPanel();

    await user.click(screen.getByText('github'));
    await user.click(screen.getByText('list_repos'));
    expect(screen.getByTestId('featureset-save-leaves-auto')).toBeInTheDocument();

    await user.click(screen.getByTestId('featureset-save'));
    await waitFor(() =>
      expect(mockSetMembers).toHaveBeenCalledWith('fs_default_s1', [
        { member_type: 'feature', member_id: 'f1', mode: 'include' },
      ])
    );
    expect(onUpdate).toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.getByTestId('featureset-auto-switch')).toHaveAttribute('aria-checked', 'false')
    );
  });

  it('turning the switch off keeps the tools and lets the user pick', async () => {
    const user = userEvent.setup();
    await renderPanel();

    await user.click(screen.getByTestId('featureset-auto-switch'));
    await waitFor(() => expect(mockSetAuto).toHaveBeenCalledWith('fs_default_s1', false));
    expect(screen.getByTestId('featureset-auto-switch')).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByText('2 / 2 selected')).toBeInTheDocument();
  });

  it('turning it back on asks first, then includes everything', async () => {
    const user = userEvent.setup();
    mockSetAuto.mockResolvedValue({ ...starter, auto_include: true });
    await renderPanel({
      ...starter,
      auto_include: false,
      members: [
        {
          id: 'm1',
          feature_set_id: starter.id,
          member_type: 'feature',
          member_id: 'f1',
          mode: 'include',
        },
      ],
    }).catch(() => {});
    await waitFor(() => expect(screen.getByText('1 / 2 selected')).toBeInTheDocument());

    await user.click(screen.getByTestId('featureset-auto-switch'));
    await user.click(await screen.findByRole('button', { name: 'Include everything' }));

    await waitFor(() => expect(mockSetAuto).toHaveBeenCalledWith('fs_default_s1', true));
    await waitFor(() => expect(screen.getByText('2 / 2 selected')).toBeInTheDocument());
  });

  it('does not offer auto mode on a manual custom set', async () => {
    await renderPanel({
      ...starter,
      id: 'fs_web',
      name: 'Web dev',
      feature_set_type: 'custom',
      is_builtin: false,
      auto_include: false,
      members: [
        {
          id: 'm1',
          feature_set_id: 'fs_web',
          member_type: 'feature',
          member_id: 'f1',
          mode: 'include',
        },
        {
          id: 'm2',
          feature_set_id: 'fs_web',
          member_type: 'feature',
          member_id: 'f2',
          mode: 'include',
        },
      ],
    });
    expect(screen.queryByTestId('featureset-auto-card')).not.toBeInTheDocument();
  });
});
