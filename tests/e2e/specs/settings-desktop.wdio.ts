/**
 * Desktop-only E2E tests for Settings (requires Tauri backend)
 * Run with: pnpm test:e2e --spec tests/e2e/specs/settings-desktop.wdio.ts
 */

import { expect, browser } from '@wdio/globals';

describe('Settings - Desktop Features', () => {
  beforeEach(async () => {
    // Navigate to settings page
    const dashboardBtn = await $('nav button[data-testid="nav-dashboard"]');
    await dashboardBtn.waitForClickable();
    await dashboardBtn.click();
    
    const settingsBtn = await $('nav button[data-testid="nav-settings"]');
    await settingsBtn.waitForClickable();
    await settingsBtn.click();
    
    // Wait for settings page to load
    await browser.pause(500);
  });

  describe('Startup & System Tray Settings', () => {
    it('should display startup settings section', async () => {
      const heading = await $('h3*=Startup & System Tray');
      await expect(heading).toBeDisplayed();
      
      const description = await $('p*=Control how McpMux starts');
      await expect(description).toBeDisplayed();
    });

    it('should display all three startup toggles', async () => {
      const autoLaunchLabel = await $('label*=Launch at Startup');
      await expect(autoLaunchLabel).toBeDisplayed();
      
      const startMinimizedLabel = await $('label*=Start Minimized');
      await expect(startMinimizedLabel).toBeDisplayed();
      
      const closeToTrayLabel = await $('label*=Close to Tray');
      await expect(closeToTrayLabel).toBeDisplayed();
    });

    it('should display descriptive text for each setting', async () => {
      const autoLaunchDesc = await $('p*=Start McpMux automatically');
      await expect(autoLaunchDesc).toBeDisplayed();
      
      const startMinimizedDesc = await $('p*=Launch in background');
      await expect(startMinimizedDesc).toBeDisplayed();
      
      const closeToTrayDesc = await $('p*=Keep running in system tray');
      await expect(closeToTrayDesc).toBeDisplayed();
    });

    it('should have functional toggle switches', async () => {
      const autoLaunchSwitch = await $('[data-testid="auto-launch-switch"]');
      await expect(autoLaunchSwitch).toBeDisplayed();
      await expect(autoLaunchSwitch).toBeEnabled();
      
      const closeToTraySwitch = await $('[data-testid="close-to-tray-switch"]');
      await expect(closeToTraySwitch).toBeDisplayed();
      await expect(closeToTraySwitch).toBeEnabled();
      
      const startMinimizedSwitch = await $('[data-testid="start-minimized-switch"]');
      await expect(startMinimizedSwitch).toBeDisplayed();
    });

    it('should toggle auto-launch setting', async () => {
      const autoLaunchSwitch = await $('[data-testid="auto-launch-switch"]');
      
      const initialState = await autoLaunchSwitch.getAttribute('aria-checked');
      
      await autoLaunchSwitch.click();
      await browser.pause(500);
      
      const newState = await autoLaunchSwitch.getAttribute('aria-checked');
      expect(newState).not.toBe(initialState);
      
      // Toggle back
      await autoLaunchSwitch.click();
      await browser.pause(500);
      
      const finalState = await autoLaunchSwitch.getAttribute('aria-checked');
      expect(finalState).toBe(initialState);
    });

    it('should toggle close to tray setting', async () => {
      const closeToTraySwitch = await $('[data-testid="close-to-tray-switch"]');
      
      const initialState = await closeToTraySwitch.getAttribute('aria-checked');
      
      await closeToTraySwitch.click();
      await browser.pause(500);
      
      const newState = await closeToTraySwitch.getAttribute('aria-checked');
      expect(newState).not.toBe(initialState);
      
      // Toggle back
      await closeToTraySwitch.click();
      await browser.pause(500);
      
      const finalState = await closeToTraySwitch.getAttribute('aria-checked');
      expect(finalState).toBe(initialState);
    });

    it('start minimized should be disabled when auto-launch is off', async () => {
      const autoLaunchSwitch = await $('[data-testid="auto-launch-switch"]');
      const startMinimizedSwitch = await $('[data-testid="start-minimized-switch"]');
      
      // Ensure auto-launch is off
      const autoLaunchState = await autoLaunchSwitch.getAttribute('aria-checked');
      if (autoLaunchState === 'true') {
        await autoLaunchSwitch.click();
        await browser.pause(500);
      }
      
      // Start minimized should be disabled
      const isDisabled = await startMinimizedSwitch.getAttribute('disabled');
      expect(isDisabled).toBe('true');
      
      const ariaChecked = await startMinimizedSwitch.getAttribute('aria-checked');
      expect(ariaChecked).toBe('false');
    });

    it('start minimized should be enabled when auto-launch is on', async () => {
      const autoLaunchSwitch = await $('[data-testid="auto-launch-switch"]');
      const startMinimizedSwitch = await $('[data-testid="start-minimized-switch"]');
      
      // Ensure auto-launch is on
      const autoLaunchState = await autoLaunchSwitch.getAttribute('aria-checked');
      if (autoLaunchState === 'false') {
        await autoLaunchSwitch.click();
        await browser.pause(500);
      }
      
      // Start minimized should be enabled
      const isDisabled = await startMinimizedSwitch.getAttribute('disabled');
      expect(isDisabled).toBeNull();
    });

    it('should toggle start minimized when enabled', async () => {
      const autoLaunchSwitch = await $('[data-testid="auto-launch-switch"]');
      const startMinimizedSwitch = await $('[data-testid="start-minimized-switch"]');
      
      // Ensure auto-launch is on first
      const autoLaunchState = await autoLaunchSwitch.getAttribute('aria-checked');
      if (autoLaunchState === 'false') {
        await autoLaunchSwitch.click();
        await browser.pause(500);
      }
      
      const initialState = await startMinimizedSwitch.getAttribute('aria-checked');
      
      await startMinimizedSwitch.click();
      await browser.pause(500);
      
      const newState = await startMinimizedSwitch.getAttribute('aria-checked');
      expect(newState).not.toBe(initialState);
      
      // Toggle back
      await startMinimizedSwitch.click();
      await browser.pause(500);
      
      const finalState = await startMinimizedSwitch.getAttribute('aria-checked');
      expect(finalState).toBe(initialState);
    });

    it('should persist settings across page reloads', async () => {
      const closeToTraySwitch = await $('[data-testid="close-to-tray-switch"]');
      
      const initialState = await closeToTraySwitch.getAttribute('aria-checked');
      
      await closeToTraySwitch.click();
      await browser.pause(500);
      
      // Reload the page
      await browser.refresh();
      await browser.pause(1000);
      
      // Navigate to settings again
      const settingsBtn = await $('nav button[data-testid="nav-settings"]');
      await settingsBtn.waitForClickable();
      await settingsBtn.click();
      await browser.pause(500);
      
      // Verify state persisted
      const persistedState = await closeToTraySwitch.getAttribute('aria-checked');
      expect(persistedState).not.toBe(initialState);
      
      // Restore original state
      await closeToTraySwitch.click();
      await browser.pause(500);
    });

    it('should show disabled state visually for start minimized', async () => {
      const autoLaunchSwitch = await $('[data-testid="auto-launch-switch"]');
      const startMinimizedSwitch = await $('[data-testid="start-minimized-switch"]');
      
      // Ensure auto-launch is off
      const autoLaunchState = await autoLaunchSwitch.getAttribute('aria-checked');
      if (autoLaunchState === 'true') {
        await autoLaunchSwitch.click();
        await browser.pause(500);
      }
      
      // Check that start minimized has disabled styling
      const className = await startMinimizedSwitch.getAttribute('class');
      expect(className).toContain('opacity-50');
    });

    it('all settings should work independently', async () => {
      const closeToTraySwitch = await $('[data-testid="close-to-tray-switch"]');
      
      // Close to tray should work regardless of auto-launch state
      const initialCloseToTray = await closeToTraySwitch.getAttribute('aria-checked');
      
      await closeToTraySwitch.click();
      await browser.pause(500);
      
      const newCloseToTray = await closeToTraySwitch.getAttribute('aria-checked');
      expect(newCloseToTray).not.toBe(initialCloseToTray);
      
      // Restore
      await closeToTraySwitch.click();
      await browser.pause(500);
    });
  });
});
