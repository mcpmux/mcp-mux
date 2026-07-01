/**
 * Approval dialog wiring:
 *  - renders the target-Space chip (so a cross-Space write is obvious),
 *  - survives a freeform `diff` shape (`{ added_tools }`) without crashing,
 *  - the "Manage approval prompts" link denies the request (fail-closed) and
 *    routes to the Built-in tab where prompts can be disabled.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithI18n } from '../render-with-i18n.helpers';

const { subscriptionHandlers, navigateToSpy, respondToMetaToolApprovalMock } = vi.hoisted(() => ({
  subscriptionHandlers: new Map<string, (payload: unknown) => void>(),
  navigateToSpy: vi.fn(),
  respondToMetaToolApprovalMock: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@/lib/backend/events', () => ({
  useBackendEventSubscription: vi.fn((channel: string, cb: (payload: unknown) => void) => {
    subscriptionHandlers.set(channel, cb);
  }),
}));

vi.mock('@/lib/api/metaTools', () => ({
  respondToMetaToolApproval: respondToMetaToolApprovalMock,
}));

vi.mock('@/hooks/use-navigate.hook', () => ({ useNavigate: () => navigateToSpy }));

import { MetaToolApprovalDialog } from '@/features/metaTools/MetaToolApprovalDialog';

async function emitRequest(payload: Record<string, unknown>) {
  await act(async () => {
    subscriptionHandlers.get('meta-tool-approval-request')?.({
      request_id: 'req-1',
      client_id: 'client-1',
      payload,
      expires_at_unix_secs: 9_999_999_999,
    });
    await Promise.resolve();
  });
}

describe('MetaToolApprovalDialog', () => {
  beforeEach(() => {
    subscriptionHandlers.clear();
    navigateToSpy.mockClear();
    respondToMetaToolApprovalMock.mockClear();
  });

  it('names the target Space when present', async () => {
    renderWithI18n(<MetaToolApprovalDialog />);
    await emitRequest({
      tool_name: 'mcpmux_manage_feature_set',
      summary: "Create FeatureSet 'X' in Space 'Personal'",
      space_name: 'Personal',
      diff: null,
      raw_args: {},
      affects_other_clients: false,
    });
    const chip = await screen.findByTestId('meta-tool-approval-space');
    expect(chip).toHaveTextContent('Personal');
  });

  it('omits the Space chip when no target Space is given', async () => {
    renderWithI18n(<MetaToolApprovalDialog />);
    await emitRequest({
      tool_name: 'mcpmux_manage_feature_set',
      summary: 'Create FeatureSet',
      diff: null,
      raw_args: {},
      affects_other_clients: false,
    });
    expect(screen.getByTestId('meta-tool-approval-dialog')).toBeInTheDocument();
    expect(screen.queryByTestId('meta-tool-approval-space')).toBeNull();
  });

  it('renders a freeform { added_tools } diff without crashing', async () => {
    renderWithI18n(<MetaToolApprovalDialog />);
    await emitRequest({
      tool_name: 'mcpmux_manage_feature_set',
      summary: 'Create FeatureSet',
      diff: { added_tools: ['github_create_issue', 'slack_send'] },
      raw_args: {},
      affects_other_clients: false,
    });
    expect(screen.getByTestId('meta-tool-approval-dialog')).toBeInTheDocument();
    expect(screen.getByText(/github_create_issue/)).toBeInTheDocument();
    expect(screen.getByText(/slack_send/)).toBeInTheDocument();
  });

  it('"Manage approval prompts" denies the request and routes to the Built-in tab', async () => {
    const user = userEvent.setup();
    renderWithI18n(<MetaToolApprovalDialog />);
    await emitRequest({
      tool_name: 'mcpmux_bind_current_workspace',
      summary: 'Bind this folder',
      diff: null,
      raw_args: {},
      affects_other_clients: false,
    });

    await user.click(await screen.findByTestId('meta-tool-approval-manage-link'));

    expect(respondToMetaToolApprovalMock).toHaveBeenCalledWith(
      'req-1',
      'client-1',
      'mcpmux_bind_current_workspace',
      'deny'
    );
    expect(navigateToSpy).toHaveBeenCalledWith('builtin-servers');
  });
});
