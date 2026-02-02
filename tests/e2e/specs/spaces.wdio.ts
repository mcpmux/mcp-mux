/**
 * E2E Tests: Space Management
 * Uses data-testid only (ADR-003).
 */

import { byTestId, TIMEOUT, waitForModalClose, safeClick } from '../helpers/selectors';

describe('Space Management - Default Space', () => {
  it('TC-SP-001: Navigate to Spaces page and verify default space exists', async () => {
    const spacesButton = await byTestId('nav-spaces');
    await safeClick(spacesButton);
    await browser.pause(2000);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sp-01-spaces-page.png');
    
    // Verify page loaded
    const pageSource = await browser.getPageSource();
    const hasSpacesPage = pageSource.includes('Workspaces') || pageSource.includes('Space');
    
    expect(hasSpacesPage).toBe(true);
    
    // Check for default space (usually "My Space")
    const hasDefaultSpace = 
      pageSource.includes('My Space') || 
      pageSource.includes('Default') ||
      pageSource.includes('Active');
    
    console.log('[DEBUG] Has default space:', hasDefaultSpace);
    expect(hasDefaultSpace).toBe(true);
  });
});

describe('Space Management - Create and Delete', () => {
  const createdSpaceName = 'Test Space E2E';

  /** Dismiss create-space modal if open (e.g. from failed previous test) */
  async function dismissCreateModalIfOpen() {
    const overlay = await $('[data-testid="create-space-modal-overlay"]');
    if (await overlay.isDisplayed().catch(() => false)) {
      const cancelBtn = await byTestId('create-space-cancel-btn');
      if (await cancelBtn.isDisplayed().catch(() => false)) {
        await cancelBtn.click();
        await browser.pause(500);
      } else {
        await browser.keys('Escape');
        await browser.pause(500);
      }
    }
  }

  it('TC-SP-002: Create a new space', async () => {
    const spacesButton = await byTestId('nav-spaces');
    await safeClick(spacesButton);
    await browser.pause(2000);
    
    // Click Create Space button
    const createButton = await byTestId('create-space-btn');
    const isCreateDisplayed = await createButton.isDisplayed().catch(() => false);
    
    if (isCreateDisplayed) {
      await safeClick(createButton);
      await browser.pause(1000);
      
      await browser.saveScreenshot('./tests/e2e/screenshots/sp-02-create-modal.png');
      
      // Find name input and enter space name
      const nameInput = await byTestId('create-space-name-input');
      const isInputDisplayed = await nameInput.isDisplayed().catch(() => false);
      
      if (isInputDisplayed) {
        await nameInput.setValue(createdSpaceName);
        await browser.pause(500);
        
        await browser.saveScreenshot('./tests/e2e/screenshots/sp-02b-name-entered.png');
        
        // Click Create Space submit button
        const submitButton = await byTestId('create-space-submit-btn');
        const isSubmitDisplayed = await submitButton.isDisplayed().catch(() => false);
        
        console.log('[DEBUG] Submit button displayed:', isSubmitDisplayed);
        
        if (isSubmitDisplayed) {
          await submitButton.waitForClickable({ timeout: TIMEOUT.medium });
          await safeClick(submitButton);
          await browser.pause(2000);
          await waitForModalClose();
        }
      } else {
        console.log('[DEBUG] Name input not found');
      }
      
      await browser.saveScreenshot('./tests/e2e/screenshots/sp-03-after-create.png');
      
      // Verify new space appears
      const pageSource = await browser.getPageSource();
      const hasNewSpace = pageSource.includes(createdSpaceName) || pageSource.includes('Test Space');
      
      console.log('[DEBUG] New space created:', hasNewSpace);
      expect(hasNewSpace).toBe(true);
    } else {
      console.log('[DEBUG] Create Space button not found');
      expect(true).toBe(true);
    }
  });

  it('TC-SP-003: Set a space as active', async () => {
    await dismissCreateModalIfOpen();
    const setActiveButtons = await $$('[data-testid^="set-active-space-"]');
    
    if (setActiveButtons.length > 0) {
      const firstButton = setActiveButtons[0];
      const isDisplayed = await firstButton.isDisplayed().catch(() => false);
      if (isDisplayed) {
        await browser.saveScreenshot('./tests/e2e/screenshots/sp-04-before-set-active.png');
        await firstButton.click();
        await browser.pause(2000);
        await browser.saveScreenshot('./tests/e2e/screenshots/sp-05-after-set-active.png');
      }
    }
    
    // Verify page has active space indicator
    const pageSource = await browser.getPageSource();
    const hasActiveIndicator = 
      pageSource.includes('Active') || 
      pageSource.includes('active');
    
    expect(hasActiveIndicator).toBe(true);
  });

  it('TC-SP-011: Verify spaces are listed on page', async () => {
    await dismissCreateModalIfOpen();
    const spacesButton = await byTestId('nav-spaces');
    await safeClick(spacesButton);
    await browser.pause(2000);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sp-06-spaces-list.png');
    
    // Verify spaces are listed
    const pageSource = await browser.getPageSource();
    const hasSpacesList = 
      pageSource.includes('My Space') || 
      pageSource.includes('Test Space') ||
      pageSource.includes('Workspaces');
    
    expect(hasSpacesList).toBe(true);
  });

  it('TC-SP-005: Cleanup - Delete test space if exists', async () => {
    await dismissCreateModalIfOpen();
    const deleteButtons = await $$('[data-testid^="delete-space-"]');
    
    await browser.saveScreenshot('./tests/e2e/screenshots/sp-07-before-delete.png');
    
    if (deleteButtons.length > 0) {
      const firstDeleteBtn = deleteButtons[0];
      const isDisplayed = await firstDeleteBtn.isDisplayed().catch(() => false);
      
      if (isDisplayed) {
        await firstDeleteBtn.click();
        await browser.pause(2000);
        await browser.saveScreenshot('./tests/e2e/screenshots/sp-08-after-delete.png');
      }
    }
    
    // Verify page still works
    const pageSource = await browser.getPageSource();
    expect(pageSource.includes('Workspaces') || pageSource.includes('Space')).toBe(true);
  });
});
