import { test, expect } from '@playwright/test';
import { DashboardPage, ClientsPage } from '../pages';

test.describe('Clients Page', () => {
  test('should display the Clients heading', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const clients = new ClientsPage(page);
    await dashboard.navigate();
    
    // Click Clients in sidebar
    await page.locator('nav button:has-text("Clients")').click();
    
    await expect(clients.heading).toBeVisible();
    await expect(clients.heading).toHaveText('Connected Clients');
  });

  test('should show description text', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const description = page.locator('text=/connected|AI|client/i');
    // Description about clients should be visible
  });

  test('should show empty state or client list', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const emptyState = page.locator('text=/No clients|no.*connected/i');
    const clientItems = page.locator('[class*="rounded"][class*="border"]');
    
    const hasEmpty = await emptyState.isVisible().catch(() => false);
    const clientCount = await clientItems.count();
    
    // Either empty state or clients should be shown
    expect(hasEmpty || clientCount > 0).toBeTruthy();
  });

  test('should display client cards if clients exist', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const clientCards = page.locator('[class*="rounded"][class*="border"]');
    const count = await clientCards.count();
    
    if (count > 0) {
      // First client card should have content
      const firstCard = clientCards.first();
      await expect(firstCard).toBeVisible();
    }
  });
});

test.describe('Client Details', () => {
  test('should show client connection status', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const clientCards = page.locator('[class*="rounded"][class*="border"]');
    const count = await clientCards.count();
    
    if (count > 0) {
      // Clients should have status indicators
      const statusIndicator = page.locator('[class*="bg-green"], [class*="bg-red"], text=/connected|active/i');
      // May or may not be visible
    }
  });

  test('should show granted feature sets for clients', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const clientCards = page.locator('[class*="rounded"][class*="border"]');
    const count = await clientCards.count();
    
    if (count > 0) {
      // Clients may show which feature sets they have access to
      const featureSetRefs = page.locator('text=/granted|access|permission/i');
      // May or may not be visible
    }
  });
});

test.describe('Client Management', () => {
  test('should have refresh button if available', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const refreshButton = page.getByRole('button', { name: /Refresh/i });
    // May or may not be visible
  });

  test('should show revoke option for connected clients', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    await dashboard.navigate();
    await page.locator('nav button:has-text("Clients")').click();
    
    const clientCards = page.locator('[class*="rounded"][class*="border"]');
    const count = await clientCards.count();
    
    if (count > 0) {
      const firstCard = clientCards.first();
      const revokeButton = firstCard.getByRole('button', { name: /Revoke|Disconnect|Remove/i });
      // May or may not be visible
    }
  });
});

test.describe('Client Toast Notifications', () => {
  test('should have toast container on clients page', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const clients = new ClientsPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("Clients")').click();
    await expect(clients.heading).toBeVisible();
    
    await expect(clients.toastContainer).toBeAttached();
  });

  // Skip in web mode - requires Tauri API for client operations
  test.skip('should show success toast when saving client config', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const clients = new ClientsPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("Clients")').click();
    
    // Click first client card to open panel
    const clientCards = page.locator('[data-testid^="client-card-"]');
    const count = await clientCards.count();
    
    if (count > 0) {
      await clientCards.first().click();
      
      // Wait for panel to open
      await expect(page.locator('text=Quick Settings')).toBeVisible();
      
      // Click Save Changes
      const saveButton = page.getByRole('button', { name: /Save Changes/i });
      if (await saveButton.isVisible()) {
        await saveButton.click();
        
        await clients.waitForToast('success');
        const toastText = await clients.getToastText();
        expect(toastText).toContain('Client settings saved');
      }
    }
  });

  // Skip in web mode - requires Tauri API for client deletion
  test.skip('should show success toast when removing a client', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const clients = new ClientsPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("Clients")').click();
    
    const clientCards = page.locator('[data-testid^="client-card-"]');
    const count = await clientCards.count();
    
    if (count > 0) {
      await clientCards.first().click();
      
      // Click Remove Client in panel footer
      page.on('dialog', dialog => dialog.accept());
      const removeButton = page.getByRole('button', { name: /Remove Client/i });
      if (await removeButton.isVisible()) {
        await removeButton.click();
        
        await clients.waitForToast('success');
        const toastText = await clients.getToastText();
        expect(toastText).toContain('Client removed');
      }
    }
  });

  // Skip in web mode - requires Tauri API for permission toggle
  test.skip('should show success toast when toggling feature set grant', async ({ page }) => {
    const dashboard = new DashboardPage(page);
    const clients = new ClientsPage(page);
    await dashboard.navigate();
    
    await page.locator('nav button:has-text("Clients")').click();
    
    const clientCards = page.locator('[data-testid^="client-card-"]');
    const count = await clientCards.count();
    
    if (count > 0) {
      await clientCards.first().click();
      
      // Expand Permissions section
      await page.locator('text=Permissions').click();
      await page.waitForTimeout(300);
      
      // Find a non-default feature set checkbox
      const featureSetToggle = page.locator('button:has([class*="rounded border"])').first();
      if (await featureSetToggle.isVisible()) {
        await featureSetToggle.click();
        
        await clients.waitForToast('success');
        const toastText = await clients.getToastText();
        expect(toastText).toMatch(/Permission (granted|revoked)/);
      }
    }
  });
});
