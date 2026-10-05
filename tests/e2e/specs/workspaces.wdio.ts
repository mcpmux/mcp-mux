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

  it('TC-WS-001: Navigate to Mapping page and see heading', async () => {
    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(1500);

    await browser.saveScreenshot('./tests/e2e/screenshots/ws-01-page.png');

    const heading = await byTestId('workspaces-title');
    expect(await heading.getText()).toBe('Mapping');

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

/** Unique id/label for an id-type mapping. Contains "mcpmux-e2e" so the
 *  Page-shell before() cleanup sweeps any leftovers. */
function uniqueMappingId(): string {
  return `mcpmux-e2e-id-${Date.now()}`;
}

/** Click a control inside the setup wizard. Not safeClick: its modal wait
 *  matches the wizard's bg-black/20 backdrop, stalls ~5s, then sends Escape,
 *  which the wizard ignores. The wizard (z-50) sits above that backdrop. */
async function clickInWizard(testId: string): Promise<void> {
  const el = await byTestId(testId);
  await el.waitForEnabled({ timeout: TIMEOUT.short });
  await el.waitForClickable({ timeout: TIMEOUT.short });
  await el.click();
}

describe('Workspaces - Create wizard flow (UI)', () => {
  let bindingId: string | null = null;

  // "New mapping" opens the 3-step WorkspaceSetupWizard (key -> connect apps
  // -> tools). Its Folder step only offers the native folder dialog, which
  // WebDriver can't drive, so these specs use the wizard's "Client / ID" type.
  // It's the same wizard, the same already-mapped guard, and the same create
  // call. Folder-type creation is covered by
  // tests/ts/components/WorkspaceSetupWizard.test.tsx, which mocks the dialog.
  it('TC-WS-006: Create a mapping through the setup wizard and see it listed', async () => {
    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(1000);

    const toggle = await byTestId('workspace-binding-create-toggle');
    await safeClick(toggle);
    await (await byTestId('workspace-setup-wizard')).waitForDisplayed({ timeout: TIMEOUT.short });

    // Step 1: switch the key type to Client / ID and type the key.
    await clickInWizard('wizard-type-id');
    const idInput = await byTestId('wizard-id-input');
    await idInput.waitForDisplayed({ timeout: TIMEOUT.short });
    const key = uniqueMappingId();
    await idInput.setValue(key);

    // Step 2 (how clients connect) is informational. Step 3 (tools) defaults
    // to the default Space with its Starter FS pre-selected, so Finish is
    // enabled without touching the pickers.
    await clickInWizard('wizard-next');
    await (await byTestId('wizard-step-apps')).waitForDisplayed({ timeout: TIMEOUT.short });
    await clickInWizard('wizard-next');
    await (await byTestId('wizard-step-tools')).waitForDisplayed({ timeout: TIMEOUT.short });
    await clickInWizard('wizard-finish');
    await browser.pause(800);

    const created = (await listWorkspaceBindings()).find((b) => b.workspace_root === key);
    expect(created).toBeTruthy();
    if (created) {
      bindingId = created.id;
      const card = await $(`[data-testid="workspace-entry-${created.id}"]`);
      await card.waitForDisplayed({ timeout: TIMEOUT.short });
      expect(await card.isDisplayed()).toBe(true);
    }

    await browser.saveScreenshot('./tests/e2e/screenshots/ws-04-created-via-form.png');

    // Finish lands on the new mapping's inspector. Close it (Escape listener)
    // so the next test starts from the bare list.
    await browser.keys('Escape');
    await browser.pause(300);
  });

  it('TC-WS-007: Re-entering an already-mapped key shows the duplicate error and blocks Continue', async () => {
    if (!bindingId) throw new Error('bindingId missing — TC-WS-006 must succeed first');
    const existing = (await listWorkspaceBindings()).find((b) => b.id === bindingId);
    if (!existing) throw new Error('expected the TC-WS-006 binding to still exist');

    const nav = await byTestId('nav-workspaces');
    await safeClick(nav);
    await browser.pause(800);

    const toggle = await byTestId('workspace-binding-create-toggle');
    await safeClick(toggle);
    await (await byTestId('workspace-setup-wizard')).waitForDisplayed({ timeout: TIMEOUT.short });

    await clickInWizard('wizard-type-id');
    const idInput = await byTestId('wizard-id-input');
    await idInput.waitForDisplayed({ timeout: TIMEOUT.short });
    await idInput.setValue(existing.workspace_root);

    const dupError = await byTestId('wizard-folder-mapped-error');
    await dupError.waitForDisplayed({ timeout: TIMEOUT.short });
    expect(await dupError.isDisplayed()).toBe(true);

    // Step 1's Continue is the wizard's only way forward, so a disabled
    // Continue means no duplicate mapping can be created.
    const next = await byTestId('wizard-next');
    expect(await next.isEnabled()).toBe(false);

    await browser.saveScreenshot('./tests/e2e/screenshots/ws-05-duplicate.png');

    // Cancel (wizard-back on step 1) closes the wizard.
    await clickInWizard('wizard-back');
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
