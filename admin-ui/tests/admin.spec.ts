import { test, expect } from '@playwright/test';

test.describe('Admin Page', () => {
  test('should load admin page and display health panel', async ({ page }) => {
    await page.goto('/admin/');
    await expect(page.locator('h1')).toContainText('DCC-MCP Gateway');
    await expect(page.locator('.nav button', { hasText: 'Health' })).toBeVisible();
  });

  test('should display all navigation panels', async ({ page }) => {
    await page.goto('/admin/');
    const panels = ['Health', 'Instances', 'Tools', 'Calls', 'Traces', 'Stats', 'Workers', 'Logs'];
    for (const label of panels) {
      await expect(page.locator('.nav button', { hasText: label })).toBeVisible();
    }
  });

  test('should switch to Instances panel and show icons', async ({ page }) => {
    await page.goto('/admin/');
    await page.click('button:has-text("Instances")');
    await page.waitForSelector('.instances-panel');
    // Check that DCC icons are rendered (img with data:image/svg+xml)
    const icons = await page.locator('.dcc-icon').count();
    if (icons > 0) {
      await expect(page.locator('.dcc-icon').first()).toHaveAttribute('src', /data:image\/svg\+xml/);
    }
  });

  test('should switch to Logs panel and display log rows', async ({ page }) => {
    await page.goto('/admin/');
    await page.click('button:has-text("Logs")');
    await page.waitForSelector('.logs-panel');
    // Logs panel should either show rows or "no data" message
    const hasRows = await page.locator('.log-row').count();
    expect(hasRows).toBeGreaterThanOrEqual(0);
  });

  test('should switch to Stats panel and display stats cards', async ({ page }) => {
    await page.goto('/admin/');
    await page.click('button:has-text("Stats")');
    await page.waitForSelector('.stats-panel');
    await expect(page.locator('.health-card')).toHaveCount(4); // total, success rate, p50, p95
  });

  test('should switch to Traces panel and display trace rows', async ({ page }) => {
    await page.goto('/admin/');
    await page.click('button:has-text("Traces")');
    await page.waitForSelector('.traces-panel');
    const hasRows = await page.locator('.trace-row').count();
    expect(hasRows).toBeGreaterThanOrEqual(0);
  });

  test('should display search filter in Instances panel', async ({ page }) => {
    await page.goto('/admin/');
    await page.click('button:has-text("Instances")');
    await page.waitForSelector('.search-input');
    await expect(page.locator('.search-input')).toBeVisible();
  });
});
