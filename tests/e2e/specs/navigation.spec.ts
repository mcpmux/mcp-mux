import { test, expect } from '@playwright/test';
import { DashboardPage } from '../pages';

test.describe('Navigation', () => {
  test('should load the dashboard on startup', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();

    await expect(dashboard.heading).toBeVisible();
    await expect(page.locator('text=Welcome to McpMux')).toBeVisible();
  });

  test('should navigate to settings page', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("Settings")').click({ force: true });
    await expect(page.locator('h1:has-text("Settings")')).toBeVisible();
  });

  test('should navigate to all main pages', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();

    // My Servers
    await page.locator('nav button:has-text("My Servers")').click({ force: true });
    await expect(page.locator('h1:has-text("My Servers")')).toBeVisible();

    // Discover
    await page.locator('nav button:has-text("Search")').click({ force: true });
    await expect(page.locator('h1:has-text("Discover Servers")')).toBeVisible();

    // Spaces (use last() to avoid space switcher)
    await page.locator('nav button:has-text("Spaces")').last().click({ force: true });
    await expect(page.locator('h1:has-text("Workspaces")')).toBeVisible();

    // FeatureSets
    await page.locator('nav button:has-text("Bundles")').click({ force: true });
    await expect(page.locator('h1:has-text("Bundles")')).toBeVisible();

    // Clients
    await page.locator('nav button:has-text("Clients")').click({ force: true });
    await expect(page.locator('h1:has-text("Connections")')).toBeVisible();

    // Settings
    await page.locator('nav button:has-text("Settings")').click({ force: true });
    await expect(page.locator('h1:has-text("Settings")')).toBeVisible();

    // Back to Dashboard
    await page.locator('nav button:has-text("Dashboard")').click({ force: true });
    await expect(dashboard.heading).toBeVisible();
  });
});
