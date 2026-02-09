/**
 * E2E Tests: Deep Link Server Install
 * Tests the mcpmux://install?server=xxx flow triggered from the discovery UI.
 * Uses data-testid only (ADR-003).
 */

import { byTestId, TIMEOUT, waitForModalClose } from '../helpers/selectors';
import {
  emitEvent,
  getActiveSpace,
  listInstalledServers,
  uninstallServer,
} from '../helpers/tauri-api';

const ECHO_SERVER_ID = 'echo-server';

/** Simulate a deep link install event (as if mcpmux://install?server=xxx was received) */
async function simulateInstallDeepLink(serverId: string) {
  await emitEvent('server-install-request', { serverId });
}

describe('Deep Link Install - Valid Server', () => {
  let activeSpaceId: string;

  before(async () => {
    // Get active space for cleanup
    const space = await getActiveSpace();
    activeSpaceId = space?.id || '';

    // Ensure echo-server is not installed (clean state)
    if (activeSpaceId) {
      const installed = await listInstalledServers(activeSpaceId);
      const echoInstalled = installed.some(
        (s) => s.server_id === ECHO_SERVER_ID
      );
      if (echoInstalled) {
        try {
          await uninstallServer(ECHO_SERVER_ID, activeSpaceId);
          await browser.pause(1000);
        } catch (e) {
          console.log('[setup] Could not uninstall echo-server:', e);
        }
      }
    }
  });

  it('TC-DL-001: Deep link shows install modal with server info', async () => {
    await simulateInstallDeepLink(ECHO_SERVER_ID);
    await browser.pause(3000); // Wait for server definition lookup

    await browser.saveScreenshot(
      './tests/e2e/screenshots/dl-01-install-modal.png'
    );

    // Modal should be displayed (either loading or ready)
    const modal = await byTestId('install-modal');
    const loading = await byTestId('install-modal-loading');
    const modalDisplayed = await modal.isDisplayed().catch(() => false);
    const loadingDisplayed = await loading.isDisplayed().catch(() => false);

    expect(modalDisplayed || loadingDisplayed).toBe(true);

    // If ready, verify server name is shown
    if (modalDisplayed) {
      const serverName = await byTestId('install-modal-server-name');
      const nameDisplayed = await serverName.isDisplayed().catch(() => false);
      if (nameDisplayed) {
        const text = await serverName.getText();
        expect(text).toContain('Echo');
      }
    }
  });

  it('TC-DL-002: Install modal shows space picker', async () => {
    const modal = await byTestId('install-modal');
    const isDisplayed = await modal.isDisplayed().catch(() => false);

    if (isDisplayed) {
      const spaceSelect = await byTestId('install-modal-space-select');
      const selectDisplayed = await spaceSelect.isDisplayed().catch(() => false);
      expect(selectDisplayed).toBe(true);

      await browser.saveScreenshot(
        './tests/e2e/screenshots/dl-02-space-picker.png'
      );
    } else {
      // Modal might still be loading or already closed on slow CI
      const pageSource = await browser.getPageSource();
      const hasModal =
        pageSource.includes('Install Server') ||
        pageSource.includes('Looking up server');
      expect(hasModal).toBe(true);
    }
  });

  it('TC-DL-003: Clicking Install button installs the server', async () => {
    const modal = await byTestId('install-modal');
    const isDisplayed = await modal.isDisplayed().catch(() => false);

    if (isDisplayed) {
      const installBtn = await byTestId('install-modal-install-btn');
      await installBtn.waitForClickable({ timeout: TIMEOUT.medium });
      await installBtn.click();
      await browser.pause(3000);

      await browser.saveScreenshot(
        './tests/e2e/screenshots/dl-03-after-install.png'
      );

      // Should show success state or have auto-dismissed
      const success = await byTestId('install-modal-success');
      const successDisplayed = await success.isDisplayed().catch(() => false);

      if (successDisplayed) {
        const successMsg = await byTestId('install-modal-success-message');
        const msgText = await successMsg.getText().catch(() => '');
        expect(msgText).toContain('has been added');
      }

      // Wait for auto-dismiss
      await browser.pause(3000);
      await waitForModalClose();
    }

    // Verify server is actually installed via API
    if (activeSpaceId) {
      const installed = await listInstalledServers(activeSpaceId);
      const echoInstalled = installed.some(
        (s) => s.server_id === ECHO_SERVER_ID
      );
      expect(echoInstalled).toBe(true);
    }
  });

  it('TC-DL-004: Deep link for already-installed server shows warning', async () => {
    // Echo server was installed in TC-DL-003
    await simulateInstallDeepLink(ECHO_SERVER_ID);
    await browser.pause(3000);

    await browser.saveScreenshot(
      './tests/e2e/screenshots/dl-04-already-installed.png'
    );

    const modal = await byTestId('install-modal');
    const isDisplayed = await modal.isDisplayed().catch(() => false);

    if (isDisplayed) {
      // Should show "already installed" warning
      const alreadyInstalled = await byTestId('install-modal-already-installed');
      const warningDisplayed = await alreadyInstalled
        .isDisplayed()
        .catch(() => false);
      expect(warningDisplayed).toBe(true);

      // Install button should be disabled
      const installBtn = await byTestId('install-modal-install-btn');
      const isDisabled = await installBtn.getAttribute('disabled');
      expect(isDisabled).not.toBeNull();

      // Dismiss the modal
      const cancelBtn = await byTestId('install-modal-cancel-btn');
      await cancelBtn.click();
      await browser.pause(1000);
      await waitForModalClose();
    }
  });

  it('TC-DL-005: Cancel button dismisses the modal', async () => {
    // First uninstall so we get a clean modal
    if (activeSpaceId) {
      try {
        await uninstallServer(ECHO_SERVER_ID, activeSpaceId);
        await browser.pause(1000);
      } catch (e) {
        /* ignore */
      }
    }

    await simulateInstallDeepLink(ECHO_SERVER_ID);
    await browser.pause(3000);

    const modal = await byTestId('install-modal');
    const isDisplayed = await modal.isDisplayed().catch(() => false);

    if (isDisplayed) {
      const cancelBtn = await byTestId('install-modal-cancel-btn');
      await cancelBtn.click();
      await browser.pause(1000);

      // Modal should be gone
      const modalAfter = await byTestId('install-modal');
      const stillDisplayed = await modalAfter.isDisplayed().catch(() => false);
      expect(stillDisplayed).toBe(false);
    }

    await browser.saveScreenshot(
      './tests/e2e/screenshots/dl-05-dismissed.png'
    );
  });

  after(async () => {
    // Cleanup: uninstall echo-server if it was installed
    if (activeSpaceId) {
      try {
        await uninstallServer(ECHO_SERVER_ID, activeSpaceId);
      } catch (e) {
        /* ignore */
      }
    }
    await waitForModalClose();
  });
});

describe('Deep Link Install - Invalid Server', () => {
  it('TC-DL-006: Deep link with unknown server ID shows error', async () => {
    await simulateInstallDeepLink('nonexistent-server-12345');
    await browser.pause(3000);

    await browser.saveScreenshot(
      './tests/e2e/screenshots/dl-06-not-found.png'
    );

    const errorModal = await byTestId('install-modal-error');
    const isDisplayed = await errorModal.isDisplayed().catch(() => false);

    if (isDisplayed) {
      // Error message should mention the server was not found
      const errorMsg = await byTestId('install-modal-error-message');
      const text = await errorMsg.getText().catch(() => '');
      expect(text).toContain('not found');

      // Close the error modal
      const closeBtn = await byTestId('install-modal-close-btn');
      await closeBtn.click();
      await browser.pause(1000);
      await waitForModalClose();
    } else {
      // On slow CI, check page source
      const pageSource = await browser.getPageSource();
      const hasError =
        pageSource.includes('not found') ||
        pageSource.includes('Server Not Found');
      expect(hasError).toBe(true);
    }
  });

  after(async () => {
    await waitForModalClose();
  });
});
