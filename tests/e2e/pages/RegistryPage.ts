import { Page, Locator, expect } from '@playwright/test';
import { BasePage } from './BasePage';

/**
 * Discover/Registry page object for browsing and installing servers
 */
export class RegistryPage extends BasePage {
  readonly heading: Locator;
  readonly searchInput: Locator;
  readonly serverGrid: Locator;
  readonly serverCards: Locator;
  readonly noResultsMessage: Locator;
  readonly loadingSpinner: Locator;
  readonly serverCount: Locator;
  readonly installedCount: Locator;
  readonly clearFiltersButton: Locator;
  readonly offlineBadge: Locator;
  readonly paginationPrev: Locator;
  readonly paginationNext: Locator;
  readonly paginationInfo: Locator;

  constructor(page: Page) {
    super(page);
    this.heading = page.getByRole('heading', { name: 'Discover Servers' });
    this.searchInput = page.getByPlaceholder('Search servers...');
    this.serverGrid = page.locator('.grid');
    this.serverCards = page.locator('[class*="ServerCard"], .rounded-xl.border');
    this.noResultsMessage = page.locator('text=No servers found');
    this.loadingSpinner = page.locator('.animate-spin');
    this.serverCount = page.locator('text=/\\d+ servers? found/');
    this.installedCount = page.locator('text=/\\d+ installed/');
    this.clearFiltersButton = page.getByRole('button', { name: 'Clear filters' });
    this.offlineBadge = page.locator('text=Offline');
    this.paginationPrev = page.locator('button:has(path[d="M15 18l-6-6 6-6"])');
    this.paginationNext = page.locator('button:has(path[d="M9 18l6-6-6-6"])');
    this.paginationInfo = page.locator('text=/\\d+ \\/ \\d+/');
  }

  async search(query: string) {
    await this.searchInput.fill(query);
    // Wait for debounced search
    await this.page.waitForTimeout(400);
  }

  async clearSearch() {
    await this.searchInput.clear();
    await this.page.waitForTimeout(400);
  }

  async getServerCount(): Promise<number> {
    const text = await this.serverCount.textContent();
    const match = text?.match(/(\d+)/);
    return match ? parseInt(match[1], 10) : 0;
  }

  async getInstalledCount(): Promise<number> {
    const text = await this.installedCount.textContent();
    const match = text?.match(/(\d+)/);
    return match ? parseInt(match[1], 10) : 0;
  }

  async selectFilter(filterName: string, optionLabel: string) {
    // Find the filter dropdown by nearby label or first select
    const filterSelects = this.page.locator('select');
    const count = await filterSelects.count();
    
    for (let i = 0; i < count; i++) {
      const select = filterSelects.nth(i);
      const options = await select.locator('option').allTextContents();
      if (options.some(o => o.includes(optionLabel))) {
        await select.selectOption({ label: optionLabel });
        return;
      }
    }
  }

  async selectSort(sortLabel: string) {
    const sortSelect = this.page.locator('select').last();
    await sortSelect.selectOption({ label: sortLabel });
  }

  async installServer(serverName: string) {
    const serverCard = this.page.locator(`text="${serverName}"`).first().locator('xpath=ancestor::div[contains(@class, "rounded")]');
    await serverCard.getByRole('button', { name: /Install/i }).click();
  }

  async uninstallServer(serverName: string) {
    const serverCard = this.page.locator(`text="${serverName}"`).first().locator('xpath=ancestor::div[contains(@class, "rounded")]');
    await serverCard.getByRole('button', { name: /Uninstall|Remove/i }).click();
  }

  async openServerDetails(serverName: string) {
    const serverCard = this.page.locator(`text="${serverName}"`).first().locator('xpath=ancestor::div[contains(@class, "rounded")]');
    await serverCard.click();
  }

  async closeServerDetails() {
    await this.page.keyboard.press('Escape');
  }

  async goToNextPage() {
    await this.paginationNext.click();
  }

  async goToPreviousPage() {
    await this.paginationPrev.click();
  }
}
