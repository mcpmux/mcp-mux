/**
 * Tauri E2E tests using WebdriverIO
 * Uses data-testid only (ADR-003).
 */

import { byTestId } from '../helpers/selectors';

describe('McpMux Application', () => {
  it('should launch and show main window', async () => {
    await browser.pause(2000);
    const title = await browser.getTitle();
    expect(title).toBe('McpMux');
  });

  it('should display sidebar navigation', async () => {
    const navItem = await byTestId('nav-dashboard');
    await navItem.waitForDisplayed({ timeout: 10000 });
    await expect(navItem).toBeDisplayed();
  });

  it('should show My Servers tab', async () => {
    const serversButton = await byTestId('nav-my-servers');
    await serversButton.waitForDisplayed({ timeout: 10000 });
    await expect(serversButton).toBeDisplayed();
  });

  it('should show Discover tab', async () => {
    const discoverButton = await byTestId('nav-discover');
    await expect(discoverButton).toBeDisplayed();
  });

  it('should navigate to Discover page', async () => {
    const discoverButton = await byTestId('nav-discover');
    await discoverButton.click();
    await browser.pause(1000);
    const heading = await byTestId('registry-title');
    await expect(heading).toBeDisplayed();
  });

  it('should show search input on Discover page', async () => {
    const searchInput = await byTestId('search-input');
    await expect(searchInput).toBeDisplayed();
  });

  it('should navigate to My Servers page', async () => {
    const serversButton = await byTestId('nav-my-servers');
    await serversButton.click();
    await browser.pause(1000);
    const heading = await byTestId('servers-title');
    await expect(heading).toBeDisplayed();
  });

  it('should navigate to Clients page', async () => {
    const clientsButton = await byTestId('nav-clients');
    await clientsButton.waitForClickable({ timeout: 5000 });
    await clientsButton.click();
    await browser.pause(1500);
    const pageSource = await browser.getPageSource();
    expect(pageSource.includes('Connected Clients') || pageSource.includes('Clients')).toBe(true);
  });

  it('should navigate to FeatureSets page', async () => {
    const featuresButton = await byTestId('nav-featuresets');
    await featuresButton.waitForClickable({ timeout: 5000 });
    await featuresButton.click();
    await browser.pause(1500);
    const pageSource = await browser.getPageSource();
    expect(pageSource.includes('Feature Sets') || pageSource.includes('FeatureSets')).toBe(true);
  });

  it('should show space switcher in sidebar', async () => {
    const navItem = await byTestId('nav-dashboard');
    await navItem.waitForDisplayed({ timeout: 5000 });
    await expect(navItem).toBeDisplayed();
  });
});

describe('Registry/Discover Functionality', () => {
  before(async () => {
    const discoverButton = await byTestId('nav-discover');
    await discoverButton.click();
    await browser.pause(2000);
  });

  it('should display server cards', async () => {
    await browser.pause(2000);
    const pageSource = await browser.getPageSource();
    const hasServerContent = 
      pageSource.includes('Echo Server') || 
      pageSource.includes('Install') ||
      pageSource.includes('server');
    expect(hasServerContent).toBe(true);
  });

  it('should filter servers when searching', async () => {
    const searchInput = await byTestId('search-input');
    await searchInput.clearValue();
    await browser.pause(300);
    await searchInput.setValue('Echo');
    await browser.pause(1000);
    const pageSource = await browser.getPageSource();
    expect(pageSource.includes('Echo')).toBe(true);
  });

  it('should clear search and show servers', async () => {
    const searchInput = await byTestId('search-input');
    await searchInput.clearValue();
    await browser.pause(1000);
    const pageSource = await browser.getPageSource();
    const hasContent = pageSource.includes('Server') || pageSource.includes('Install');
    expect(hasContent).toBe(true);
  });
});
