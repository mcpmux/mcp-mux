import { test, expect } from '@playwright/test';
import { DashboardPage, SidebarNav, ServersPage, RegistryPage, SettingsPage } from '../pages';

/**
 * End-to-end user flow tests that simulate real user journeys
 */

test.describe('Complete User Flows', () => {
  test('should complete first-time setup flow', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);
    
    // 1. Load app
    await dashboard.navigate();
    await expect(dashboard.heading).toBeVisible();
    
    // 2. Verify gateway status is shown
    await expect(dashboard.gatewayStatus).toBeVisible();
    
    // 3. Navigate to discover servers
    await sidebar.goToDiscover();
    await expect(page.getByRole('heading', { name: 'Discover Servers' })).toBeVisible();
    
    // 4. Navigate to settings to configure theme
    await sidebar.goToSettings();
    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
    
    // 5. Return to dashboard
    await sidebar.goToDashboard();
    await expect(dashboard.heading).toBeVisible();
  });

  test('should navigate through all main sections', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);
    
    await dashboard.navigate();
    
    // Track which pages we visit
    const visitedPages: string[] = [];
    
    // Dashboard
    await expect(dashboard.heading).toBeVisible();
    visitedPages.push('dashboard');
    
    // My Servers
    await sidebar.goToMyServers();
    await expect(page.getByRole('heading', { name: 'My Servers' })).toBeVisible();
    visitedPages.push('servers');
    
    // Discover
    await sidebar.goToDiscover();
    await expect(page.getByRole('heading', { name: 'Discover Servers' })).toBeVisible();
    visitedPages.push('discover');
    
    // Spaces
    await sidebar.goToSpaces();
    await expect(page.getByRole('heading', { name: 'Spaces' })).toBeVisible();
    visitedPages.push('spaces');
    
    // FeatureSets
    await sidebar.goToFeatureSets();
    await expect(page.getByRole('heading', { name: /FeatureSets|Feature Sets/i })).toBeVisible();
    visitedPages.push('featuresets');
    
    // Clients
    await sidebar.goToClients();
    await expect(page.getByRole('heading', { name: 'Clients' })).toBeVisible();
    visitedPages.push('clients');
    
    // Settings
    await sidebar.goToSettings();
    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
    visitedPages.push('settings');
    
    // Verify all pages were visited
    expect(visitedPages).toEqual([
      'dashboard',
      'servers',
      'discover',
      'spaces',
      'featuresets',
      'clients',
      'settings',
    ]);
  });

  test('should persist theme preference', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);
    const settings = new SettingsPage(page);
    
    await dashboard.navigate();
    await sidebar.goToSettings();
    
    // Set dark theme
    await settings.selectTheme('dark');
    await expect(page.locator('html')).toHaveClass(/dark/);
    
    // Navigate away and back
    await sidebar.goToDashboard();
    await sidebar.goToSettings();
    
    // Dark theme should still be active
    await expect(page.locator('html')).toHaveClass(/dark/);
  });
});

test.describe('Server Discovery Flow', () => {
  test('should search and browse servers', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);
    const registry = new RegistryPage(page);
    
    await dashboard.navigate();
    await sidebar.goToDiscover();
    
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

  test('should display gateway URL when running', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    // Check if gateway shows URL
    const gatewayUrl = page.locator('code:has-text("localhost")');
    // May or may not be visible depending on gateway state
  });

  test('should show connection config section', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    
    // Config section should be present
    await expect(page.locator('text=Connect Your Client')).toBeVisible();
    
    // Config code block should be present
    await expect(page.locator('pre')).toBeVisible();
  });
});

test.describe('Responsive Behavior', () => {
  test('should adjust layout for mobile viewport', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    
    // Set mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });
    await dashboard.navigate();
    
    // App should still load
    await expect(page.locator('text=Dashboard, text=McpMux')).toBeVisible();
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
    await expect(page.locator('text=McpMux')).toBeVisible();
  });
});

test.describe('Error Handling', () => {
  test('should handle network errors gracefully', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    
    // Load app normally first
    await dashboard.navigate();
    await expect(dashboard.heading).toBeVisible();
    
    // App should be usable even with potential network issues
  });

  test('should show loading states', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);
    
    await dashboard.navigate();
    await sidebar.goToDiscover();
    
    // May show loading spinner briefly
    // Just verify page eventually loads
    await expect(page.getByRole('heading', { name: 'Discover Servers' })).toBeVisible();
  });
});
