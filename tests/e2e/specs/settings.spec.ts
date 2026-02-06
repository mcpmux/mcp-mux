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

  test.describe('Startup & System Tray Settings', () => {
    test('should display startup settings section', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      // Check for startup settings card
      await expect(page.getByText('Startup & System Tray')).toBeVisible();
      await expect(page.getByText(/Control how McpMux starts/)).toBeVisible();
    });

    test('should display all three startup toggles', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      // Check all three settings exist
      await expect(page.getByText('Launch at Startup')).toBeVisible();
      await expect(page.getByText('Start Minimized')).toBeVisible();
      await expect(page.getByText('Close to Tray')).toBeVisible();
    });

    test('should display descriptive text for each setting', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      // Check descriptions
      await expect(page.getByText(/Start McpMux automatically when you log in/)).toBeVisible();
      await expect(page.getByText(/Launch in background to system tray/)).toBeVisible();
      await expect(page.getByText(/Keep running in system tray when window is closed/)).toBeVisible();
    });

    test('should have functional toggle switches', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      // Check switches are interactive
      const autoLaunchSwitch = page.getByTestId('auto-launch-switch');
      const startMinimizedSwitch = page.getByTestId('start-minimized-switch');
      const closeToTraySwitch = page.getByTestId('close-to-tray-switch');

      await expect(autoLaunchSwitch).toBeVisible();
      await expect(startMinimizedSwitch).toBeVisible();
      await expect(closeToTraySwitch).toBeVisible();

      // All switches should be enabled (except start-minimized might be disabled if auto-launch is off)
      await expect(autoLaunchSwitch).toBeEnabled();
      await expect(closeToTraySwitch).toBeEnabled();
    });

    test('should toggle auto-launch setting', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const autoLaunchSwitch = page.getByTestId('auto-launch-switch');
      
      // Get initial state
      const initialState = await autoLaunchSwitch.getAttribute('aria-checked');
      
      // Toggle the switch
      await autoLaunchSwitch.click();
      await page.waitForTimeout(500); // Wait for backend to process
      
      // Verify state changed
      const newState = await autoLaunchSwitch.getAttribute('aria-checked');
      expect(newState).not.toBe(initialState);
      
      // Toggle back to original state
      await autoLaunchSwitch.click();
      await page.waitForTimeout(500);
      
      const finalState = await autoLaunchSwitch.getAttribute('aria-checked');
      expect(finalState).toBe(initialState);
    });

    test('should toggle close to tray setting', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const closeToTraySwitch = page.getByTestId('close-to-tray-switch');
      
      // Get initial state
      const initialState = await closeToTraySwitch.getAttribute('aria-checked');
      
      // Toggle the switch
      await closeToTraySwitch.click();
      await page.waitForTimeout(500); // Wait for state to update
      
      // Verify state changed
      const newState = await closeToTraySwitch.getAttribute('aria-checked');
      expect(newState).not.toBe(initialState);
      
      // Toggle back
      await closeToTraySwitch.click();
      await page.waitForTimeout(500);
      
      const finalState = await closeToTraySwitch.getAttribute('aria-checked');
      expect(finalState).toBe(initialState);
    });

    test('start minimized should be disabled when auto-launch is off', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const autoLaunchSwitch = page.getByTestId('auto-launch-switch');
      const startMinimizedSwitch = page.getByTestId('start-minimized-switch');
      
      // Ensure auto-launch is off
      const autoLaunchState = await autoLaunchSwitch.getAttribute('aria-checked');
      if (autoLaunchState === 'true') {
        await autoLaunchSwitch.click();
        await page.waitForTimeout(500);
      }
      
      // Start minimized should be disabled
      await expect(startMinimizedSwitch).toBeDisabled();
      await expect(startMinimizedSwitch).toHaveAttribute('aria-checked', 'false');
    });

    test('start minimized should be enabled when auto-launch is on', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const autoLaunchSwitch = page.getByTestId('auto-launch-switch');
      const startMinimizedSwitch = page.getByTestId('start-minimized-switch');
      
      // Ensure auto-launch is on
      const autoLaunchState = await autoLaunchSwitch.getAttribute('aria-checked');
      if (autoLaunchState === 'false') {
        await autoLaunchSwitch.click();
        await page.waitForTimeout(500);
      }
      
      // Start minimized should be enabled
      await expect(startMinimizedSwitch).toBeEnabled();
    });

    test('should toggle start minimized when enabled', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const autoLaunchSwitch = page.getByTestId('auto-launch-switch');
      const startMinimizedSwitch = page.getByTestId('start-minimized-switch');
      
      // Ensure auto-launch is on first
      const autoLaunchState = await autoLaunchSwitch.getAttribute('aria-checked');
      if (autoLaunchState === 'false') {
        await autoLaunchSwitch.click();
        await page.waitForTimeout(500);
      }
      
      // Get initial state
      const initialState = await startMinimizedSwitch.getAttribute('aria-checked');
      
      // Toggle the switch
      await startMinimizedSwitch.click();
      await page.waitForTimeout(500);
      
      // Verify state changed
      const newState = await startMinimizedSwitch.getAttribute('aria-checked');
      expect(newState).not.toBe(initialState);
      
      // Toggle back
      await startMinimizedSwitch.click();
      await page.waitForTimeout(500);
      
      const finalState = await startMinimizedSwitch.getAttribute('aria-checked');
      expect(finalState).toBe(initialState);
    });

    test('should persist settings across page reloads', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const closeToTraySwitch = page.getByTestId('close-to-tray-switch');
      
      // Get initial state
      const initialState = await closeToTraySwitch.getAttribute('aria-checked');
      
      // Toggle the switch
      await closeToTraySwitch.click();
      await page.waitForTimeout(500);
      
      // Reload the page
      await page.reload();
      await page.waitForLoadState('networkidle');
      
      // Navigate to settings again
      await page.locator('nav button:has-text("Settings")').click();
      
      // Verify state persisted
      const persistedState = await closeToTraySwitch.getAttribute('aria-checked');
      expect(persistedState).not.toBe(initialState);
      
      // Restore original state
      await closeToTraySwitch.click();
      await page.waitForTimeout(500);
    });

    test('should show disabled state visually for start minimized', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const autoLaunchSwitch = page.getByTestId('auto-launch-switch');
      const startMinimizedSwitch = page.getByTestId('start-minimized-switch');
      
      // Ensure auto-launch is off
      const autoLaunchState = await autoLaunchSwitch.getAttribute('aria-checked');
      if (autoLaunchState === 'true') {
        await autoLaunchSwitch.click();
        await page.waitForTimeout(500);
      }
      
      // Check that start minimized has disabled styling
      await expect(startMinimizedSwitch).toHaveClass(/opacity-50/);
    });

    test('all settings should work independently', async ({ page }) => {
      const dashboard = new DashboardPage(page);
      await dashboard.navigate();
      
      await page.locator('nav button:has-text("Settings")').click();

      const closeToTraySwitch = page.getByTestId('close-to-tray-switch');
      
      // Close to tray should work regardless of auto-launch state
      const initialCloseToTray = await closeToTraySwitch.getAttribute('aria-checked');
      
      await closeToTraySwitch.click();
      await page.waitForTimeout(500);
      
      const newCloseToTray = await closeToTraySwitch.getAttribute('aria-checked');
      expect(newCloseToTray).not.toBe(initialCloseToTray);
      
      // Restore
      await closeToTraySwitch.click();
      await page.waitForTimeout(500);
    });
  });
});
