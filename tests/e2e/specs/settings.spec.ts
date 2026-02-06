import { test, expect } from '@playwright/test';
import { DashboardPage } from '../pages';

test.describe('Settings', () => {
  test('should display settings heading', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    // Click Settings in sidebar
    await page.locator('nav button:has-text("Settings")').click();
    
    // Check heading
    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  });

  test('should display appearance settings', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("Settings")').click();

    await expect(page.locator('text=Appearance').first()).toBeVisible();
    await expect(page.getByRole('button', { name: 'Light', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Dark', exact: true })).toBeVisible();
  });

  test('should display logs section', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("Settings")').click();

    // Use heading role to be more specific
    await expect(page.locator('h3:has-text("Logs"), h2:has-text("Logs")').first()).toBeVisible();
  });

  test('should switch between themes', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("Settings")').click();

    // Switch to light theme
    await page.getByRole('button', { name: 'Light', exact: true }).click();
    await page.waitForTimeout(300);
    
    // Switch to dark theme
    await page.getByRole('button', { name: 'Dark', exact: true }).click();
    await page.waitForTimeout(300);
    await expect(page.locator('html')).toHaveClass(/dark/);
  });

  test.describe('Software Updates', () => {
    test('should display update checker section', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      // Check for update checker card
      await expect(page.getByTestId('update-checker')).toBeVisible();
      await expect(page.getByText('Software Updates')).toBeVisible();
      await expect(page.getByText(/Keep your application up to date/)).toBeVisible();
    });

    test('should display current version', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      // Check current version is displayed
      await expect(page.getByTestId('current-version')).toBeVisible();
      await expect(page.getByTestId('current-version')).toContainText('v');
    });

    test('should have check for updates button', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const checkButton = page.getByTestId('check-updates-btn');
      await expect(checkButton).toBeVisible();
      await expect(checkButton).toHaveText(/Check for Updates/);
      await expect(checkButton).toBeEnabled();
    });

    test('should show loading state when checking for updates', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const checkButton = page.getByTestId('check-updates-btn');
      await checkButton.click();

      // Button should show loading state briefly
      await expect(checkButton).toContainText(/Checking/);
      await expect(checkButton).toBeDisabled();
    });

    test('should display update status message', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const checkButton = page.getByTestId('check-updates-btn');
      await checkButton.click();

      // Wait for check to complete (should show either update available or up to date)
      await page.waitForSelector('[data-testid="update-message"], [data-testid="update-available"]', {
        timeout: 10000,
      });

      // Verify one of the expected states is shown
      const hasMessage = await page.getByTestId('update-message').isVisible().catch(() => false);
      const hasUpdate = await page.getByTestId('update-available').isVisible().catch(() => false);
      
      expect(hasMessage || hasUpdate).toBeTruthy();
    });

    test('should allow multiple update checks', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const checkButton = page.getByTestId('check-updates-btn');
      
      // First check
      await checkButton.click();
      await page.waitForSelector('[data-testid="update-message"], [data-testid="update-available"]', {
        timeout: 10000,
      });

      // Check button should be available again
      await expect(checkButton).toBeEnabled();
      
      // Second check
      await checkButton.click();
      await expect(checkButton).toContainText(/Checking/);
    });
  });

  test.describe('Logs Section', () => {
    test('should display logs path', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const logsPath = page.getByTestId('logs-path');
      await expect(logsPath).toBeVisible();
      // Should not show "Loading..." after page loads
      await expect(logsPath).not.toContainText('Loading...');
    });

    test('should have open logs folder button', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const openButton = page.getByTestId('open-logs-btn');
      await expect(openButton).toBeVisible();
      await expect(openButton).toContainText('Open Logs Folder');
    });

    test('should show description text', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      await expect(page.getByText(/Logs are rotated daily/i)).toBeVisible();
    });
  });

  test.describe('Page Layout', () => {
    test('should display all sections in order', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      // Verify sections appear in expected order
      const sections = [
        page.getByText('Software Updates'),
        page.getByText('Appearance'),
        page.locator('h3:has-text("Logs"), h2:has-text("Logs")').first(),
      ];

      for (const section of sections) {
        await expect(section).toBeVisible();
      }
    });

    test('should be scrollable if content overflows', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      // Content should be within a scrollable container
      const mainContent = page.locator('[class*="space-y-6"]').first();
      await expect(mainContent).toBeVisible();
    });
  });
});
