/**
 * Desktop-only E2E tests for Settings (requires Tauri backend)
 * Run with: pnpm test:e2e --spec tests/e2e/specs/settings-desktop.wdio.ts
 */

import { expect, browser } from '@wdio/globals';
import { byTestId, TIMEOUT } from '../helpers/selectors';

describe('Settings - Desktop Features', () => {
    before(async () => {
        // Let the app and WebView load (spec may run in isolation so no prior tests have warmed the UI)
        await browser.pause(5000);
        // Ensure app shell is ready before any test
        const sidebar = await byTestId('sidebar');
        await sidebar.waitForDisplayed({ timeout: TIMEOUT.veryLong });
        const navSettings = await byTestId('nav-settings');
        await navSettings.waitForClickable({ timeout: TIMEOUT.medium });
    });

    beforeEach(async () => {
        // Go to Settings (same pattern as settings.wdio.ts)
        const settingsBtn = await byTestId('nav-settings');
        await settingsBtn.click();
        // Wait for desktop Startup section to be present (section is always rendered; toggles may still be loading)
        const startupSection = await byTestId('settings-startup-section');
        await startupSection.waitForDisplayed({ timeout: TIMEOUT.medium });
        // Wait for toggles to be interactive (get_startup_settings has resolved)
        const autoLaunchSwitch = await byTestId('auto-launch-switch');
        await autoLaunchSwitch.waitForDisplayed({ timeout: TIMEOUT.medium });
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

            // Note: When disabled, the value stays at its default (true)
            const ariaChecked = await startMinimizedSwitch.getAttribute('aria-checked');
            expect(ariaChecked).toBe('true');
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

            // Navigate to settings again and wait for section to load
            const settingsBtn = await $('nav button[data-testid="nav-settings"]');
            await settingsBtn.waitForClickable();
            await settingsBtn.click();
            const switchAfterReload = await $('[data-testid="close-to-tray-switch"]');
            await switchAfterReload.waitForDisplayed({ timeout: TIMEOUT.medium });

            // Verify state persisted
            const persistedState = await switchAfterReload.getAttribute('aria-checked');
            expect(persistedState).not.toBe(initialState);

            // Restore original state
            await switchAfterReload.click();
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
