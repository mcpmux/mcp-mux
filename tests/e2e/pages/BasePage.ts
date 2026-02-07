import { Page, Locator } from '@playwright/test';

/**
 * Base Page Object class with common functionality
 */
export abstract class BasePage {
  readonly page: Page;
  readonly loadingIndicator: Locator;
  readonly errorMessage: Locator;

  constructor(page: Page) {
    this.page = page;
    this.loadingIndicator = page.getByTestId('loading');
    this.errorMessage = page.getByRole('alert');
  }

  /**
   * Navigate to a URL
   */
  async goto(path: string = '/') {
    await this.page.goto(path);
    await this.waitForLoad();
  }

  /**
   * Wait for the page to finish loading
   */
  async waitForLoad() {
    await this.page.waitForLoadState('networkidle');
  }

  /**
   * Wait for loading indicator to disappear
   */
  async waitForLoadingComplete() {
    await this.loadingIndicator.waitFor({ state: 'hidden', timeout: 10000 }).catch(() => {
      // Loading indicator might not exist, that's OK
    });
  }

  /**
   * Check if an error message is displayed
   */
  async hasError(): Promise<boolean> {
    return await this.errorMessage.isVisible().catch(() => false);
  }

  /**
   * Get the error message text
   */
  async getErrorText(): Promise<string | null> {
    if (await this.hasError()) {
      return await this.errorMessage.textContent();
    }
    return null;
  }

  /**
   * Take a screenshot for debugging
   */
  async screenshot(name: string) {
    await this.page.screenshot({ path: `./reports/screenshots/${name}.png` });
  }

  /**
   * Wait for a toast notification of the given type to appear
   */
  async waitForToast(type: 'success' | 'error' | 'warning' | 'info', timeout = 5000) {
    await this.page.getByTestId(`toast-${type}`).first().waitFor({ timeout });
  }

  /**
   * Get text content of the first visible toast
   */
  async getToastText(): Promise<string | null> {
    const toast = this.page.getByTestId('toast-container').locator('[role="alert"]').first();
    return toast.textContent();
  }

  /**
   * Dismiss the first visible toast
   */
  async dismissToast() {
    await this.page.getByTestId('toast-close').first().click();
  }

  /**
   * Assert that a toast container is present in the DOM
   */
  get toastContainer(): Locator {
    return this.page.getByTestId('toast-container');
  }
}
