import { Page, Locator, expect } from '@playwright/test';
import { BasePage } from './BasePage';

/**
 * My Servers page object for managing installed servers
 */
export class ServersPage extends BasePage {
  readonly heading: Locator;
  readonly addServerButton: Locator;
  readonly gatewayStatus: Locator;
  readonly startGatewayButton: Locator;
  readonly serverList: Locator;
  readonly emptyState: Locator;

  constructor(page: Page) {
    super(page);
    this.heading = page.getByRole('heading', { name: 'My Servers' });
    this.addServerButton = page.getByRole('button', { name: /Add Custom Server/i });
    this.gatewayStatus = page.locator('text=Gateway Running, text=Gateway Stopped').first();
    this.startGatewayButton = page.getByRole('button', { name: 'Start Gateway' });
    this.serverList = page.locator('.space-y-3');
    this.emptyState = page.locator('text=No servers installed');
  }

  async isGatewayRunning(): Promise<boolean> {
    return this.page.locator('text=Gateway Running').isVisible();
  }

  async startGateway() {
    await this.startGatewayButton.click();
    await this.page.waitForSelector('text=Gateway Running', { timeout: 10000 });
  }

  async getServerCards(): Promise<Locator> {
    return this.page.locator('[class*="bg-[rgb(var(--card))]"]');
  }

  async getServerByName(name: string): Promise<Locator> {
    return this.page.locator(`text="${name}"`).first().locator('xpath=ancestor::div[contains(@class, "rounded-xl")]');
  }

  async enableServer(serverName: string) {
    const serverCard = await this.getServerByName(serverName);
    await serverCard.getByRole('button', { name: 'Enable' }).click();
  }

  async disableServer(serverName: string) {
    const serverCard = await this.getServerByName(serverName);
    await serverCard.getByRole('button', { name: 'Disable' }).click();
  }

  async getServerStatus(serverName: string): Promise<string> {
    const serverCard = await this.getServerByName(serverName);
    const statusBadge = serverCard.locator('[class*="inline-flex items-center"]').first();
    return (await statusBadge.textContent()) || '';
  }

  async openServerMenu(serverName: string) {
    const serverCard = await this.getServerByName(serverName);
    await serverCard.getByRole('button', { name: /more/i }).click();
  }

  async viewServerLogs(serverName: string) {
    await this.openServerMenu(serverName);
    await this.page.getByRole('menuitem', { name: /View Logs/i }).click();
  }

  async uninstallServer(serverName: string) {
    await this.openServerMenu(serverName);
    await this.page.getByRole('menuitem', { name: /Uninstall|Remove/i }).click();
    // Confirm dialog
    await this.page.getByRole('button', { name: /OK|Confirm|Yes/i }).click();
  }
}
