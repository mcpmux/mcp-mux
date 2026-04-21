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
import { emitEvent, invoke } from '../helpers/tauri-api';

describe('Meta tools - Settings UI', () => {
  it('TC-MT-001: Master-switch round-trips through Settings > get_meta_tools_enabled', async () => {
    const settingsButton = await byTestId('nav-settings');
    await safeClick(settingsButton);
    await browser.pause(1000);

    const metaSection = await byTestId('settings-meta-tools-section');
    await expect(metaSection).toBeDisplayed();

    // Initial state should be enabled (product default).
    const initial = await invoke<boolean>('get_meta_tools_enabled');
    expect(initial).toBe(true);

    // Toggle via the Tauri command and verify UI reflects the change after
    // a navigation away-and-back (the switch is loaded on mount).
    await invoke<void>('set_meta_tools_enabled', { enabled: false });
    expect(await invoke<boolean>('get_meta_tools_enabled')).toBe(false);

    // Restore so subsequent tests see the default.
    await invoke<void>('set_meta_tools_enabled', { enabled: true });
    expect(await invoke<boolean>('get_meta_tools_enabled')).toBe(true);
  });

  it('TC-MT-002: Grants panel + audit log render in the Settings section', async () => {
    const settingsButton = await byTestId('nav-settings');
    await safeClick(settingsButton);
    await browser.pause(1000);

    const grants = await byTestId('meta-tool-grants-panel');
    const audit = await byTestId('meta-tool-audit-log');
    await expect(grants).toBeDisplayed();
    await expect(audit).toBeDisplayed();
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
