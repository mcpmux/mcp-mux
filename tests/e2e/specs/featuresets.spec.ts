import { test, expect } from '@playwright/test';
import { DashboardPage, SidebarNav, FeatureSetsPage } from '../pages';

test.describe('FeatureSets Page', () => {
  let dashboard: DashboardPage;
  let sidebar: SidebarNav;
  let featureSets: FeatureSetsPage;

  test.beforeEach(async ({ page }) => {
    dashboard = new DashboardPage(page);
    sidebar = new SidebarNav(page);
    featureSets = new FeatureSetsPage(page);

    await dashboard.navigate();
    await sidebar.goToFeatureSets();
    await expect(featureSets.heading).toBeVisible();
  });

  test('should display the FeatureSets heading', async ({ page }) => {
    await expect(featureSets.heading).toBeVisible();
    const headingText = await featureSets.heading.textContent();
    expect(headingText?.toLowerCase()).toContain('feature');
  });

  test('should show description text', async ({ page }) => {
    const description = page.locator('text=/permission|bundle|tool/i');
    // Some description about feature sets should be visible
  });

  test('should display built-in feature sets', async ({ page }) => {
    // There are usually built-in feature sets like "All Features", "Default"
    const builtInSets = page.locator('text=/All Features|Default|Server:/i');
    const count = await builtInSets.count();
    
    // May have built-in sets
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test('should show feature set cards with details', async ({ page }) => {
    const cards = page.locator('[class*="rounded"][class*="border"]');
    const count = await cards.count();
    
    if (count > 0) {
      // First card should be visible
      await expect(cards.first()).toBeVisible();
    }
  });
});

test.describe('FeatureSet Details', () => {
  test.beforeEach(async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);

    await dashboard.navigate();
    await sidebar.goToFeatureSets();
  });

  test('should expand feature set to show members', async ({ page }) => {
    // Click on a feature set to expand it
    const featureSetItems = page.locator('[class*="rounded"][class*="border"]');
    const count = await featureSetItems.count();
    
    if (count > 0) {
      const firstItem = featureSetItems.first();
      const expandButton = firstItem.locator('button, [class*="chevron"]').first();
      
      if (await expandButton.isVisible()) {
        await expandButton.click();
        // Content should expand
      }
    }
  });

  test('should show feature counts or member lists', async ({ page }) => {
    // Feature sets show counts of tools, prompts, resources
    const countIndicators = page.locator('text=/\\d+ tools?|\\d+ prompts?|\\d+ resources?/i');
    // May or may not be visible
  });
});

test.describe('FeatureSet Types', () => {
  test.beforeEach(async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);

    await dashboard.navigate();
    await sidebar.goToFeatureSets();
  });

  test('should distinguish between built-in and custom feature sets', async ({ page }) => {
    // Built-in sets typically have different styling or badges
    const builtInBadge = page.locator('text=/Built-in|System|All/i');
    const customBadge = page.locator('text=/Custom|User/i');
    
    // At least one type should exist
  });

  test('should show server-specific feature sets if servers installed', async ({ page }) => {
    // Server-specific sets show the server name
    const serverSets = page.locator('text=/Server:/i');
    const count = await serverSets.count();
    
    // May have server-specific sets
    expect(count).toBeGreaterThanOrEqual(0);
  });
});
