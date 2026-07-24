import { Page, Locator } from '@playwright/test';
import { BasePage } from './BasePage';

/**
 * Dashboard page object
 */
export class DashboardPage extends BasePage {
  readonly heading: Locator;
  readonly gatewayStatus: Locator;
  readonly gatewayToggleButton: Locator;
  readonly serverCountCard: Locator;
  readonly featureSetsCard: Locator;
  readonly clientsCard: Locator;
  readonly activeSpaceCard: Locator;
  readonly connectIDEsSection: Locator;
  readonly clientGrid: Locator;

  constructor(page: Page) {
    super(page);
    this.heading = page.getByRole('heading', { name: 'Dashboard' });
    this.gatewayStatus = page.locator('text=Gateway:').first();
    this.gatewayToggleButton = page.getByRole('button', { name: /Start|Stop/ });
    this.serverCountCard = page.locator('text=My Servers').first();
    this.featureSetsCard = page.locator('text=Bundles').first();
    this.clientsCard = page.locator('text=Clients').first();
    this.activeSpaceCard = page.locator('text=Active Space').first();
    this.connectIDEsSection = page.locator('text=Connect Your IDEs');
    this.clientGrid = page.locator('[data-testid="client-grid"]');
  }

  async navigate() {
    await this.goto('/');
    await this.waitForLoad();
  }

  async isGatewayRunning(): Promise<boolean> {
    const text = await this.gatewayStatus.textContent();
    return text?.includes('Running') ?? false;
  }

  async toggleGateway() {
    await this.gatewayToggleButton.click();
    // Wait for status to change
    await this.page.waitForTimeout(500);
  }

  async getServerCount(): Promise<string> {
    const card = this.page.locator('text=Connected / Installed').locator('..');
    const countText = await card.locator('.text-3xl').textContent();
    return countText || '0/0';
  }

  async copyConfig() {
    // Open JSON config popover and click copy
    await this.page.locator('[data-testid="client-icon-copy-config"]').click();
    await this.page.locator('[data-testid="copy-config-btn"]').click();
  }
}
