import { test, expect } from '@playwright/test';
import { DashboardPage, RegistryPage } from '../pages';

test.describe('Post-Action User Guidance', () => {
  test.describe('My Servers empty state', () => {
    test('should show Discover MCP Servers button when no servers installed', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();

      await page.locator('nav button:has-text("My Servers")').click();
      await expect(page.getByRole('heading', { name: 'My Servers' })).toBeVisible();

      // Check for empty state with discover button
      const emptyState = page.locator('text=No servers installed');
      if (await emptyState.isVisible().catch(() => false)) {
        const discoverBtn = page.locator('[data-testid="discover-servers-btn"]');
        await expect(discoverBtn).toBeVisible();
        await expect(discoverBtn).toHaveText('Discover MCP Servers');
      }
    });

    test('should navigate to Discover page when clicking Discover button in empty state', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();

      await page.locator('nav button:has-text("My Servers")').click();

      const emptyState = page.locator('text=No servers installed');
      if (await emptyState.isVisible().catch(() => false)) {
        const discoverBtn = page.locator('[data-testid="discover-servers-btn"]');
        await discoverBtn.click();

        // Should now be on the Discover page
        await expect(page.getByRole('heading', { name: 'Discover Servers' })).toBeVisible();
      }
    });
  });

  test.describe('Registry post-install toast', () => {
    test('should have toast container on registry page for install guidance', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      const registry = new RegistryPage(page);
      await dashboard.navigate();

      await page.locator('nav button:has-text("Discover")').click();
      await expect(registry.heading).toBeVisible();

      // Toast container should exist for showing post-install guidance
      await expect(registry.toastContainer).toBeAttached();
    });

    // Skip in web mode - requires Tauri API for install
    test.skip('should show toast with Go to My Servers action after installing', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();

      await page.locator('nav button:has-text("Discover")').click();

      const installBtn = page.getByRole('button', { name: /Install/i }).first();
      if (await installBtn.isVisible()) {
        await installBtn.click();

        // Toast should appear with action button
        await expect(page.getByTestId('toast-success')).toBeVisible({ timeout: 5000 });
        await expect(page.getByTestId('toast-action')).toBeVisible();
        await expect(page.getByTestId('toast-action')).toContainText('My Servers');
      }
    });

    // Skip in web mode - requires Tauri API for install
    test.skip('should navigate to My Servers when clicking toast action after install', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();

      await page.locator('nav button:has-text("Discover")').click();

      const installBtn = page.getByRole('button', { name: /Install/i }).first();
      if (await installBtn.isVisible()) {
        await installBtn.click();

        await expect(page.getByTestId('toast-action')).toBeVisible({ timeout: 5000 });
        await page.getByTestId('toast-action').click();

        // Should navigate to My Servers
        await expect(page.getByRole('heading', { name: 'My Servers' })).toBeVisible();
      }
    });
  });

  test.describe('OAuth consent post-approval guidance', () => {
    // Skip in web mode - OAuth consent requires Tauri deep link events
    test.skip('should show success state with Manage Permissions button after approval', async ({ page }) => {
      // This test requires the OAuthConsentModal to be triggered via a deep link event
      // which is only available in the full Tauri desktop app
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();

      // After approval, the modal should show:
      // - "Client Approved" heading
      // - "Manage Permissions" button
      // - "Later" button
      const manageBtn = page.locator('[data-testid="go-to-clients-btn"]');
      await expect(manageBtn).toBeVisible();
      await expect(manageBtn).toContainText('Manage Permissions');
    });
  });
});
