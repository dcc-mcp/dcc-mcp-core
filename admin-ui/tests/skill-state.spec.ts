import { expect, test } from '@playwright/test';
import { createTranslator, SUPPORTED_LOCALES } from '../src/i18n';
import { disableTestMotion, mockAdminApi } from './admin.fixtures';

for (const locale of SUPPORTED_LOCALES) {
  for (const width of [390, 1440]) {
    test.describe(`${locale} Skills state at ${width}px`, () => {
      test.use({ locale, viewport: { width, height: 900 } });
      test('separates current loading from recorded calls through search and detail', async ({ page }, testInfo) => {
        await disableTestMotion(page);
        await mockAdminApi(page);
        await page.route('**/admin/api/skills', async (route) => route.fulfill({ json: {
          total: 2, loaded: 1, unloaded: 1, action_count: 2,
          skills: [
            { name: 'maya-modeling', dcc_type: 'maya', loaded: false, action_count: 1,
              instance_count: 1, instances: ['12345678'], tools: ['create_sphere'],
              adoption: { used: true, call_count: 2 } },
            { name: 'blender-lookdev', dcc_type: 'blender', loaded: true, action_count: 1,
              instance_count: 1, instances: ['abcdef12'], tools: ['render_preview'],
              adoption: { used: false, call_count: 0 } },
          ],
        } }));
        await page.route('**/admin/api/skill-detail?*', async (route) => route.fulfill({ json: {
          skill: { name: 'maya-modeling', dcc_type: 'maya', state: 'unloaded',
            description: 'Indexed Maya modeling tools with historical calls.', tools: [] },
        } }));
        const t = createTranslator(locale);
        await page.goto('/admin/?panel=discover&discoverTab=skills');
        const row = page.locator('.skill-inventory-row[data-skill-name="maya-modeling"]');
        await expect(row).toContainText(t('skillPaths.state.unloaded'));
        await expect(row).toContainText(t('skillPaths.state.used'));
        await expect(page.locator('.skill-inventory-row[data-skill-name="blender-lookdev"]'))
          .toContainText(t('skillPaths.state.notUsed'));
        await expect(page.locator('html')).toHaveAttribute('lang', locale);
        await page.screenshot({ path: testInfo.outputPath('skills-loading-and-history.png'), fullPage: true });
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
        expect(overflow).toBeLessThanOrEqual(2);
        const search = page.locator('.list-search-wrap[data-panel="discover"] .list-search-input');
        await search.fill(t('skillPaths.state.loaded'));
        await expect(page.locator('.skill-inventory-row')).toHaveCount(1);
        await expect(page.locator('.skill-inventory-row')).toContainText('blender-lookdev');
        await search.fill(t('skillPaths.state.used'));
        await expect(page.locator('.skill-inventory-row')).toHaveCount(1);
        await expect(row).toBeVisible();
        await row.getByRole('button', { name: t('skillPaths.action.openDetail', { name: 'maya-modeling' }) }).click();
        const dialog = page.getByRole('dialog');
        await expect(dialog).toBeVisible();
        await expect(dialog.locator('.skill-detail-summary-grid strong').first())
          .toHaveText(t('skillPaths.detail.loadingState'));
        await expect(dialog.locator('.skill-detail-summary-grid > span').first())
          .toHaveText(`${t('skillPaths.detail.loadingState')}unloaded`);
        await expect(dialog.getByRole('button', { name: t('action.reload'), exact: true })).toBeEnabled();
        await page.screenshot({ path: testInfo.outputPath('skill-loading-detail.png'), fullPage: true });
        await page.keyboard.press('Escape');
        await expect(dialog).toBeHidden();
        await search.fill(t('skillPaths.state.notUsed'));
        await expect(page.locator('.skill-inventory-row')).toHaveCount(1);
        await expect(page.locator('.skill-inventory-row')).toContainText('blender-lookdev');
        await search.fill('');
        await expect(page.locator('.skill-inventory-row')).toHaveCount(2);
      });
    });
  }
}
