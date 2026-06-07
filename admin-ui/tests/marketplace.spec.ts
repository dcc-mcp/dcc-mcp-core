import { test, expect, type Page } from '@playwright/test';

type MockState = {
  catalog: object[];
  installed: object[];
};

async function mockMarketplaceApi(page: Page, state: MockState) {
  await page.route('**/admin/api/**', async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.replace(/^\/admin\/api/, '');
    const method = route.request().method();
    let body: unknown;
    let status = 200;

    if (path === '/health') {
      body = {
        status: 'ok',
        instances_ready: 1,
        instances_total: 2,
        uptime_secs: 3723,
        version: '0.17.7',
        rss_bytes: 2097152,
      };
    } else if (path === '/marketplace/catalog') {
      body = { entries: state.catalog };
    } else if (path === '/marketplace/installed') {
      body = { packages: state.installed };
    } else if (path === '/marketplace/install' && method === 'POST') {
      const payload = route.request().postDataJSON() as { name?: string; dcc?: string };
      state.installed.push({
        name: payload.name ?? 'unknown',
        dcc: payload.dcc ?? 'unknown',
        version: '1.0.0',
        install_type: 'git',
        path: `/fake/path/${payload.name}`,
        source_name: 'builtin',
      });
      body = { ok: true, name: payload.name, dcc: payload.dcc };
    } else if (path === '/marketplace/uninstall' && method === 'POST') {
      const payload = route.request().postDataJSON() as { name?: string; dcc?: string };
      state.installed = state.installed.filter(
        (pkg: any) => !(pkg.name === payload.name && pkg.dcc === payload.dcc),
      );
      body = { ok: true, name: payload.name, dcc: payload.dcc };
    } else {
      status = 404;
      body = { error: `Unhandled test route: ${method} ${path}` };
    }

    await route.fulfill({
      status,
      contentType: 'application/json',
      body: JSON.stringify(body),
    });
  });
}

