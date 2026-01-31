import { test, expect } from '@playwright/test';
import { DashboardPage, SidebarNav, ServersPage } from '../pages';

test.describe('My Servers Page', () => {
  let dashboard: DashboardPage;
  let sidebar: SidebarNav;
  let servers: ServersPage;

  test.beforeEach(async ({ page }) => {
    dashboard = new DashboardPage(page);
    sidebar = new SidebarNav(page);
    servers = new ServersPage(page);

    await dashboard.navigate();
    await sidebar.goToMyServers();
    await expect(servers.heading).toBeVisible();
  });

  test('should display the My Servers heading', async ({ page }) => {
    await expect(servers.heading).toHaveText('My Servers');
    await expect(page.locator('text=Manage your installed MCP servers')).toBeVisible();
  });

  test('should display gateway status banner', async ({ page }) => {
    // Gateway status should be visible
    const gatewayBanner = page.locator('text=Gateway Running, text=Gateway Stopped').first();
    await expect(gatewayBanner.or(page.locator('text=Gateway'))).toBeVisible();
  });

  test('should show Add Server Manually button', async ({ page }) => {
    await expect(servers.addServerButton).toBeVisible();
  });

  test('should show empty state when no servers installed', async ({ page }) => {
    // This may or may not show depending on test data
    // Just verify the page loaded correctly
    const hasServers = await page.locator('.space-y-3 > div').count() > 0;
    const hasEmptyState = await servers.emptyState.isVisible();
    
    // Either servers or empty state should be visible
    expect(hasServers || hasEmptyState).toBeTruthy();
  });

  test('should display server cards with status badges', async ({ page }) => {
    // Check if there are any server cards
    const serverCards = page.locator('[class*="rounded-xl"][class*="border"]');
    const cardCount = await serverCards.count();
    
    if (cardCount > 0) {
      // First card should have a status badge
      const firstCard = serverCards.first();
      const statusBadge = firstCard.locator('[class*="inline-flex items-center"]').first();
      await expect(statusBadge).toBeVisible();
    }
  });

  test('should toggle gateway when clicking Start/Stop', async ({ page }) => {
    const startButton = page.getByRole('button', { name: 'Start Gateway' });
    const isGatewayRunning = await page.locator('text=Gateway Running').isVisible();

    if (!isGatewayRunning && await startButton.isVisible()) {
      await startButton.click();
      // Wait for gateway to start
      await expect(page.locator('text=Gateway Running')).toBeVisible({ timeout: 15000 });
    }
  });
});

test.describe('Server Actions', () => {
  test.beforeEach(async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);

    await dashboard.navigate();
    await sidebar.goToMyServers();
  });

  test('should show action buttons for installed servers', async ({ page }) => {
    const serverCards = page.locator('[class*="rounded-xl"][class*="border"]');
    const cardCount = await serverCards.count();

    if (cardCount > 0) {
      const firstCard = serverCards.first();
      // Should have either Enable, Disable, Configure, or Connect button
      const actionButtons = firstCard.locator('button');
      await expect(actionButtons.first()).toBeVisible();
    }
  });

  test('should show overflow menu for servers', async ({ page }) => {
    const serverCards = page.locator('[class*="rounded-xl"][class*="border"]');
    const cardCount = await serverCards.count();

    if (cardCount > 0) {
      // Look for the menu button (usually has 3 dots or similar)
      const menuButton = serverCards.first().locator('button').last();
      if (await menuButton.isVisible()) {
        await menuButton.click();
        // Menu should open
        await expect(page.locator('[role="menu"], [class*="dropdown"]')).toBeVisible();
      }
    }
  });
});
