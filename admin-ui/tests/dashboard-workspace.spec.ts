import { expect, test, type Locator, type Page, type TestInfo } from '@playwright/test';
import { disableTestMotion, mockAdminApi, now } from './admin.fixtures';

// Browser-level UI acceptance with deterministic API fixtures. These tests do
// not exercise a live gateway or DCC backend.
async function navigate(page: Page, name: string) {
  if (page.viewportSize()!.width <= 760) {
    await page.locator('.mobile-navigation-trigger').click();
    await page.getByRole('option', { name, exact: true }).click();
  } else {
    await page.locator('.nav-links').getByRole('link', { name, exact: true }).click();
  }
}

async function evidence(page: Page, testInfo: TestInfo, name: string, viewportOnly = false) {
  const path = testInfo.outputPath(`${name}.png`);
  // Fixed sheets and scrolled trace evidence need actual viewport captures.
  // Long inventories use full-page output only before interaction scrolls them.
  await page.screenshot({ path, fullPage: !viewportOnly });
  await testInfo.attach(name, { path, contentType: 'image/png' });
}

async function expectSheet(page: Page, dialog: Locator, exerciseKeyboard = true) {
  await expect(dialog).toBeVisible();
  await expect(dialog).toBeInViewport({ ratio: 1 });
  await expect(dialog.getByRole('button', { name: 'Close', exact: true })).toBeInViewport();
  if (!exerciseKeyboard) return;
  // Check keyboard containment in both directions, including wrap-around.
  for (let index = 0; index < 12; index += 1) {
    await page.keyboard.press(index < 6 ? 'Tab' : 'Shift+Tab');
    await expect.poll(() => dialog.evaluate((node) => node.contains(document.activeElement))).toBe(true);
  }
}

async function mockMarketplace(page: Page) {
  const entry = {
    name: 'maya-modeling', description: 'Modeling tools for a studio workspace.',
    dcc: ['maya'], tags: ['modeling'], version: '2.1.0', maintainer: 'td-core',
  };
  await page.route('**/admin/api/marketplace/**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    const body = path.endsWith('/catalog') ? { entries: [entry] }
      : path.endsWith('/installed') ? { packages: [{
        name: entry.name, dcc: 'maya', version: '2.1.0', install_type: 'git',
        path: '/studio/skills/maya-modeling', source_name: 'builtin',
        source_url: 'https://github.com/dcc-mcp/marketplace',
        install_url: null, install_ref: null, installed_at_ms: 1779091200000,
      }] }
        : path.endsWith('/outdated') ? { dcc: null, count: 0, packages: [] }
          : { sources: [] };
    await route.fulfill({ json: body });
  });
}