test.describe('Marketplace Panel', () => {
  test('shows empty state when no packages are available', async ({ page }) => {
    await mockMarketplaceApi(page, { catalog: [], installed: [] });
    await page.goto('/admin/?panel=marketplace');

    await expect(page.locator('.marketplace-panel')).toBeVisible();
    await expect(page.locator('.marketplace-panel h2')).toContainText('Marketplace');

    // Browse tab should be active and show empty state
    const browseTab = page.locator('.marketplace-tab').first();
    await expect(browseTab).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('.marketplace-content .empty')).toContainText(
      'No marketplace packages available.',
    );

    // Switch to Installed tab — also empty
    await page.getByRole('tab', { name: /Installed/ }).click();
    await expect(page.locator('.marketplace-content .empty')).toContainText(
      "No packages installed yet.",
    );
  });

  test('shows empty state for search with no results', async ({ page }) => {
    await mockMarketplaceApi(page, {
      catalog: [
        {
          name: 'maya-modeling',
          description: 'Maya modeling tools.',
          dcc: ['maya'],
          tags: ['modeling', 'polygon'],
          version: '2.1.0',
          maintainer: 'td-core',
        },
      ],
      installed: [],
    });
    await page.goto('/admin/?panel=marketplace');

    // Should show the package
    await expect(page.locator('.marketplace-card')).toHaveCount(1);

    // Search for something that doesn't match
    await page.getByLabel('Filter current panel').fill('houdini');

    // Should show empty search message
    await expect(page.locator('.marketplace-content .empty')).toContainText(
      'No packages match your search.',
    );
  });

  test('installs a package via mock API and shows success notice', async ({ page }) => {
    await mockMarketplaceApi(page, {
      catalog: [
        {
          name: 'maya-modeling',
          description: 'A collection of modeling tools for Maya.',
          dcc: ['maya'],
          tags: ['modeling'],
          version: '2.1.0',
          maintainer: 'td-core',
        },
        {
          name: 'blender-lookdev',
          description: 'Look development utilities for Blender.',
          dcc: ['blender'],
          tags: ['shading', 'lookdev'],
          version: '0.5.0',
        },
      ],
      installed: [],
    });
    await page.goto('/admin/?panel=marketplace');

    await expect(page.locator('.marketplace-card')).toHaveCount(2);

    // The maya DCC chip should show "Install maya" text
    const mayaCard = page.locator('.marketplace-card[data-name="maya-modeling"]');
    await expect(mayaCard).toContainText('Install maya');

    // Click the install button
    await mayaCard.locator('.marketplace-card-chip-action').first().click();

    // Install success notice should appear
    const notice = page.locator('.marketplace-install-notice');
    await expect(notice).toBeVisible();
    await expect(notice).toContainText('maya-modeling');
    await expect(notice.locator('.marketplace-install-notice-link')).toContainText(
      'View in Skills',
    );

    // DCC chip should now show as installed (checkmark)
    await expect(mayaCard.locator('.marketplace-card-chip-installed')).toBeVisible();
  });

  test('shows DCC filter chips and filters the catalog grid', async ({ page }) => {
    await mockMarketplaceApi(page, {
      catalog: [
        {
          name: 'maya-modeling',
          description: 'Maya tools.',
          dcc: ['maya'],
          tags: [],
          version: '1.0.0',
        },
        {
          name: 'blender-kit',
          description: 'Blender tools.',
          dcc: ['blender'],
          tags: [],
          version: '1.0.0',
        },
        {
          name: 'cross-dcc-utils',
          description: 'Works everywhere.',
          dcc: ['maya', 'blender', 'houdini'],
          tags: [],
          version: '1.0.0',
        },
      ],
      installed: [],
    });
    await page.goto('/admin/?panel=marketplace');

    // DCC filter row should be visible with All + individual DCC chips
    const filterRow = page.locator('.marketplace-dcc-filter');
    await expect(filterRow).toBeVisible();
    await expect(filterRow.locator('.marketplace-dcc-chip')).toHaveCount(4); // All + blender + houdini + maya

    // All chip should be active by default
    const allChip = filterRow.locator('.marketplace-dcc-chip.active');
    await expect(allChip).toContainText('All');
    await expect(page.locator('.marketplace-card')).toHaveCount(3);

    // Click 'maya' chip
    await filterRow.getByRole('button', { name: 'maya' }).click();
    await expect(page.locator('.marketplace-card')).toHaveCount(2);
    await expect(page.locator('.marketplace-card[data-name="blender-kit"]')).not.toBeVisible();

    // Click 'maya' again to de-select (toggle off)
    await filterRow.getByRole('button', { name: 'maya' }).click();
    await expect(page.locator('.marketplace-card')).toHaveCount(3);

    // DCC filter + text search can combine
    await page.getByLabel('Filter current panel').fill('cross');
    await filterRow.getByRole('button', { name: 'houdini' }).click();
    await expect(page.locator('.marketplace-card')).toHaveCount(1);
    await expect(page.locator('.marketplace-card[data-name="cross-dcc-utils"]')).toBeVisible();
  });

  test('opens detail modal on card click and shows package metadata', async ({ page }) => {
    await mockMarketplaceApi(page, {
      catalog: [
        {
          name: 'maya-modeling',
          description: 'A collection of modeling tools for Maya.',
          dcc: ['maya'],
          tags: ['modeling', 'polygon', 'deformation'],
          version: '2.1.0',
          min_core_version: '0.15.0',
          maintainer: 'td-core',
          url: 'https://github.com/dcc-mcp/maya-modeling',
          source_name: 'builtin',
          install: { type: 'git', url: 'https://github.com/dcc-mcp/maya-modeling.git' },
        },
      ],
      installed: [],
    });
    await page.goto('/admin/?panel=marketplace');

    // Click the card to open detail modal
    await page.locator('.marketplace-card[data-name="maya-modeling"]').click();

    const modal = page.locator('.marketplace-detail-modal');
    await expect(modal).toBeVisible();
    await expect(modal.locator('.marketplace-detail-name')).toContainText('maya-modeling');
    await expect(modal.locator('.marketplace-detail-desc')).toContainText(
      'A collection of modeling tools for Maya.',
    );

    // Key-value grid
    await expect(modal).toContainText('2.1.0'); // version
    await expect(modal).toContainText('0.15.0'); // min_core_version
    await expect(modal).toContainText('td-core'); // maintainer
    await expect(modal).toContainText('git'); // install type
    await expect(modal).toContainText('builtin'); // source

    // Tags section
    await expect(modal).toContainText('modeling');
    await expect(modal).toContainText('polygon');
    await expect(modal).toContainText('deformation');

    // DCC section
    await expect(modal).toContainText('maya');

    // No compatibility warning (0.15.0 <= 0.17.7)
    await expect(modal.locator('.marketplace-detail-warning')).not.toBeVisible();

    // Close via backdrop click
    await page.locator('.marketplace-detail-backdrop').click({ position: { x: 10, y: 10 } });
    await expect(modal).not.toBeVisible();
  });

  test('shows compatibility warning when min_core_version exceeds current version', async ({
    page,
  }) => {
    await mockMarketplaceApi(page, {
      catalog: [
        {
          name: 'futuristic-tools',
          description: 'Requires a future core version.',
          dcc: ['maya'],
          tags: [],
          version: '3.0.0',
          min_core_version: '9.9.9',
          maintainer: 'future-team',
        },
      ],
      installed: [],
    });
    // Health query is only enabled on health / debug / setup panels.
    // Visit health panel first, then navigate to marketplace via SPA link
    // (client-side nav keeps TanStack Query cache alive; page.goto would reload).
    await page.goto('/admin/?panel=health');
    await expect(page.locator('.health-panel')).toContainText('0.17.7');
    await page.getByRole('navigation').getByRole('link', { name: 'Marketplace' }).click();
    await expect(page.locator('.marketplace-panel')).toBeVisible();

    // Open detail modal
    await page.locator('.marketplace-card[data-name="futuristic-tools"]').click();

    const modal = page.locator('.marketplace-detail-modal');
    await expect(modal).toBeVisible();

    // Compatibility warning alert should be visible
    const warning = modal.locator('.marketplace-detail-warning');
    await expect(warning).toBeVisible();
    await expect(warning).toHaveAttribute('role', 'alert');
    await expect(warning).toContainText('9.9.9');
    await expect(warning).toContainText('0.17.7');

    // min_core_version in the KV grid should have warning styling
    await expect(modal.locator('.marketplace-detail-kv-warn')).toContainText('9.9.9');

    // Close via Escape key
    await page.keyboard.press('Escape');
    await expect(modal).not.toBeVisible();
  });
});
