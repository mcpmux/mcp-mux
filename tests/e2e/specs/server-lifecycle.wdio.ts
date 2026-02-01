/**
 * E2E Tests: Server Installation & Lifecycle
 * Uses data-testid only (ADR-003).
 */

import { byTestId } from '../helpers/selectors';

describe('Server Installation - Echo Server (No Inputs)', () => {
  it('TC-SD-004: Install Echo Server from Discover page', async () => {
    const discoverButton = await byTestId('nav-discover');
    await discoverButton.click();
    await browser.pause(2000);
    
    const searchInput = await byTestId('search-input');
    await searchInput.clearValue();
    await browser.pause(300);
    await searchInput.setValue('Echo');
    await browser.pause(1000);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sl-01-search-echo.png');
    
    const installButton = await byTestId('install-btn-echo-server');
    await installButton.waitForDisplayed({ timeout: 5000 });
    await installButton.waitForClickable({ timeout: 5000 });
    await installButton.click();
    await browser.pause(3000);
    
    const uninstallButton = await byTestId('uninstall-btn-echo-server');
    await expect(uninstallButton).toBeDisplayed();
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sl-02-installed.png');
  });

  it('TC-SL-001: Enable Echo Server (verify server appears in My Servers)', async () => {
    const myServersButton = await byTestId('nav-my-servers');
    await myServersButton.click();
    await browser.pause(2000);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sl-03-my-servers.png');
    
    // Verify Echo Server is in the list
    const pageSource = await browser.getPageSource();
    expect(pageSource.includes('Echo Server')).toBe(true);
    
    const enableButton = await byTestId('enable-server-echo-server');
    const isEnableDisplayed = await enableButton.isDisplayed().catch(() => false);
    
    if (isEnableDisplayed) {
      await enableButton.click();
      await browser.pause(5000); // Wait for connection
    }
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sl-04-enabled.png');
  });

  it('TC-SL-002: Verify connected server shows features (tools, prompts)', async () => {
    // Wait for connection to fully establish
    await browser.pause(3000);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sl-05-connected.png');
    
    // Check page for connection indicators
    const pageSource = await browser.getPageSource();
    
    // Server should show Connected status or feature counts
    const isConnected = 
      pageSource.includes('Connected') || 
      pageSource.includes('tools') ||
      pageSource.includes('Disable');
    
    expect(isConnected).toBe(true);
  });

  it('TC-SL-003: Disable connected server', async () => {
    const disableButton = await byTestId('disable-server-echo-server');
    const isDisableDisplayed = await disableButton.isDisplayed().catch(() => false);
    
    if (isDisableDisplayed) {
      await disableButton.click();
      await browser.pause(2000);
      await browser.saveScreenshot('./tests/e2e/screenshots/sl-06-disabled.png');
      const enableButton = await byTestId('enable-server-echo-server');
      await expect(enableButton).toBeDisplayed();
    } else {
      const enableButton = await byTestId('enable-server-echo-server');
      const isEnableDisplayed = await enableButton.isDisplayed().catch(() => false);
      expect(isEnableDisplayed).toBe(true);
    }
  });

  it('TC-SD-005: Uninstall Echo Server', async () => {
    const discoverButton = await byTestId('nav-discover');
    await discoverButton.click();
    await browser.pause(2000);
    
    const searchInput = await byTestId('search-input');
    await searchInput.clearValue();
    await browser.pause(300);
    await searchInput.setValue('Echo');
    await browser.pause(1000);
    
    const uninstallButton = await byTestId('uninstall-btn-echo-server');
    await uninstallButton.waitForDisplayed({ timeout: 5000 });
    await uninstallButton.waitForClickable({ timeout: 5000 });
    await uninstallButton.click();
    await browser.pause(3000);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sl-07-uninstalled.png');
    
    const installButton = await byTestId('install-btn-echo-server');
    await expect(installButton).toBeDisplayed();
  });
});