for (const width of [390, 1440]) {
  test.describe(`Dashboard workspace at ${width}px`, () => {
    test.use({ viewport: { width, height: 900 }, locale: 'en-US' });
    test.beforeEach(async ({ page }) => {
      await disableTestMotion(page);
      await mockAdminApi(page);
    });

    test('all four navigation groups remain reachable through browser history', async ({ page }, testInfo) => {
      await page.goto('/admin/?panel=instances');
      await expect(page.locator('.instances-panel')).toBeVisible();
      await expect(page.locator('.instance-link-groups a')).toHaveCount(12);
      const clippedLinks = await page.locator('.instance-link-groups > span').evaluateAll((groups) => groups.some((group) => {
        const bounds = group.getBoundingClientRect();
        return bounds.right > document.documentElement.clientWidth + 1
          || Array.from(group.querySelectorAll('a')).some((link) => {
            const rect = link.getBoundingClientRect();
            return rect.left < bounds.left - 1 || rect.right > bounds.right + 1;
          });
      }));
      expect(clippedLinks).toBe(false);
      if (width > 760) {
        await expect(page.locator('.nav-section-title')).toHaveText(['Runtime', 'Extensions', 'Execution', 'Observability']);
        await expect(page.locator('.nav-links .nav-link')).toHaveCount(20);
      } else {
        await page.locator('.mobile-navigation-trigger').click();
        await expect(page.getByRole('option')).toHaveCount(20);
        for (const group of ['Runtime', 'Extensions', 'Execution', 'Observability']) {
          await expect(page.locator('.mobile-navigation-content').getByText(group, { exact: true })).toBeVisible();
        }
        await page.keyboard.press('Escape');
      }
      await evidence(page, testInfo, 'runtime-navigation');
      await navigate(page, 'Skills');
      await expect(page).toHaveURL(/panel=discover.*discoverTab=skills/);
      await expect(page.locator('.skill-paths-panel')).toBeVisible();
      await navigate(page, 'Tasks');
      await expect(page).toHaveURL(/panel=tasks/);
      await expect(page.locator('.task-card')).toHaveCount(2);
      await navigate(page, 'Traces');
      await expect(page).toHaveURL(/panel=traces/);
      await expect(page.locator('.traces-panel')).toBeVisible();
      await page.goBack();
      await expect(page).toHaveURL(/panel=tasks/);
      await expect(page.locator('.tasks-panel')).toBeVisible();
      await page.goBack();
      await expect(page).toHaveURL(/panel=discover.*discoverTab=skills/);
      await expect(page.locator('.skill-paths-panel')).toBeVisible();
      await page.goForward();
      await expect(page).toHaveURL(/panel=tasks/);
      await expect(page.locator('.tasks-panel')).toBeVisible();
    });

    test('skill sheet closes, restores focus, reopens, and leaves no history overlay', async ({ page }, testInfo) => {
      await page.goto('/admin/?panel=instances');
      await navigate(page, 'Skills');
      const opener = page.getByRole('button', { name: 'Open maya-modeling detail', exact: true });
      await expect(opener).toBeVisible();
      const sources = page.locator('.skill-path-table-wrap .source-pill');
      await expect(sources).toHaveCount(2);
      expect(await sources.evaluateAll((badges) => badges.some((badge) => badge.scrollHeight > badge.clientHeight + 1))).toBe(false);
      await evidence(page, testInfo, 'skills-inventory');
      await opener.click();
      const dialog = page.getByRole('dialog', { name: 'maya-modeling', exact: true });
      await expectSheet(page, dialog, false);
      await expect(dialog.locator('.skill-detail-path')).toContainText('SKILL.md');
      await evidence(page, testInfo, 'skill-detail-initial', true);
      await expectSheet(page, dialog);
      await dialog.evaluate((node) => { node.scrollTop = 180; });
      if (width === 390) await expect.poll(() => dialog.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
      await expect.poll(() => dialog.evaluate((node) => {
        const bounds = node.getBoundingClientRect();
        const heading = node.querySelector('.skill-detail-heading')!;
        const headingBounds = heading.getBoundingClientRect();
        const hit = document.elementFromPoint(bounds.left + bounds.width / 2, bounds.top + 2);
        return headingBounds.top <= bounds.top + 1 && Boolean(hit?.closest('.skill-detail-heading'));
      })).toBe(true);
      await expect(dialog.getByRole('button', { name: 'Close', exact: true })).toBeInViewport();
      await evidence(page, testInfo, 'skill-detail-scrolled', true);
      await page.keyboard.press('Escape');
      await expect(dialog).toBeHidden();
      await expect(opener).toBeFocused();
      await opener.click();
      await dialog.getByRole('button', { name: 'Close', exact: true }).click();
      await expect(dialog).toBeHidden();
      await expect(opener).toBeFocused();
      if (width > 760) {
        await opener.click();
        await page.locator('.detail-dialog-overlay').click({ position: { x: 10, y: 10 } });
        await expect(dialog).toBeHidden();
        await expect(opener).toBeFocused();
      }
      await opener.click();
      await expect(dialog).toBeVisible();
      await page.goBack();
      await expect(page.locator('.instances-panel')).toBeVisible();
      await expect(page.getByRole('dialog')).toHaveCount(0);
      await page.goForward();
      await expect(opener).toBeVisible();
      await expect(page.getByRole('dialog')).toHaveCount(0);
      await opener.click();
      await expectSheet(page, dialog);
    });

    test('catalog and installed package sheets support close and reopen', async ({ page }, testInfo) => {
      await mockMarketplace(page);
      await page.goto('/admin/?panel=discover&discoverTab=marketplace');
      const catalogOpener = page.locator('.marketplace-card[data-name="maya-modeling"] .marketplace-card-media-trigger');
      await catalogOpener.click();
      const dialog = page.getByRole('dialog');
      await expectSheet(page, dialog);
      await evidence(page, testInfo, 'marketplace-catalog-sheet', true);
      await page.keyboard.press('Escape');
      await expect(dialog).toBeHidden();
      await expect(catalogOpener).toBeFocused();
      await catalogOpener.click();
      await dialog.getByRole('button', { name: 'Close', exact: true }).click();
      await expect(dialog).toBeHidden();
      await expect(catalogOpener).toBeFocused();
      await page.getByRole('tab', { name: 'Installed', exact: true }).click();
      const installedOpener = page.locator('.marketplace-installed-row[data-name="maya-modeling"] .marketplace-installed-title');
      await installedOpener.click();
      await expectSheet(page, dialog);
      await evidence(page, testInfo, 'marketplace-installed-sheet', true);
      await dialog.getByRole('button', { name: 'Close', exact: true }).click();
      await expect(dialog).toBeHidden();
      await expect(installedOpener).toBeFocused();
      await installedOpener.click();
      await expect(dialog).toBeVisible();
      await page.keyboard.press('Escape');
      await expect(dialog).toBeHidden();
      await expect(installedOpener).toBeFocused();
    });

    test('session hierarchy and details remain usable on narrow and wide screens', async ({ page }, testInfo) => {
      await page.goto('/admin/?panel=sessions');
      const rows = page.locator('.sessions-row');
      await expect(rows).toHaveCount(3);
      const root = rows.filter({ hasText: 'Layout Agent' });
      await root.locator('.sessions-detail-btn').click();
      await expect(page.locator('.sessions-detail-row')).toContainText('sess-root-0001');
      await evidence(page, testInfo, 'sessions-expanded');
      await root.locator('.sessions-detail-btn').click();
      await expect(page.locator('.sessions-detail-row')).toHaveCount(0);
      await root.locator('.sessions-tree-btn').click();
      await expect(rows).toHaveCount(2);
      await page.locator('.sessions-search-input').fill('Rig Agent');
      await expect(rows).toHaveCount(1);
      await expect(rows).toContainText('Rig Agent');
      await rows.locator('.sessions-detail-btn').click();
      await expect(page.locator('.sessions-detail-row')).toContainText('sess-child-0002');
      await page.locator('.sessions-search-input').clear();
      await root.locator('.sessions-tree-btn').click();
      await expect(rows).toHaveCount(3);
    });

    test('task disclosure links to searchable trace steps and raw evidence', async ({ page }, testInfo) => {
      await page.goto('/admin/?panel=tasks');
      const task = page.locator('.task-card.ok').filter({ hasText: 'Create a sphere' });
      await expect(task.locator('.task-detail')).toBeHidden();
      await task.locator('.task-title-row').click();
      await expect(task.locator('.task-detail')).toBeVisible();
      await evidence(page, testInfo, 'tasks-expanded');
      await task.getByRole('button', { name: /trace req-123/ }).click();
      await expect(page).toHaveURL(/trace=req-123/);
      const detail = page.locator('.trace-detail-panel');
      await expect(detail).toContainText('Scene Builder');
      await page.evaluate(() => window.scrollTo(0, 0));
      await detail.locator('.trace-run-header').scrollIntoViewIfNeeded();
      await expect(detail.locator('.trace-run-header')).toBeInViewport({ ratio: 1 });
      await evidence(page, testInfo, 'trace-overview-viewport', true);
      const steps = detail.locator('.span-row');
      await expect(steps).toHaveCount(1);
      await detail.getByRole('searchbox', { name: 'Search steps' }).fill('missing-step');
      await expect(steps).toHaveCount(0);
      await detail.getByRole('searchbox', { name: 'Search steps' }).fill('dispatch');
      await expect(steps).toHaveCount(1);
      await detail.getByRole('combobox', { name: 'Filter step status' }).selectOption('failed');
      await expect(steps).toHaveCount(0);
      await detail.getByRole('combobox', { name: 'Filter step status' }).selectOption('all');
      await steps.locator('summary').click();
      await expect(steps.locator('.trace-step-content')).toBeVisible();
      await detail.locator('.agent-context-card > summary').click();
      await detail.locator('.agent-context-card .trace-raw-context > summary').click();
      await expect(detail.locator('.caller-context-pre')).toContainText('Layout Artist');
      await detail.locator('.trace-payload-section > summary').first().click();
      await expect(detail.locator('.trace-payload-section').first().locator('pre')).toContainText('"radius":1');
      await detail.locator('.trace-record-section > summary').click();
      await expect(detail.locator('.trace-record-grid').first()).toContainText('req-123');
      await detail.locator('.trace-record-section .trace-raw-context > summary').click();
      const rawRecord = detail.locator('.trace-record-section .trace-raw-context pre');
      await expect(rawRecord).toContainText('req-123');
      await rawRecord.scrollIntoViewIfNeeded();
      if (width === 390) {
        await expect(page.locator('.workspace-bar')).toBeInViewport();
        expect(await page.locator('.workspace-bar').evaluate((node) => Math.abs(node.getBoundingClientRect().top))).toBeLessThanOrEqual(1);
      }
      await evidence(page, testInfo, 'trace-evidence-viewport', true);
      await page.goBack();
      await expect(page.locator('.tasks-panel')).toBeVisible();
      await page.goForward();
      await expect(detail).toContainText('Scene Builder');
    });

    test('polling prepends logs without closing the expanded gateway event', async ({ page }, testInfo) => {
      const original = { timestamp: now, level: 'info', source: 'contention', dcc_type: 'gateway', message: 'Original gateway event', detail: 'Original event diagnostic detail' };
      const incoming = { ...original, timestamp: '2026-05-18T08:00:01.000Z', message: 'New gateway event', detail: 'New event diagnostic detail' };
      let prepend = false;
      await page.route('**/admin/api/logs*', async (route) => {
        const logs = prepend ? [incoming, original] : [original];
        await route.fulfill({ json: { logs, total: logs.length, server_version: '0.19.56' } });
      });
      await page.goto('/admin/?panel=logs');
      const entry = page.locator('.log-entry').filter({ hasText: 'Original gateway event' });
      await entry.locator('summary').click();
      await expect(entry).toHaveJSProperty('open', true);
      prepend = true;
      // Wait for the actual five-second query poll, without a manual refresh.
      await expect(page.locator('.log-entry')).toHaveCount(2, { timeout: 12_000 });
      await expect(entry).toHaveJSProperty('open', true);
      await expect(entry.locator('.log-entry-details')).toBeVisible();
      await expect(entry.locator('.log-entry-details')).toContainText('Original event diagnostic detail');
      await evidence(page, testInfo, 'logs-after-poll');
    });
  });
}
