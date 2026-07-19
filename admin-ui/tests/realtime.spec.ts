import { test, expect } from '@playwright/test';

test.describe('Real-time infrastructure', () => {
  test('panels use polling when no SSE connection', async ({ page }) => {
    // Mock all API endpoints
    await page.route('**/admin/api/**', async (route) => {
      await route.fulfill({ status: 200, json: { status: 'ok' } });
    });

    // Mock SSE endpoint to fail (simulate unavailable)
    await page.route('**/admin/api/events', async (route) => {
      await route.fulfill({ status: 404 });
    });

    await page.goto('/admin/?panel=sessions');

    // Panel should still render (using polling fallback)
    const panel = page.locator('section[data-panel="sessions"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
  });

  test('panels handle connection errors gracefully', async ({ page }) => {
    await page.route('**/admin/api/**', async (route) => {
      await route.abort('connectionrefused');
    });

    await page.goto('/admin/?panel=sessions');

    // Panel should still render with error state
    const panel = page.locator('section[data-panel="sessions"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
  });
});
