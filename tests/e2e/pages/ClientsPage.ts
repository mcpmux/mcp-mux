import { Page, Locator } from '@playwright/test';
import { BasePage } from './BasePage';

/**
 * Clients page object for viewing connected AI clients
 */
export class ClientsPage extends BasePage {
  readonly heading: Locator;
  readonly clientList: Locator;
  readonly clientCards: Locator;
  readonly emptyState: Locator;
  readonly refreshButton: Locator;

  constructor(page: Page) {
    super(page);
    this.heading = page.getByRole('heading', { name: 'Clients' });
    this.clientList = page.locator('[data-testid="client-list"]');
    this.clientCards = page.locator('[data-testid="client-card"]');
    this.emptyState = page.locator('text=No clients connected');
    this.refreshButton = page.getByRole('button', { name: /Refresh/i });
  }

  async getClientCount(): Promise<number> {
    return await this.clientCards.count();
  }

  async getClientByName(name: string): Promise<Locator> {
    return this.page.locator(`text="${name}"`).first();
  }

  async revokeClient(clientName: string) {
    const card = this.page.locator(`text="${clientName}"`).first().locator('xpath=ancestor::div');
    await card.getByRole('button', { name: /Revoke|Disconnect/i }).click();
    await this.page.getByRole('button', { name: /Confirm|Yes/i }).click();
  }
}
