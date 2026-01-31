import { Page, Locator } from '@playwright/test';
import { BasePage } from './BasePage';

/**
 * FeatureSets page object for managing permission bundles
 */
export class FeatureSetsPage extends BasePage {
  readonly heading: Locator;
  readonly createButton: Locator;
  readonly featureSetList: Locator;
  readonly featureSetCards: Locator;
  readonly emptyState: Locator;

  constructor(page: Page) {
    super(page);
    this.heading = page.getByRole('heading', { name: /FeatureSets|Feature Sets/i });
    this.createButton = page.getByRole('button', { name: /Create|New/i });
    this.featureSetList = page.locator('[data-testid="featureset-list"]');
    this.featureSetCards = page.locator('[data-testid="featureset-card"]');
    this.emptyState = page.locator('text=No feature sets');
  }

  async getFeatureSetByName(name: string): Promise<Locator> {
    return this.page.locator(`text="${name}"`).first();
  }

  async createFeatureSet(name: string, description?: string) {
    await this.createButton.click();
    await this.page.getByPlaceholder(/name/i).fill(name);
    if (description) {
      await this.page.getByPlaceholder(/description/i).fill(description);
    }
    await this.page.getByRole('button', { name: /Create|Save/i }).click();
  }

  async deleteFeatureSet(name: string) {
    const card = await this.getFeatureSetByName(name);
    await card.locator('xpath=ancestor::div').getByRole('button', { name: /Delete/i }).click();
    await this.page.getByRole('button', { name: /Confirm|Yes/i }).click();
  }
}
