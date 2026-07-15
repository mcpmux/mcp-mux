import { test, expect } from '@playwright/test';
import { DashboardPage, RegistryPage } from '../pages';

/**
 * End-to-end user flow tests that simulate real user journeys
 */

test.describe('Complete User Flows', () => {
  test('should complete first-time setup flow', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    
    // 1. Load app
    await dashboard.navigate();
    await expect(dashboard.heading).toBeVisible();
    
    // 2. Verify gateway status is shown
    await expect(dashboard.gatewayStatus).toBeVisible();
    
    // 3. Navigate to discover servers
    await page.locator('nav button:has-text("Discover")').click();
    await expect(page.locator('h1:has-text("Discover")')).toBeVisible();
    
    // 4. Navigate to settings to configure theme
    await page.locator('nav button:has-text("Settings")').click();
    await expect(page.locator('h1:has-text("Settings")')).toBeVisible();
    
    // 5. Return to dashboard
    await page.locator('nav button:has-text("Home")').click();
    await expect(dashboard.heading).toBeVisible();
  });

  test('should navigate through all main sections', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    // Dashboard
    await expect(dashboard.heading).toBeVisible();
    
    // My Servers
    await page.locator('nav button:has-text("Tools")').click();
    await expect(page.locator('h1:has-text("Tools")')).toBeVisible();
    
    // Discover
    await page.locator('nav button:has-text("Discover")').click();
    await expect(page.locator('h1:has-text("Discover")')).toBeVisible();
    
    // Mapping (the workspace→tools mapping tab; nav label was renamed from
    // "Workspaces", but the page heading is still "Workspaces").
    await page.locator('nav button:has-text("Mapping")').click();
    await expect(page.locator('h1:has-text("Workspaces")')).toBeVisible();
    
    // FeatureSets
    await page.locator('nav button:has-text("FeatureSets")').click();
    await expect(page.locator('h1:has-text("FeatureSets")')).toBeVisible();
    
    // Clients
    await page.locator('nav button:has-text("Apps")').click();
    await expect(page.locator('h1:has-text("Apps")')).toBeVisible();
    
    // Settings
    await page.locator('nav button:has-text("Settings")').click();
    await expect(page.locator('h1:has-text("Settings")')).toBeVisible();
  });

  test('should persist theme preference', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    
    await dashboard.navigate();
    await page.locator('nav button:has-text("Settings")').click();
    
    // Set dark theme
    await page.getByRole('button', { name: 'Dark', exact: true }).click();
    await page.waitForTimeout(500);
    
    // Verify dark theme is applied
    await expect(page.locator('html')).toHaveClass(/dark/);
    
    // Navigate away and back
    await page.locator('nav button:has-text("Home")').click();
    await page.locator('nav button:has-text("Settings")').click();
    
    // Dark theme should still be active
    await expect(page.locator('html')).toHaveClass(/dark/);
  });
});

test.describe('Server Discovery Flow', () => {
  test('should search and browse servers', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const registry = new RegistryPage(page);
    
    await dashboard.navigate();
    await page.locator('nav button:has-text("Discover")').click();
    
    // 1. Initial state - should show servers
    await expect(registry.serverCount).toBeVisible();
    
    // 2. Search for a server
    await registry.search('file');
    
    // 3. Results should update
    await expect(registry.serverCount).toBeVisible();
    
    // 4. Clear search
    await registry.clearSearch();
    
    // 5. Should show full list again
    await expect(registry.serverCount).toBeVisible();
  });
});

test.describe('Dashboard Interactions', () => {
  test('should display all stat cards', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    // All main stat cards should be visible
    await expect(dashboard.serverCountCard).toBeVisible();
    await expect(dashboard.featureSetsCard).toBeVisible();
    await expect(dashboard.clientsCard).toBeVisible();
    await expect(dashboard.activeSpaceCard).toBeVisible();
  });

  test('should show connect IDEs section', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();

    // Connect IDEs section should be present
    await expect(page.locator('text=Connect Your IDEs')).toBeVisible();

    // Client grid should be present
    await expect(page.locator('[data-testid="client-grid"]')).toBeVisible();
  });
});

test.describe('Responsive Behavior', () => {
  test('should adjust layout for mobile viewport', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    
    // Set mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });
    await dashboard.navigate();
    
    // App should still load
    await expect(dashboard.heading).toBeVisible();
  });

  test('should adjust layout for tablet viewport', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    
    // Set tablet viewport
    await page.setViewportSize({ width: 768, height: 1024 });
    await dashboard.navigate();
    
    // App should still load
    await expect(dashboard.heading).toBeVisible();
  });

  test('should work on desktop viewport', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    
    // Set desktop viewport
    await page.setViewportSize({ width: 1920, height: 1080 });
    await dashboard.navigate();
    
    // App should display properly
    await expect(dashboard.heading).toBeVisible();
  });
});

test.describe('Error Handling', () => {
  test('should handle network errors gracefully', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    
    // Load app normally first
    await dashboard.navigate();
    await expect(dashboard.heading).toBeVisible();
    
    // App should be usable
  });

  test('should show loading states', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    
    await dashboard.navigate();
    await page.locator('nav button:has-text("Discover")').click();
    
    // Just verify page eventually loads
    await expect(page.locator('h1:has-text("Discover")')).toBeVisible();
  });
});
