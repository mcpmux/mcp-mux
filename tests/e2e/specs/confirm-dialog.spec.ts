import { test, expect } from '@playwright/test';
import { DashboardPage, SpacesPage, ClientsPage } from '../pages';

// Helper to click Spaces in sidebar (avoids space switcher button)
async function goToSpaces(page: import('@playwright/test').Page) {
  await page.locator('nav button:has-text("Spaces")').last().click();
}

test.describe('ConfirmDialog – Spaces', () => {
  test('should show confirm dialog when clicking delete on a non-default space', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await goToSpaces(page);
    await expect(page.locator('h1:has-text("Workspaces")')).toBeVisible();

    // Look for a delete button
    const deleteBtn = page.locator('[data-testid^="delete-space-"]').first();
    if (await deleteBtn.isVisible().catch(() => false)) {
      await deleteBtn.click();

      // Confirm dialog should appear
      await expect(page.getByTestId('confirm-dialog')).toBeVisible();
      await expect(page.getByTestId('confirm-dialog-confirm')).toBeVisible();
      await expect(page.getByTestId('confirm-dialog-cancel')).toBeVisible();

      // Title should mention delete
      await expect(page.getByTestId('confirm-dialog').locator('h3')).toContainText(/[Dd]elete/);
    }
  });

  test('should dismiss confirm dialog on cancel without deleting', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await goToSpaces(page);
    await expect(page.locator('h1:has-text("Workspaces")')).toBeVisible();

    const deleteBtn = page.locator('[data-testid^="delete-space-"]').first();
    if (await deleteBtn.isVisible().catch(() => false)) {
      // Count spaces before
      const spaceBefore = await page.locator('[data-testid^="space-card-"]').count();

      await deleteBtn.click();
      await expect(page.getByTestId('confirm-dialog')).toBeVisible();

      // Click cancel
      await page.getByTestId('confirm-dialog-cancel').click();

      // Dialog should close
      await expect(page.getByTestId('confirm-dialog')).not.toBeVisible();

      // Space count should be the same (nothing was deleted)
      const spaceAfter = await page.locator('[data-testid^="space-card-"]').count();
      expect(spaceAfter).toBe(spaceBefore);
    }
  });

  test('should dismiss confirm dialog when clicking overlay', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await goToSpaces(page);
    await expect(page.locator('h1:has-text("Workspaces")')).toBeVisible();

    const deleteBtn = page.locator('[data-testid^="delete-space-"]').first();
    if (await deleteBtn.isVisible().catch(() => false)) {
      await deleteBtn.click();
      await expect(page.getByTestId('confirm-dialog')).toBeVisible();

      // Click overlay (outside the dialog)
      await page.getByTestId('confirm-dialog-overlay').click({ position: { x: 5, y: 5 } });

      // Dialog should close
      await expect(page.getByTestId('confirm-dialog')).not.toBeVisible();
    }
  });
});

test.describe('ConfirmDialog – Clients', () => {
  test('should show confirm dialog when clicking Remove Client', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    await expect(page.getByRole('heading', { name: 'Connected Clients' })).toBeVisible();

    // Click the first client card to open the detail panel
    const clientCards = page.locator('[data-testid^="client-card-"]');
    const count = await clientCards.count();

    if (count > 0) {
      await clientCards.first().click();

      // Wait for panel to open
      await page.waitForTimeout(300);

      // Find the Remove Client button in the panel
      const removeBtn = page.getByRole('button', { name: /Remove Client/i });
      if (await removeBtn.isVisible().catch(() => false)) {
        await removeBtn.click();

        // Confirm dialog should appear
        await expect(page.getByTestId('confirm-dialog')).toBeVisible();
        await expect(page.getByTestId('confirm-dialog-confirm')).toHaveText(/Remove/i);

        // Cancel should dismiss without removing
        await page.getByTestId('confirm-dialog-cancel').click();
        await expect(page.getByTestId('confirm-dialog')).not.toBeVisible();

        // Client should still be there
        const countAfter = await clientCards.count();
        expect(countAfter).toBe(count);
      }
    }
  });
});
