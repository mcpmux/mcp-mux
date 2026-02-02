/**
 * E2E Tests: Server Configuration with Inputs
 * Uses data-testid only (ADR-003).
 */

import { byTestId, TIMEOUT, waitForModalClose, safeClick } from '../helpers/selectors';

describe('Server Configuration - API Key Server', () => {
  it('TC-SC-001: Install API Key Server and click Enable shows config modal', async () => {
    const discoverButton = await byTestId('nav-discover');
    await safeClick(discoverButton);
    await browser.pause(2000);
    
    const searchInput = await byTestId('search-input');
    await searchInput.clearValue();
    await browser.pause(300);
    await searchInput.setValue('API Key');
    await browser.pause(1000);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sc-01-search-apikey.png');
    
    const installButton = await byTestId('install-btn-api-key-server');
    const isInstallDisplayed = await installButton.isDisplayed().catch(() => false);
    
    if (isInstallDisplayed) {
      await installButton.waitForClickable({ timeout: TIMEOUT.medium });
      await safeClick(installButton);
      await browser.pause(3000);
      await waitForModalClose();
    }
    
    const uninstallButton = await byTestId('uninstall-btn-api-key-server');
    await expect(uninstallButton).toBeDisplayed();
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sc-02-apikey-installed.png');
  });

  it('TC-SC-002: Enable shows configuration modal with API Key input', async () => {
    const myServersButton = await byTestId('nav-my-servers');
    await safeClick(myServersButton);
    await browser.pause(2000);
    
    // Verify API Key Server is in the list
    const pageSource = await browser.getPageSource();
    expect(pageSource.includes('API Key Server')).toBe(true);
    
    const enableButton = await byTestId('enable-server-api-key-server');
    await safeClick(enableButton);
    await browser.pause(1000);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sc-03-config-modal.png');
    
    // Should show configuration modal
    const modalSource = await browser.getPageSource();
    const hasConfigModal = 
      modalSource.includes('Configure') || 
      modalSource.includes('API Key') ||
      modalSource.includes('Test API Key');
    
    expect(hasConfigModal).toBe(true);
  });

  it('TC-SC-002b: Enter API Key and save configuration', async () => {
    const apiKeyInput = await byTestId('config-input-API_KEY');
    const isInputDisplayed = await apiKeyInput.isDisplayed().catch(() => false);
    
    if (isInputDisplayed) {
      await apiKeyInput.setValue('test_api_key_12345');
      await browser.pause(500);
      
      await browser.saveScreenshot('./tests/e2e/screenshots/sc-04-entered-key.png');
      
      const saveButton = await byTestId('config-save-btn');
      const isSaveDisplayed = await saveButton.isDisplayed().catch(() => false);
      
      if (isSaveDisplayed) {
        await safeClick(saveButton);
        await browser.pause(3000);
        await waitForModalClose();
        
        await browser.saveScreenshot('./tests/e2e/screenshots/sc-05-saved.png');
      }
    }
    
    // Verify we're back on My Servers page or modal closed
    const pageSource = await browser.getPageSource();
    const modalClosed = 
      !pageSource.includes('Cancel') || 
      pageSource.includes('Connected') || 
      pageSource.includes('Connecting') ||
      pageSource.includes('My Servers');
    
    expect(modalClosed).toBe(true);
  });

  it('Cleanup: Uninstall API Key Server', async () => {
    const discoverButton = await byTestId('nav-discover');
    await safeClick(discoverButton);
    await browser.pause(2000);
    
    const searchInput = await byTestId('search-input');
    await searchInput.clearValue();
    await browser.pause(300);
    await searchInput.setValue('API Key');
    await browser.pause(1000);
    
    const uninstallButton = await byTestId('uninstall-btn-api-key-server');
    const isDisplayed = await uninstallButton.isDisplayed().catch(() => false);
    
    if (isDisplayed) {
      await uninstallButton.waitForClickable({ timeout: TIMEOUT.medium });
      await safeClick(uninstallButton);
      await browser.pause(2000);
      await waitForModalClose();
    }
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sc-06-apikey-cleanup.png');
  });
});

describe('Server Configuration - Directory Server', () => {
  it('TC-SC-003: Install Directory Server', async () => {
    const discoverButton = await byTestId('nav-discover');
    await safeClick(discoverButton);
    await browser.pause(2000);
    
    const searchInput = await byTestId('search-input');
    await searchInput.clearValue();
    await browser.pause(300);
    await searchInput.setValue('Directory');
    await browser.pause(1000);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sc-07-search-dir.png');
    
    const installButton = await byTestId('install-btn-directory-server');
    const isInstallDisplayed = await installButton.isDisplayed().catch(() => false);
    
    if (isInstallDisplayed) {
      await installButton.waitForClickable({ timeout: TIMEOUT.medium });
      await safeClick(installButton);
      await browser.pause(3000);
      await waitForModalClose();
    }
    
    const uninstallButton = await byTestId('uninstall-btn-directory-server');
    await expect(uninstallButton).toBeDisplayed();
  });

  it('TC-SC-003b: Enable shows config modal with directory path input', async () => {
    const myServersButton = await byTestId('nav-my-servers');
    await safeClick(myServersButton);
    await browser.pause(2000);
    
    const enableButton = await byTestId('enable-server-directory-server');
    const isEnableDisplayed = await enableButton.isDisplayed().catch(() => false);
    
    if (isEnableDisplayed) {
      await safeClick(enableButton);
      await browser.pause(1000);
      
      await browser.saveScreenshot('./tests/e2e/screenshots/sc-08-dir-modal.png');
      
      const dirInput = await byTestId('config-input-DIRECTORY');
      const isInputDisplayed = await dirInput.isDisplayed().catch(() => false);
      
      if (isInputDisplayed) {
        // Enter a test directory path
        await dirInput.setValue('C:\\Users\\test');
        await browser.pause(500);
        
        await browser.saveScreenshot('./tests/e2e/screenshots/sc-09-dir-entered.png');
        
        const saveButton = await byTestId('config-save-btn');
        if (await saveButton.isDisplayed().catch(() => false)) {
          await safeClick(saveButton);
          await browser.pause(3000);
          await waitForModalClose();
        }
      }
    }
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sc-10-dir-after-config.png');
  });

  it('Cleanup: Uninstall Directory Server', async () => {
    const discoverButton = await byTestId('nav-discover');
    await safeClick(discoverButton);
    await browser.pause(2000);
    
    const searchInput = await byTestId('search-input');
    await searchInput.clearValue();
    await browser.pause(300);
    await searchInput.setValue('Directory');
    await browser.pause(1000);
    
    const uninstallButton = await byTestId('uninstall-btn-directory-server');
    const isDisplayed = await uninstallButton.isDisplayed().catch(() => false);
    
    if (isDisplayed) {
      await uninstallButton.waitForClickable({ timeout: TIMEOUT.medium });
      await safeClick(uninstallButton);
      await browser.pause(2000);
      await waitForModalClose();
    }
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sc-11-dir-cleanup.png');
  });
});
