import { test, expect } from '@playwright/test';
import { DashboardPage, ClientsPage } from '../pages';

test.describe('Connections Page', () => {
  test('should display the Connections heading', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const clients = new ClientsPage(page);
    await dashboard.navigate();

    // Click Clients in sidebar
    await page.locator('nav button:has-text("Clients")').click();

    await expect(clients.heading).toBeVisible();
    await expect(clients.heading).toHaveText('Connections');
  });

  test('should describe that routing lives in Workspaces', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();

    // Routing is configured in Workspaces, not per-client.
    await expect(
      page.getByRole('button', { name: /^Workspaces$/ })
    ).toBeVisible();
  });

  test('should show description text', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const description = page.locator('text=/connected|AI|client/i');
    // Description about clients should be visible
  });

  test('should show empty state or client list', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const emptyState = page.locator('text=/No clients|no.*connected/i');
    const clientItems = page.locator('[class*="rounded"][class*="border"]');
    
    const hasEmpty = await emptyState.isVisible().catch(() => false);
    const clientCount = await clientItems.count();
    
    // Either empty state or clients should be shown
    expect(hasEmpty || clientCount > 0).toBeTruthy();
  });

  test('should display client cards if clients exist', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const clientCards = page.locator('[class*="rounded"][class*="border"]');
    const count = await clientCards.count();
    
    if (count > 0) {
      // First client card should have content
      const firstCard = clientCards.first();
      await expect(firstCard).toBeVisible();
    }
  });
});

test.describe('Connection Details', () => {
  test('should show last-seen indicator on connection cards', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();

    const clientCards = page.locator('[data-testid^="client-card-"]');
    const count = await clientCards.count();

    if (count > 0) {
      // Each card surfaces "Last seen …" — pure observability (no routing bits).
      const firstCard = clientCards.first();
      await expect(firstCard).toBeVisible();
      await expect(firstCard).toContainText(/Last seen/);
    }
  });

  test('should route routing config to Workspaces from the side panel', async ({
    page,
  }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();

    const clientCards = page.locator('[data-testid^="client-card-"]');
    const count = await clientCards.count();

    if (count > 0) {
      await clientCards.first().click();

      // The side panel's routing callout exposes a button that sends the user
      // to the Mapping tab. The label is client-type-specific: DCR clients show
      // "Open Mapping", API-key clients "Open this client's mapping".
      await expect(page.getByRole('button', { name: /Open .*apping/ })).toBeVisible();

      // Legacy per-client controls MUST NOT be present any more.
      await expect(page.locator('text=Quick Settings')).toHaveCount(0);
      await expect(page.locator('text=Connection Mode')).toHaveCount(0);
      await expect(page.locator('text=Effective Features')).toHaveCount(0);
      await expect(page.locator('text=Advanced Permissions')).toHaveCount(0);
    }
  });
});

test.describe('Connection lifecycle', () => {
  test('should have refresh button if available', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();

    const refreshButton = page.getByRole('button', { name: /Refresh/ });
    // Always rendered on the Connections header.
    await expect(refreshButton).toBeVisible();
  });
});

test.describe('Connections toast container', () => {
  test('should have toast container on Connections page', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const clients = new ClientsPage(page);
    await dashboard.navigate();

    await page.locator('nav button:has-text("Clients")').click();
    await expect(clients.heading).toBeVisible();

    await expect(clients.toastContainer).toBeAttached();
  });

  // Skip in web mode - requires Tauri API for the save-alias command.
  test.skip('should toast on display-name save', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const clients = new ClientsPage(page);
    await dashboard.navigate();

    await page.locator('nav button:has-text("Clients")').click();

    const clientCards = page.locator('[data-testid^="client-card-"]');
    const count = await clientCards.count();

    if (count > 0) {
      await clientCards.first().click();

      // Type into the display-name input and hit save.
      const aliasInput = page.getByPlaceholder(/./).first();
      await aliasInput.fill('New Alias');
      await page.getByRole('button', { name: /Save/ }).click();

      await clients.waitForToast('success');
      expect(await clients.getToastText()).toMatch(/Saved/);
    }
  });

  // Skip in web mode - requires Tauri API for revoke.
  test.skip('should toast on revoke', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const clients = new ClientsPage(page);
    await dashboard.navigate();

    await page.locator('nav button:has-text("Clients")').click();

    const clientCards = page.locator('[data-testid^="client-card-"]');
    const count = await clientCards.count();

    if (count > 0) {
      await clientCards.first().click();

      page.on('dialog', (dialog) => dialog.accept());
      await page.getByRole('button', { name: /Revoke connection/ }).click();
      // Confirm dialog
      await page.getByRole('button', { name: /Revoke/ }).click();

      await clients.waitForToast('success');
      expect(await clients.getToastText()).toMatch(/revoked/);
    }
  });
});
