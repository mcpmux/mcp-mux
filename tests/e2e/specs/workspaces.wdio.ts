/**
 * E2E Tests: Workspaces page.
 *
 * A WorkspaceBinding ("mapping") maps a normalized filesystem path to one or
 * more FeatureSets within a Space. Roots are globally unique. These specs
 * cover the CRUD path, the UI card render, the manual-Apply create form, and
 * duplicate-folder validation.
 *
 * Uses data-testid only (ADR-003).
 */

import { byTestId, safeClick, TIMEOUT } from '../helpers/selectors';
import {
  createWorkspaceBinding,
  deleteWorkspaceBinding,
  getDefaultSpace,
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

/** First auto-seeded ("starter"/legacy "default") FS in a space, else any. */
async function pickFeatureSet(spaceId: string): Promise<string> {
  const fsList = await listFeatureSetsBySpace(spaceId);
  const seed =
    fsList.find((fs) => fs.feature_set_type === 'starter' || fs.feature_set_type === 'default') ??
    fsList[0];
  if (!seed) throw new Error('No FeatureSet in space — cannot set up test');
  return seed.id;
}

describe('Workspaces - Page shell', () => {
  before(async () => {
    // Clean any leftover e2e bindings so the state assertions are deterministic.
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
    const space = await getDefaultSpace();
    if (!space) throw new Error('No default space — cannot set up test');
    spaceId = space.id;
    featureSetId = await pickFeatureSet(spaceId);
  });

  it('TC-WS-002: Create mapping pointing at the default space FS', async () => {
    const created: WorkspaceBinding = await createWorkspaceBinding({
      workspace_root: root,
      space_id: spaceId,
      feature_set_ids: [featureSetId],
    });
    bindingId = created.id;

    expect(created.workspace_root.toLowerCase().endsWith(root.toLowerCase())).toBe(true);
    expect(created.space_id).toBe(spaceId);
    expect(created.feature_set_ids).toContain(featureSetId);
  });

  it('TC-WS-003: Mapping card renders on the Workspaces page', async () => {
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
      const card = await $(`[data-testid="workspace-entry-${bindingId}"]`);
      await card.waitForDisplayed({ timeout: TIMEOUT.short });
      expect(await card.isDisplayed()).toBe(true);
    }
  });

  it('TC-WS-004: Card references the target Space by name', async () => {
    const src = await browser.getPageSource();
    // The card footer shows "Serves <FS> from <Space>" — check the Space name
    // is present (FS names may collide with unrelated copy).
    const space = await getDefaultSpace();
    expect(src.includes(space?.name ?? '__never__')).toBe(true);
  });

  it('TC-WS-005: Delete mapping and card disappears', async () => {
    if (!bindingId) throw new Error('bindingId missing — TC-WS-002 must succeed first');
    await deleteWorkspaceBinding(bindingId);

    const dash = await byTestId('nav-dashboard');
    await safeClick(dash);
    await browser.pause(300);
    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(1500);

    const cards = await $$(`[data-testid="workspace-entry-${bindingId}"]`);
    expect(cards.length).toBe(0);
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

describe('Workspaces - Empty mapping (no Space tools)', () => {
  let bindingId: string | null = null;
  const root = uniqueRoot();

  it('TC-WS-010: A mapping with zero FeatureSets is savable and persists', async () => {
    const space = await getDefaultSpace();
    if (!space) throw new Error('No default space — cannot set up test');

    // An empty feature_set_ids list is a deliberate "this root gets no Space
    // tools" mapping — it must persist, not be rejected.
    const created: WorkspaceBinding = await createWorkspaceBinding({
      workspace_root: root,
      space_id: space.id,
      feature_set_ids: [],
    });
    bindingId = created.id;
    expect(created.feature_set_ids.length).toBe(0);

    const reloaded = (await listWorkspaceBindings()).find((b) => b.id === created.id);
    expect(reloaded).toBeTruthy();
    expect(reloaded!.feature_set_ids.length).toBe(0);

    // Card renders for an empty mapping too.
    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(1200);
    const card = await $(`[data-testid="workspace-entry-${created.id}"]`);
    await card.waitForDisplayed({ timeout: TIMEOUT.short });
    expect(await card.isDisplayed()).toBe(true);
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

  it('TC-WS-006: Create mapping through the form and see it listed', async () => {
    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(1000);

    const toggle = await byTestId('workspace-binding-create-toggle');
    await safeClick(toggle);
    await browser.pause(400);

    const rootInput = await byTestId('workspace-binding-root-input');
    const root = uniqueRoot();
    await rootInput.setValue(root);
    // Let the debounced root validation + default FS auto-select settle.
    await browser.pause(600);

    // Space defaults to the default space + its starter FS is auto-selected,
    // so the explicit Apply ("Create mapping") can be pressed without touching
    // the pickers.
    const submit = await byTestId('workspace-binding-submit');
    await submit.waitForEnabled({ timeout: TIMEOUT.short });
    await safeClick(submit);
    await browser.pause(800);

    const created = (await listWorkspaceBindings()).find((b) =>
      b.workspace_root.toLowerCase().endsWith(root.toLowerCase())
    );
    expect(created).toBeTruthy();
    if (created) {
      bindingId = created.id;
      const card = await $(`[data-testid="workspace-entry-${created.id}"]`);
      await card.waitForDisplayed({ timeout: TIMEOUT.short });
      expect(await card.isDisplayed()).toBe(true);
    }

    await browser.saveScreenshot('./tests/e2e/screenshots/ws-04-created-via-form.png');
  });

  it('TC-WS-007: Mapping an already-mapped folder shows a duplicate error and blocks Apply', async () => {
    if (!bindingId) throw new Error('bindingId missing — TC-WS-006 must succeed first');
    const existing = (await listWorkspaceBindings()).find((b) => b.id === bindingId);
    if (!existing) throw new Error('expected the TC-WS-006 binding to still exist');

    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(800);

    const toggle = await byTestId('workspace-binding-create-toggle');
    await safeClick(toggle);
    await browser.pause(400);

    const rootInput = await byTestId('workspace-binding-root-input');
    await rootInput.setValue(existing.workspace_root);
    await browser.pause(700); // debounced validation + duplicate check

    const dupError = await byTestId('workspace-binding-duplicate-error');
    await dupError.waitForDisplayed({ timeout: TIMEOUT.short });
    expect(await dupError.isDisplayed()).toBe(true);

    const submit = await byTestId('workspace-binding-submit');
    expect(await submit.isEnabled()).toBe(false);

    await browser.saveScreenshot('./tests/e2e/screenshots/ws-05-duplicate.png');
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
