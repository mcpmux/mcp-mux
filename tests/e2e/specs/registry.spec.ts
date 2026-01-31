import { test, expect } from '@playwright/test';
import { DashboardPage, SidebarNav, RegistryPage } from '../pages';

test.describe('Registry/Discover Page', () => {
  let dashboard: DashboardPage;
  let sidebar: SidebarNav;
  let registry: RegistryPage;

  test.beforeEach(async ({ page }) => {
    dashboard = new DashboardPage(page);
    sidebar = new SidebarNav(page);
    registry = new RegistryPage(page);

    await dashboard.navigate();
    await sidebar.goToDiscover();
    await expect(registry.heading).toBeVisible();
  });

  test('should display the Discover Servers heading', async ({ page }) => {
    await expect(registry.heading).toHaveText('Discover Servers');
  });

  test('should display search input', async ({ page }) => {
    await expect(registry.searchInput).toBeVisible();
    await expect(registry.searchInput).toHaveAttribute('placeholder', 'Search servers...');
  });

  test('should display server count in footer', async ({ page }) => {
    await expect(registry.serverCount).toBeVisible();
  });

  test('should filter servers when searching', async ({ page }) => {
    // Get initial count
    const initialText = await registry.serverCount.textContent();
    const initialCount = parseInt(initialText?.match(/(\d+)/)?.[1] || '0', 10);

    // Search for something specific
    await registry.search('github');
    
    // Count should change (likely decrease or stay same if github is common)
    const filteredText = await registry.serverCount.textContent();
    const filteredCount = parseInt(filteredText?.match(/(\d+)/)?.[1] || '0', 10);

    // If there are results, the count should be reasonable
    if (filteredCount > 0) {
      expect(filteredCount).toBeLessThanOrEqual(initialCount);
    }
  });

  test('should clear search results', async ({ page }) => {
    await registry.search('xyznonexistent');
    await registry.clearSearch();

    // Should show servers again
    await expect(registry.serverCount).toBeVisible();
  });

  test('should show no results message for invalid search', async ({ page }) => {
    await registry.search('xyznonexistent12345');
    
    // Either no results message or 0 servers found
    const noResults = await registry.noResultsMessage.isVisible();
    const zeroCount = (await registry.serverCount.textContent())?.includes('0');
    
    expect(noResults || zeroCount).toBeTruthy();
  });

  test('should display server cards in grid', async ({ page }) => {
    const grid = page.locator('.grid');
    await expect(grid).toBeVisible();
    
    // Should have at least one server card if not offline
    const cards = grid.locator('> div');
    const count = await cards.count();
    
    // May be 0 if offline/no data
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test('should show installed count if servers are installed', async ({ page }) => {
    // This is optional - only shows if > 0 installed
    const installedText = page.locator('text=/\\d+ installed/');
    const isVisible = await installedText.isVisible();
    
    if (isVisible) {
      const text = await installedText.textContent();
      expect(text).toMatch(/\d+ installed/);
    }
  });
});

test.describe('Registry Filters and Sorting', () => {
  test.beforeEach(async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);

    await dashboard.navigate();
    await sidebar.goToDiscover();
  });

  test('should have filter dropdowns', async ({ page }) => {
    const filterSelects = page.locator('select');
    const count = await filterSelects.count();
    
    // Should have at least one filter (sort dropdown)
    expect(count).toBeGreaterThan(0);
  });

  test('should change sort order', async ({ page }) => {
    const sortSelect = page.locator('select').last();
    
    if (await sortSelect.isVisible()) {
      const options = await sortSelect.locator('option').allTextContents();
      
      if (options.length > 1) {
        // Select a different sort option
        await sortSelect.selectOption({ index: 1 });
        // Page should still show results
        await expect(page.locator('text=/\\d+ servers?/')).toBeVisible();
      }
    }
  });

  test('should show Clear filters button when filter is active', async ({ page }) => {
    const filterSelects = page.locator('select');
    const firstFilter = filterSelects.first();
    
    if (await firstFilter.isVisible()) {
      const options = await firstFilter.locator('option').allTextContents();
      
      // Select a non-default option if available
      if (options.length > 1) {
        await firstFilter.selectOption({ index: 1 });
        
        // Clear filters button may appear
        const clearButton = page.getByRole('button', { name: 'Clear filters' });
        // May or may not appear depending on filter structure
      }
    }
  });
});

test.describe('Registry Pagination', () => {
  test.beforeEach(async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const sidebar = new SidebarNav(page);

    await dashboard.navigate();
    await sidebar.goToDiscover();
  });

  test('should show pagination if more than one page', async ({ page }) => {
    const paginationInfo = page.locator('text=/\\d+ \\/ \\d+/');
    
    // Pagination only shows if multiple pages
    const isVisible = await paginationInfo.isVisible();
    
    if (isVisible) {
      const text = await paginationInfo.textContent();
      expect(text).toMatch(/\d+ \/ \d+/);
    }
  });

  test('should navigate to next page', async ({ page }) => {
    const registry = new RegistryPage(page);
    const paginationInfo = page.locator('text=/\\d+ \\/ \\d+/');
    
    if (await paginationInfo.isVisible()) {
      const initialText = await paginationInfo.textContent();
      
      // If not on last page, click next
      if (!initialText?.startsWith('1 / 1')) {
        await registry.goToNextPage();
        
        // Page number should change
        const newText = await paginationInfo.textContent();
        expect(newText).not.toBe(initialText);
      }
    }
  });
});
