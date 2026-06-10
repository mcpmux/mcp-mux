/**
 * E2E Tests: self-management `mcpmux_*` meta tools.
 *
 * Covers the user-visible approval flow end-to-end:
 *   * the master switch round-trips through the SettingsPage
 *   * the approval dialog renders when the gateway emits a request event
 *   * the Allow/Deny buttons call respond_to_meta_tool_approval
 *   * the grants panel + audit log render without a live gateway
 *
 * The gateway's internal state machine is covered by the Rust integration
 * tests; here we verify the Tauri bridge + React wiring actually moves
 * bytes between the two.
 */

import { byTestId, TIMEOUT, safeClick } from '../helpers/selectors';
import {
  emitEvent,
  getDefaultSpace,
  invoke,
  getMetaToolsAutoApprove,
  setMetaToolsAutoApprove,
} from '../helpers/tauri-api';

interface BuiltinServerRow {
  id: string;
  enabled: boolean;
}

describe('Built-in Servers - Tool Optimization UI', () => {
  it('TC-MT-001: Tool Optimization enablement round-trips per Space', async () => {
    const nav = await byTestId('nav-builtin-servers');
    await safeClick(nav);
    await browser.pause(1000);

    const card = await byTestId('builtin-server-tool-optimization');
    await expect(card).toBeDisplayed();

    const space = await getDefaultSpace();
    if (!space) throw new Error('No default space — cannot set up test');

    const enabledFor = async () =>
      (await invoke<BuiltinServerRow[]>('list_builtin_servers', { spaceId: space.id })).find(
        (s) => s.id === 'tool-optimization'
      )?.enabled;

    // Default: enabled for the Space (product default).
    expect(await enabledFor()).toBe(true);

    // Disable for this Space and verify.
    await invoke<void>('set_builtin_server_enabled', {
      spaceId: space.id,
      serverId: 'tool-optimization',
      enabled: false,
    });
    expect(await enabledFor()).toBe(false);

    // Restore so subsequent tests see the default.
    await invoke<void>('set_builtin_server_enabled', {
      spaceId: space.id,
      serverId: 'tool-optimization',
      enabled: true,
    });
    expect(await enabledFor()).toBe(true);
  });

  it('TC-MT-002: Grants panel + audit log render in the Built-in Servers tab', async () => {
    const nav = await byTestId('nav-builtin-servers');
    await safeClick(nav);
    await browser.pause(1000);

    const grants = await byTestId('meta-tool-grants-panel');
    const audit = await byTestId('meta-tool-audit-log');
    await expect(grants).toBeDisplayed();
    await expect(audit).toBeDisplayed();
  });

  it('TC-MT-003: DEBUG auto-approve toggle round-trips and reflects backend state', async () => {
    const nav = await byTestId('nav-builtin-servers');
    await safeClick(nav);
    await browser.pause(1000);

    // The amber debug control lives inside the grants panel.
    const toggleWrap = await byTestId('meta-tool-auto-approve');
    await expect(toggleWrap).toBeDisplayed();

    // Start from a known-off baseline (session-only state).
    await setMetaToolsAutoApprove(false);
    expect(await getMetaToolsAutoApprove()).toBe(false);

    // Flip it on via the UI switch and confirm the backend agrees.
    const toggle = await byTestId('meta-tool-auto-approve-toggle');
    await safeClick(toggle);
    await browser.waitUntil(async () => (await getMetaToolsAutoApprove()) === true, {
      timeout: TIMEOUT.medium,
      timeoutMsg: 'auto-approve did not turn on after clicking the switch',
    });

    // Flip back off so later tests see the safe default.
    await safeClick(toggle);
    await browser.waitUntil(async () => (await getMetaToolsAutoApprove()) === false, {
      timeout: TIMEOUT.medium,
      timeoutMsg: 'auto-approve did not turn off after clicking the switch',
    });
  });
});

describe('Meta tools - Approval dialog', () => {
  it('TC-MT-010: Emitting `meta-tool-approval-request` surfaces the dialog', async () => {
    // Fire a synthetic approval request from the Rust side; the dialog
    // component listens on this exact Tauri event name, no gateway needed.
    const requestId = `test-${Date.now()}`;
    await emitEvent('meta-tool-approval-request', {
      request_id: requestId,
      client_id: '00000000-0000-0000-0000-0000000000aa',
      payload: {
        tool_name: 'mcpmux_pin_this_session',
        summary: 'E2E: pin to FeatureSet "tiny" (3 tools)',
        diff: {
          before: ['github_create_issue', 'firebase_deploy', 'slack_send'],
          after: ['github_create_issue'],
          added: [],
          removed: ['firebase_deploy', 'slack_send'],
        },
        raw_args: { feature_set_id: '11111111-1111-1111-1111-111111111111' },
        affects_other_clients: false,
      },
      expires_at_unix_secs: Math.floor(Date.now() / 1000) + 60,
    });

    const dialog = await byTestId('meta-tool-approval-dialog');
    await dialog.waitForDisplayed({ timeout: TIMEOUT.medium });

    // Every button is present and clickable.
    await expect(await byTestId('meta-tool-approval-allow-once')).toBeDisplayed();
    await expect(await byTestId('meta-tool-approval-always')).toBeDisplayed();
    await expect(await byTestId('meta-tool-approval-deny')).toBeDisplayed();
  });

  it('TC-MT-011: Clicking Deny closes the dialog and records a decision', async () => {
    // Queue a fresh dialog (previous test may have left one mid-flight on
    // slow CI — wait for it to close first).
    const requestId = `test-deny-${Date.now()}`;
    await emitEvent('meta-tool-approval-request', {
      request_id: requestId,
      client_id: '00000000-0000-0000-0000-0000000000bb',
      payload: {
        tool_name: 'mcpmux_set_space_active',
        summary: 'E2E deny: change space active FS',
        diff: null,
        raw_args: {},
        affects_other_clients: true,
      },
      expires_at_unix_secs: Math.floor(Date.now() / 1000) + 60,
    });

    const dialog = await byTestId('meta-tool-approval-dialog');
    await dialog.waitForDisplayed({ timeout: TIMEOUT.medium });

    // The dialog shows the cross-client warning for this request.
    await expect(
      await byTestId('meta-tool-approval-cross-client-warning')
    ).toBeDisplayed();

    const deny = await byTestId('meta-tool-approval-deny');
    await safeClick(deny);

    // Dialog dismisses after the respond_to_meta_tool_approval round-trip.
    await dialog.waitForDisplayed({ reverse: true, timeout: TIMEOUT.medium });
  });
});
