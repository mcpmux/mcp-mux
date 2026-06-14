/**
 * Approval dialog wiring:
 *  - renders the target-Space chip (so a cross-Space write is obvious), and
 *  - survives a freeform `diff` shape (`{ added_tools }`) without crashing —
 *    a regression guard for the earlier "Cannot read properties of undefined
 *    (reading 'length')" bug.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, act } from '@testing-library/react';

const { handlers } = vi.hoisted(() => ({
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
}));

// Capture the dialog's `listen('meta-tool-approval-request', cb)` so the test
// can deliver synthetic requests.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(name, cb);
    return Promise.resolve(() => handlers.delete(name));
  }),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));

import { MetaToolApprovalDialog } from '@/features/metaTools/MetaToolApprovalDialog';

async function emitRequest(payload: Record<string, unknown>) {
  await act(async () => {
    handlers.get('meta-tool-approval-request')?.({
      payload: {
        request_id: 'req-1',
        client_id: 'client-1',
        payload,
        expires_at_unix_secs: 9_999_999_999,
      },
    });
    await Promise.resolve();
  });
}

describe('MetaToolApprovalDialog', () => {
  beforeEach(() => handlers.clear());

  it('names the target Space when present', async () => {
    render(<MetaToolApprovalDialog />);
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
    render(<MetaToolApprovalDialog />);
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
    render(<MetaToolApprovalDialog />);
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
});
