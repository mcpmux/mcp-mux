/**
 * E2E Test Selectors - data-testid only (ADR-003)
 * Use $('[data-testid="x"]') for all element selection.
 */

// CI-friendly timeouts (Windows CI is slower)
export const TIMEOUT = {
  short: 5000,
  medium: 15000,  // Default for waitForDisplayed/Clickable
  long: 30000,    // For slow operations like MCP connections
  veryLong: 60000,
};

/** Get element by data-testid */
export const byTestId = (testId: string) => $(`[data-testid="${testId}"]`);

/**
 * Wait for any modal overlay to close (backdrop with blur).
 * This is a best-effort function - it won't fail the test if the modal doesn't close.
 * It will try to dismiss it by pressing Escape if it's still open.
 */
export async function waitForModalClose(timeout = TIMEOUT.short): Promise<void> {
  try {
    // Match any fixed fullscreen overlay (bg-black/20, bg-black/50, bg-black/60)
    const overlays = await $$('.fixed.inset-0');

    for (const overlay of overlays) {
      const isDisplayed = await overlay.isDisplayed().catch(() => false);
      if (!isDisplayed) continue;

      // Check if it looks like a modal backdrop (has bg-black in its classes)
      const cls = await overlay.getAttribute('class').catch(() => '') ?? '';
      if (!cls.includes('bg-black')) continue;

      // Try to wait for it to close naturally
      const closed = await overlay
        .waitForDisplayed({ timeout, reverse: true })
        .then(() => true)
        .catch(() => false);

      if (!closed) {
        // Modal still open - try to dismiss it with Escape key
        console.log('[waitForModalClose] Modal still displayed, trying Escape key');
        await browser.keys('Escape');
        await browser.pause(500);
      }
    }
  } catch {
    // Silently continue - modal handling shouldn't fail tests
  }
}

/** Click element after ensuring no modal overlay is blocking */
export async function safeClick(element: WebdriverIO.Element, timeout = TIMEOUT.medium): Promise<void> {
  await waitForModalClose(TIMEOUT.short);
  await element.waitForClickable({ timeout });
  await element.click();
}
