/**
 * E2E Tests: Workspaces page.
 *
 * A WorkspaceBinding maps a normalized filesystem path to a concrete
 * (space_id, feature_set_id) pair. Roots are globally unique. These specs
 * cover the CRUD path plus the UI shell.
 *
 * Uses data-testid only (ADR-003).
 */

import { byTestId, safeClick, TIMEOUT } from '../helpers/selectors';
import {
  createWorkspaceBinding,
  deleteWorkspaceBinding,
  getActiveSpace,
  listFeatureSetsBySpace,
  listWorkspaceBindings,
  type WorkspaceBinding,
} from '../helpers/tauri-api';

function uniqueRoot(): string {
  const stamp = Date.now();
  return process.platform === 'win32'
    ? `d:\\tmp\\mcpmux-e2e-${stamp}`
    : `/tmp/mcpmux-e2e-${stamp}`;
}

describe('Workspaces - Page shell', () => {
  before(async () => {
    // Clean any leftover e2e bindings so the empty-state / populated-state
    // assertions are deterministic across reruns.
    const existing = await listWorkspaceBindings();
    for (const b of existing.filter((x) => x.workspace_root.includes('mcpmux-e2e'))) {
      await deleteWorkspaceBinding(b.id);
    }
  });

  it('TC-WS-001: Navigate to Workspaces page and see heading', async () => {
    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(1500);

    await browser.saveScreenshot('./tests/e2e/screenshots/ws-01-page.png');

    const src = await browser.getPageSource();
    expect(src.includes('Workspaces')).toBe(true);

    const createBtn = await byTestId('workspace-binding-create-toggle');
    expect(await createBtn.isDisplayed()).toBe(true);
  });
});

describe('Workspaces - Create, render, delete', () => {
  let bindingId: string | null = null;
  let spaceId = '';
  let featureSetId = '';
  const root = uniqueRoot();

  before(async () => {
    const active = await getActiveSpace();
    if (!active) throw new Error('No active space — cannot set up test');
    spaceId = active.id;
    const fsList = await listFeatureSetsBySpace(spaceId);
    const defaultFs = fsList.find((fs) => fs.feature_set_type === 'default');
    if (!defaultFs) throw new Error('No Default FS in active space');
    featureSetId = defaultFs.id;
  });

  it('TC-WS-002: Create binding pointing at the active space default FS', async () => {
    const created: WorkspaceBinding = await createWorkspaceBinding({
      workspace_root: root,
      space_id: spaceId,
      feature_set_id: featureSetId,
    });
    bindingId = created.id;

    expect(created.workspace_root.toLowerCase().endsWith(root.toLowerCase())).toBe(true);
    expect(created.space_id).toBe(spaceId);
    expect(created.feature_set_id).toBe(featureSetId);
  });

  it('TC-WS-003: Binding row renders on the Workspaces page', async () => {
    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(1500);

    // Brief nav-away-and-back to force a data reload.
    const dashBtn = await byTestId('nav-dashboard');
    await safeClick(dashBtn);
    await browser.pause(300);
    await safeClick(nav);
    await browser.pause(1500);

    await browser.saveScreenshot('./tests/e2e/screenshots/ws-02-populated.png');

    if (bindingId) {
      const row = await $(`[data-testid="workspace-binding-row-${bindingId}"]`);
      await row.waitForDisplayed({ timeout: TIMEOUT.short });
      expect(await row.isDisplayed()).toBe(true);
    }
  });

  it('TC-WS-004: Binding row references the target Space + FS by name', async () => {
    const src = await browser.getPageSource();
    // The row's footer shows "Routes to <FS> in <Space>" — check the Space
    // name is present. FS is "Default" (builtin) which may also appear in
    // unrelated copy, so we only assert on the Space name for stability.
    const active = await getActiveSpace();
    expect(src.includes(active?.name ?? '__never__')).toBe(true);
  });

  it('TC-WS-005: Delete binding and row disappears', async () => {
    if (!bindingId) throw new Error('bindingId missing — TC-WS-002 must succeed first');
    await deleteWorkspaceBinding(bindingId);

    const dash = await byTestId('nav-dashboard');
    await safeClick(dash);
    await browser.pause(300);
    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(1500);

    const rows = await $$(`[data-testid="workspace-binding-row-${bindingId}"]`);
    expect(rows.length).toBe(0);
    bindingId = null;
  });

  after(async () => {
    if (bindingId) {
      try {
        await deleteWorkspaceBinding(bindingId);
      } catch {
        /* ignore */
      }
    }
  });
});

describe('Workspaces - Create form flow (UI)', () => {
  let bindingId: string | null = null;

  it('TC-WS-006: Create binding through the form and see it listed', async () => {
    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(1000);

    const toggle = await byTestId('workspace-binding-create-toggle');
    await safeClick(toggle);
    await browser.pause(400);

    const rootInput = await byTestId('workspace-binding-root-input');
    const root = uniqueRoot();
    await rootInput.setValue(root);

    // `space` and `fs` default to the active space + its Default FS, so we
    // can submit without touching the pickers.
    const submit = await byTestId('workspace-binding-submit');
    await safeClick(submit);
    await browser.pause(800);

    const created = (await listWorkspaceBindings()).find(
      (b) => b.workspace_root.toLowerCase().endsWith(root.toLowerCase())
    );
    expect(created).toBeTruthy();
    if (created) {
      bindingId = created.id;
      const row = await $(`[data-testid="workspace-binding-row-${created.id}"]`);
      await row.waitForDisplayed({ timeout: TIMEOUT.short });
      expect(await row.isDisplayed()).toBe(true);
    }

    await browser.saveScreenshot('./tests/e2e/screenshots/ws-04-created-via-form.png');
  });

  after(async () => {
    if (bindingId) {
      try {
        await deleteWorkspaceBinding(bindingId);
      } catch {
        /* ignore */
      }
    }
  });
});
