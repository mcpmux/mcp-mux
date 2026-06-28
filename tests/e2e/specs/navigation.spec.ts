import { test, expect } from '@playwright/test';
import { DashboardPage } from '../pages';

test.describe('Navigation', () => {
  test('should load the dashboard on startup', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();

    await expect(dashboard.heading).toBeVisible();
    await expect(page.locator('text=Your AI control plane')).toBeVisible();
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
    await page.locator('nav button:has-text("Tools")').click({ force: true });
    await expect(page.locator('h1:has-text("Tools")')).toBeVisible();

    // Discover
    await page.locator('nav button:has-text("Discover")').click({ force: true });
    await expect(page.locator('h1:has-text("Discover")')).toBeVisible();

    // Mapping (the workspace→tools mapping tab; nav label was renamed from
    // "Workspaces", but the page heading is still "Workspaces").
    await page.locator('nav button:has-text("Mapping")').click({ force: true });
    await expect(page.locator('h1:has-text("Workspaces")')).toBeVisible();

    // FeatureSets
    await page.locator('nav button:has-text("FeatureSets")').click({ force: true });
    await expect(page.locator('h1:has-text("FeatureSets")')).toBeVisible();

    // Clients
    await page.locator('nav button:has-text("Apps")').click({ force: true });
    await expect(page.locator('h1:has-text("Apps")')).toBeVisible();

    // Settings
    await page.locator('nav button:has-text("Settings")').click({ force: true });
    await expect(page.locator('h1:has-text("Settings")')).toBeVisible();

    // Back to Dashboard
    await page.locator('nav button:has-text("Home")').click({ force: true });
    await expect(dashboard.heading).toBeVisible();
  });
});
