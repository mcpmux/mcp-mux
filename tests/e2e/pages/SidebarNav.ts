import { Page, Locator } from '@playwright/test';
import { BasePage } from './BasePage';

/**
 * Sidebar navigation component
 */
export class SidebarNav {
  readonly page: Page;
  readonly dashboard: Locator;
  readonly myServers: Locator;
  readonly discover: Locator;
  readonly spaces: Locator;
  readonly featureSets: Locator;
  readonly clients: Locator;
  readonly settings: Locator;
  readonly spaceSwitcher: Locator;
  readonly themeToggle: Locator;

  constructor(page: Page) {
    this.page = page;
    this.dashboard = page.getByRole('button', { name: 'Dashboard', exact: true });
    this.myServers = page.getByRole('button', { name: 'My Servers', exact: true });
    this.discover = page.getByRole('button', { name: 'Search', exact: true });
    this.spaces = page.getByRole('button', { name: 'Spaces', exact: true }).last();
    this.featureSets = page.getByRole('button', { name: 'Bundles', exact: true });
    this.clients = page.getByRole('button', { name: 'Clients', exact: true }).last();
    this.settings = page.getByRole('button', { name: 'Settings', exact: true });
    this.spaceSwitcher = page.locator('[data-testid="space-switcher"]');
    this.themeToggle = page.locator('button[title*="mode"]');
  }

  async goToDashboard() {
    await this.dashboard.click();
  }

  async goToMyServers() {
    await this.myServers.click();
  }

  async goToDiscover() {
    await this.discover.click();
  }

  async goToSpaces() {
    await this.spaces.click();
  }

  async goToFeatureSets() {
    await this.featureSets.click();
  }

  async goToClients() {
    await this.clients.click();
  }

  async goToSettings() {
    await this.settings.click();
  }

  async toggleTheme() {
    await this.themeToggle.click();
  }
}
