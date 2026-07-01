import { test, expect } from '@playwright/test';
import { DashboardPage } from '../pages';

test.describe('My Servers Page', () => {
  test('should display the My Servers heading', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    // Click My Servers in sidebar
    await page.locator('nav button:has-text("My Servers")').click();
    
    // Check heading
    await expect(page.getByRole('heading', { name: 'My Servers' })).toBeVisible();
  });

  test('should display gateway status banner', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("My Servers")').click();
    
    // Gateway status should be visible - either running or stopped
    const runningBanner = page.locator('text=Gateway Running');
    const stoppedBanner = page.locator('text=Gateway Stopped');
    const isRunning = await runningBanner.isVisible().catch(() => false);
    const isStopped = await stoppedBanner.isVisible().catch(() => false);
    expect(isRunning || isStopped).toBeTruthy();
  });

  test('should show server page content', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("My Servers")').click();
    await expect(page.getByRole('heading', { name: 'My Servers' })).toBeVisible();
    
    // Page should have content - either servers or empty state
    const hasServers = await page.locator('[class*="rounded-xl"][class*="border"]').count() > 0;
    const hasEmptyState = await page.locator('text=No servers installed').isVisible().catch(() => false);
    
    expect(hasServers || hasEmptyState).toBeTruthy();
  });

  test('should display gateway controls', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("My Servers")').click();
    
    // Should have gateway-related UI
    const gatewayText = page.locator('text=/Gateway/i');
    await expect(gatewayText.first()).toBeVisible();
  });
});

test.describe('Server Actions', () => {
  test('should show server cards if servers exist', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("My Servers")').click();
    
    const serverCards = page.locator('[class*="rounded-xl"][class*="border"]');
    const cardCount = await serverCards.count();

    if (cardCount > 0) {
      const firstCard = serverCards.first();
      await expect(firstCard).toBeVisible();
    }
  });

  test('should show buttons on server cards', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("My Servers")').click();
    
    // Look for server-specific cards (not status banners)
    const serverCards = page.locator('[data-server-card], [class*="rounded"][class*="border"]:has(button)');
    const cardCount = await serverCards.count();

    // Only assert if there are actual server cards with buttons
    if (cardCount > 0) {
      const firstCard = serverCards.first();
      const actionButtons = firstCard.locator('button');
      const buttonCount = await actionButtons.count();
      expect(buttonCount).toBeGreaterThan(0);
    }
    // If no server cards found, test passes (no servers installed)
  });
});

test.describe('Server Action Menu', () => {
  test('should show View Logs and View Definition in action menu', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();

    await page.locator('nav button:has-text("My Servers")').click();

    // Find any server card with a menu button (three-dot / MoreVertical)
    const menuButtons = page.locator('button[aria-label="More actions"]');
    const count = await menuButtons.count();

    if (count > 0) {
      await menuButtons.first().click();

      // The menu should contain View Logs and View Definition items
      await expect(page.getByRole('menuitem', { name: /View Logs/i })).toBeVisible();
      await expect(page.getByRole('menuitem', { name: /View Definition/i })).toBeVisible();
      await expect(page.getByRole('menuitem', { name: /Uninstall/i })).toBeVisible();

      // Close menu
      await page.keyboard.press('Escape');
    }
    // If no servers installed, test passes silently
  });
});

test.describe('Server Toast Notifications', () => {
  // Skip in web mode - requires Tauri API for server enable/disable
  test.skip('should show success toast on server enable', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("My Servers")').click();
    
    const enableBtn = page.getByRole('button', { name: /Enable/i }).first();
    if (await enableBtn.isVisible()) {
      await enableBtn.click();
      
      await expect(page.getByTestId('toast-success')).toBeVisible({ timeout: 5000 });
    }
  });

  // Skip in web mode - requires Tauri API for log viewer
  test.skip('should show toast when clearing server logs', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("My Servers")').click();
    
    // Open log viewer for first server (if available)
    const logButton = page.getByRole('button', { name: /Logs/i }).first();
    if (await logButton.isVisible()) {
      await logButton.click();
      
      // Accept confirmation dialog
      page.on('dialog', dialog => dialog.accept());
      
      // Click clear logs button
      const clearBtn = page.locator('button[title="Clear all logs"]');
      if (await clearBtn.isVisible()) {
        await clearBtn.click();
        
        await expect(page.getByTestId('toast-success').first()).toBeVisible({ timeout: 5000 });
        const toastText = await page.getByRole('main').getByTestId('toast-container').locator('[role="alert"]').first().textContent();
        expect(toastText).toContain('Logs cleared');
      }
    }
  });

  // Skip in web mode - requires Tauri API for log file path
  test.skip('should show toast when copying log file path', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("My Servers")').click();
    
    const logButton = page.getByRole('button', { name: /Logs/i }).first();
    if (await logButton.isVisible()) {
      await logButton.click();
      
      // Click copy path button
      const copyBtn = page.locator('button[title="Open log file in external editor"]');
      if (await copyBtn.isVisible()) {
        await copyBtn.click();
        
        await expect(page.getByTestId('toast-success').first()).toBeVisible({ timeout: 5000 });
        const toastText = await page.getByRole('main').getByTestId('toast-container').locator('[role="alert"]').first().textContent();
        expect(toastText).toContain('Path copied');
      }
    }
  });
});
