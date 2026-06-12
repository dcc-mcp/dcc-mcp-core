import { test, expect, type Page } from '@playwright/test';

type MockState = {
  catalog: object[];
  installed: object[];
  sources: object[];
  outdated: object[];
};

async function clickMarketplaceTab(page: Page, name: string | RegExp) {
  const tab = page.getByRole('tab', { name });
  await expect(tab).toBeVisible();
  await tab.click({ force: true });
  await expect(tab).toHaveAttribute('aria-selected', 'true');
  return tab;
}

async function gotoMarketplace(page: Page, url = '/admin/?panel=marketplace') {
  await page.goto(url, { timeout: 45_000, waitUntil: 'domcontentloaded' });
  await expect(page.locator('.marketplace-panel')).toBeVisible();
}

async function openMarketplaceSources(page: Page) {
  const tab = await clickMarketplaceTab(page, 'Sources');
  await expect(page.locator('.marketplace-sources-section')).toBeVisible();
  return tab;
}

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
    } else if (path === '/marketplace/sources' && method === 'GET') {
      body = { sources: state.sources };
    } else if (path === '/marketplace/sources' && method === 'POST') {
      const payload = route.request().postDataJSON() as { source?: string };
      const entry = { name: payload.source ?? 'unknown', url: `https://github.com/${payload.source}`, origin: 'explicit' };
      state.sources.push(entry);
      body = { sources: state.sources };
    } else if (path === '/marketplace/outdated') {
      body = { dcc: null, count: state.outdated.length, packages: state.outdated };
    } else if (path === '/marketplace/update' && method === 'POST') {
      const payload = route.request().postDataJSON() as { name?: string; dcc?: string; all?: boolean };
      const updatedPkg = state.outdated.find(
        (pkg: any) => pkg.name === payload.name && pkg.dcc === payload.dcc,
      );
      if (updatedPkg) {
        state.outdated = state.outdated.filter(
          (pkg: any) => !(pkg.name === payload.name && pkg.dcc === payload.dcc),
        );
      }
      body = {
        updated: 1,
        results: [
          {
            updated: true,
            name: payload.name ?? 'unknown',
            dcc: payload.dcc ?? 'unknown',
            previous_version: updatedPkg ? (updatedPkg as any).installed_version : null,
            new_version: updatedPkg ? (updatedPkg as any).latest_version : '9.9.9',
            path: `/fake/path/${payload.name}`,
            install_type: 'git',
            source_name: 'builtin',
            source_url: 'https://github.com/dcc-mcp/marketplace',
            reload_required: true,
          },
        ],
      };
    } else if (path === '/marketplace/install' && method === 'POST') {
      const payload = route.request().postDataJSON() as { name?: string; dcc?: string; force?: boolean };
      state.installed.push({
        name: payload.name ?? 'unknown',
        dcc: payload.dcc ?? 'unknown',
        version: '1.0.0',
        install_type: 'git',
        path: `/fake/path/${payload.name}`,
        source_name: 'builtin',
        source_url: 'https://github.com/dcc-mcp/marketplace',
        install_url: null,
        install_ref: null,
        installed_at_ms: Date.now(),
      });
      body = {
        installed: true,
        name: payload.name,
        dcc: payload.dcc,
        version: '1.0.0',
        path: `/fake/path/${payload.name}`,
        skill_search_path: '/fake/path',
        install_type: 'git',
        reload_required: true,
      };
    } else if (path === '/marketplace/uninstall' && method === 'POST') {
      const payload = route.request().postDataJSON() as { name?: string; dcc?: string };
      state.installed = state.installed.filter(
        (pkg: any) => !(pkg.name === payload.name && pkg.dcc === payload.dcc),
      );
      body = {
        uninstalled: true,
        name: payload.name,
        dcc: payload.dcc,
        path: `/fake/path/${payload.name}`,
        removed_state: true,
        removed_files: true,
        reload_required: true,
      };
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
  test.describe.configure({ timeout: 60_000 });

  test('loads marketplace data from the root admin URL through the dev API mock', async ({ page }) => {
    test.setTimeout(60_000);

    await gotoMarketplace(page, '/?panel=discover&discoverTab=marketplace');

    const panel = page.locator('.marketplace-panel');
    await expect(panel.locator('h2')).toContainText('Marketplace');
    await expect(panel.locator('.marketplace-card')).toHaveCount(3);
    const statusText = await panel.locator('.status-bar').textContent();
    expect(statusText ?? '').not.toContain('Unexpected token');
    expect(statusText ?? '').not.toContain('<!doctype');
    await expect(page.getByRole('tab', { name: 'Installed' })).toHaveCount(1);
    await expect(page.getByRole('tab', { name: 'Sources' })).toHaveCount(1);
    await expect(page.getByRole('tab', { name: /Installed\\d/ })).toHaveCount(0);
    await expect(page.getByRole('tab', { name: /Sources\\d/ })).toHaveCount(0);
  });

  test('shows a localized gateway diagnostic when catalog returns HTML', async ({ page }) => {
    await page.route('**/admin/api/**', async (route) => {
      const url = new URL(route.request().url());
      const path = url.pathname.replace(/^\/admin\/api/, '');
      const method = route.request().method();

      if (path === '/marketplace/catalog') {
        await route.fulfill({
          status: 200,
          contentType: 'text/html',
          body: '<!doctype html><title>Vite dev shell</title>',
        });
        return;
      }

      let body: unknown;
      let status = 200;
      if (path === '/health') {
        body = { status: 'ok', instances_ready: 1, instances_total: 2, uptime_secs: 3723, version: '0.17.7', rss_bytes: 2097152 };
      } else if (path === '/marketplace/installed') {
        body = { packages: [] };
      } else if (path === '/marketplace/sources') {
        body = { sources: [] };
      } else if (path === '/marketplace/outdated') {
        body = { dcc: null, count: 0, packages: [] };
      } else {
        status = 404;
        body = { error: `Unhandled test route: ${method} ${path}` };
      }

      await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
    });

    await gotoMarketplace(page);

    const statusLine = page.locator('.marketplace-panel .status-bar');
    const emptyState = page.locator('.marketplace-content .empty');
    await expect(statusLine).toContainText('Marketplace API returned an HTML page instead of JSON.');
    await expect(emptyState).toContainText('Marketplace API returned an HTML page instead of JSON.');
    await expect(statusLine).not.toContainText('Unexpected token');
    await expect(statusLine).not.toContainText('<!doctype');
    await expect(emptyState).not.toContainText('Unexpected token');
    await expect(emptyState).not.toContainText('<!doctype');
  });

  test('shows empty state when no packages are available', async ({ page }) => {
    await mockMarketplaceApi(page, { catalog: [], installed: [], sources: [], outdated: [] });
    await gotoMarketplace(page);

    await expect(page.locator('.marketplace-panel h2')).toContainText('Marketplace');
    await expect(page.locator('.discover-panel > .discover-tabs')).toHaveCSS('display', 'flex');
    await expect(page.locator('.discover-panel > .discover-tabs')).toHaveCSS('gap', /[1-9]/);

    // Browse tab should be active and show empty state
    const browseTab = page.locator('.marketplace-tab').first();
    await expect(browseTab).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('.marketplace-content .empty')).toContainText(
      'No marketplace packages available.',
    );

    // Switch to Installed tab — also empty
    await clickMarketplaceTab(page, /Installed/);
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
      sources: [],
      outdated: [],
    });
    await gotoMarketplace(page);

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
      sources: [],
      outdated: [],
    });
    await gotoMarketplace(page);

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
      sources: [],
      outdated: [],
    });
    await gotoMarketplace(page);

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
      sources: [],
      outdated: [],
    });
    await gotoMarketplace(page);

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
      sources: [],
      outdated: [],
    });
    // Health query is only enabled on health / debug / setup panels.
    // Visit health panel first, then navigate to marketplace via SPA link
    // (client-side nav keeps TanStack Query cache alive; page.goto would reload).
    await page.goto('/admin/?panel=health', { waitUntil: 'domcontentloaded' });
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

  // ── PIP-700 new tests ────────────────────────────────────────────────────

  test('displays source management section and lists sources', async ({ page }) => {
    await mockMarketplaceApi(page, {
      catalog: [],
      installed: [],
      sources: [
        { name: 'official', url: 'https://github.com/dcc-mcp/marketplace', origin: 'builtin' },
        { name: 'td-core', url: 'https://github.com/td-core/skills', origin: 'config' },
      ],
      outdated: [],
    });
    await gotoMarketplace(page);

    // Click the Sources tab
    await openMarketplaceSources(page);
    await expect(page.locator('.marketplace-sources-section')).toBeVisible();
    await expect(page.locator('.marketplace-force-install')).toHaveCount(0);

    // Source rows should appear
    await expect(page.locator('.marketplace-source-row')).toHaveCount(2);
    await expect(page.locator('.marketplace-source-row').first()).toContainText('official');
    await expect(page.locator('.marketplace-source-row').last()).toContainText('td-core');
  });

  test('adds a source via input and button', async ({ page }) => {
    test.setTimeout(60_000);

    await mockMarketplaceApi(page, {
      catalog: [],
      installed: [],
      sources: [],
      outdated: [],
    });
    await gotoMarketplace(page);

    // Open Sources tab
    await openMarketplaceSources(page);

    // Initially empty
    await expect(page.locator('.marketplace-source-row')).toHaveCount(0);

    // Fill input and submit
    const sourceInput = page.locator('.marketplace-source-input');
    await expect(sourceInput).toBeVisible();
    await sourceInput.evaluate((input, value) => {
      const valueSetter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        'value',
      )?.set;
      valueSetter?.call(input, value);
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.dispatchEvent(new Event('change', { bubbles: true }));
    }, 'dcc-mcp/skills');
    await expect(page.locator('.marketplace-source-btn')).toBeEnabled();
    await page.locator('.marketplace-source-btn').evaluate((button) => {
      (button as HTMLButtonElement).click();
    });

    // Should show the new source
    await expect(page.locator('.marketplace-source-row')).toHaveCount(1);
    await expect(page.locator('.marketplace-source-row').first()).toContainText('dcc-mcp/skills');
  });

  test('shows force reinstall checkbox', async ({ page }) => {
    await mockMarketplaceApi(page, {
      catalog: [],
      installed: [],
      sources: [],
      outdated: [],
    });
    await gotoMarketplace(page);

    // Force reinstall checkbox should be visible
    await expect(page.locator('.marketplace-force-install')).toBeVisible();
    await expect(page.locator('.marketplace-force-install input[type="checkbox"]')).toBeVisible();
  });

  test('sends force=true when force reinstall is enabled', async ({ page }) => {
    const installPayloads: Array<{ name?: string; dcc?: string; force?: boolean }> = [];

    await mockMarketplaceApi(page, {
      catalog: [
        {
          name: 'maya-modeling',
          description: 'Maya tools.',
          dcc: ['maya'],
          tags: [],
          version: '2.1.0',
        },
      ],
      installed: [],
      sources: [],
      outdated: [],
    });
    await page.route('**/admin/api/marketplace/install', async (route) => {
      installPayloads.push(route.request().postDataJSON());
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          installed: true,
          name: 'maya-modeling',
          dcc: 'maya',
          version: '2.1.0',
          path: '/fake/path/maya-modeling',
          skill_search_path: '/fake/path',
          install_type: 'git',
          reload_required: true,
        }),
      });
    });

    await gotoMarketplace(page);
    await page.locator('.marketplace-force-install input[type="checkbox"]').check();
    await page.locator('.marketplace-card-chip-action').first().click();

    await expect(page.locator('.marketplace-install-notice')).toContainText('maya-modeling');
    expect(installPayloads).toEqual([
      {
        name: 'maya-modeling',
        dcc: 'maya',
        force: true,
      },
    ]);
  });

  test('shows outdated badge on installed packages and update button', async ({ page }) => {
    await mockMarketplaceApi(page, {
      catalog: [
        {
          name: 'maya-modeling',
          description: 'Maya tools.',
          dcc: ['maya'],
          tags: [],
          version: '2.1.0',
          maintainer: 'td-core',
        },
      ],
      installed: [
        {
          name: 'maya-modeling',
          dcc: 'maya',
          version: '1.0.0',
          install_type: 'git',
          path: '/fake/path/maya-modeling',
          source_name: 'builtin',
          source_url: 'https://github.com/dcc-mcp/marketplace',
          install_url: null,
          install_ref: null,
          installed_at_ms: Date.now(),
        },
      ],
      sources: [],
      outdated: [
        {
          name: 'maya-modeling',
          dcc: 'maya',
          installed_version: '1.0.0',
          latest_version: '2.1.0',
          source_name: 'builtin',
          source_url: 'https://github.com/dcc-mcp/marketplace',
          install_type: 'git',
          install_url: null,
          install_ref: null,
          path: '/fake/path/maya-modeling',
        },
      ],
    });
    await gotoMarketplace(page);

    await expect(page.locator('.marketplace-summary-item.warn strong')).toHaveText('1');
    const installedTab = page.getByRole('tab', { name: 'Installed' });
    await expect(installedTab).toHaveAttribute('aria-selected', 'false');
    await expect(installedTab.locator('.marketplace-tab-count')).toHaveText(['1', 'Updates 1']);
    await expect(installedTab.locator('.marketplace-tab-count-warn')).toHaveText('Updates 1');

    // Switch to Installed tab
    await clickMarketplaceTab(page, 'Installed');

    // Installed tab uses a dense inventory list instead of browse cards.
    await expect(page.locator('.marketplace-installed-list')).toBeVisible();
    await expect(page.locator('.marketplace-installed-row')).toHaveCount(1);
    await expect(page.locator('.marketplace-grid')).toHaveCount(0);

    // Outdated badge text and direct update action live on the row.
    const row = page.locator('.marketplace-installed-row[data-name="maya-modeling"]');
    await expect(row).toContainText('Update available');
    await expect(row).toContainText('/fake/path/maya-modeling');
    await expect(row).toContainText('builtin');

    await expect(row.locator('.marketplace-installed-action.is-primary')).toContainText('Update');
  });

  test('shows reload triggered notice after install', async ({ page }) => {
    await mockMarketplaceApi(page, {
      catalog: [
        {
          name: 'maya-modeling',
          description: 'Maya tools.',
          dcc: ['maya'],
          tags: [],
          version: '2.1.0',
        },
      ],
      installed: [],
      sources: [],
      outdated: [],
    });
    await gotoMarketplace(page);

    // Click install
    await page.locator('.marketplace-card-chip-action').first().click();

    // Notice should contain reload hint
    const notice = page.locator('.marketplace-install-notice');
    await expect(notice).toBeVisible();
    await expect(notice.locator('.marketplace-reload-hint')).toContainText('Skill reload triggered');
  });

  test('shows structured install error for already_installed', async ({ page }) => {
    // Override the default mock with an error response for install
    await page.route('**/admin/api/**', async (route) => {
      const url = new URL(route.request().url());
      const path = url.pathname.replace(/^\/admin\/api/, '');
      const method = route.request().method();
      let body: unknown;
      let status = 200;

      if (path === '/health') {
        body = { status: 'ok', instances_ready: 1, instances_total: 2, uptime_secs: 3723, version: '0.17.7', rss_bytes: 2097152 };
      } else if (path === '/marketplace/catalog') {
        body = { entries: [{ name: 'maya-modeling', description: 'Maya tools.', dcc: ['maya'], tags: [], version: '2.1.0' }] };
      } else if (path === '/marketplace/installed') {
        body = { packages: [] };
      } else if (path === '/marketplace/sources') {
        body = { sources: [] };
      } else if (path === '/marketplace/outdated') {
        body = { dcc: null, count: 0, packages: [] };
      } else if (path === '/marketplace/install' && method === 'POST') {
        status = 400;
        body = { error: { kind: 'already_installed', message: 'Package maya-modeling is already installed for DCC maya' } };
      } else {
        status = 404;
        body = { error: `Unhandled test route: ${method} ${path}` };
      }

      await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
    });

    await gotoMarketplace(page);

    // Click install
    await page.locator('.marketplace-card-chip-action').first().click();

    // Status line should show the friendly error
    await expect(page.locator('.marketplace-panel .status-bar')).toContainText('Already installed');
  });

  test('shows a localized gateway diagnostic when install returns HTML', async ({ page }) => {
    await page.route('**/admin/api/**', async (route) => {
      const url = new URL(route.request().url());
      const path = url.pathname.replace(/^\/admin\/api/, '');
      const method = route.request().method();

      if (path === '/marketplace/install' && method === 'POST') {
        await route.fulfill({
          status: 500,
          contentType: 'text/html',
          body: '<!doctype html><title>Vite dev shell</title>',
        });
        return;
      }

      let body: unknown;
      let status = 200;
      if (path === '/health') {
        body = { status: 'ok', instances_ready: 1, instances_total: 2, uptime_secs: 3723, version: '0.17.7', rss_bytes: 2097152 };
      } else if (path === '/marketplace/catalog') {
        body = { entries: [{ name: 'maya-modeling', description: 'Maya tools.', dcc: ['maya'], tags: [], version: '2.1.0' }] };
      } else if (path === '/marketplace/installed') {
        body = { packages: [] };
      } else if (path === '/marketplace/sources') {
        body = { sources: [] };
      } else if (path === '/marketplace/outdated') {
        body = { dcc: null, count: 0, packages: [] };
      } else {
        status = 404;
        body = { error: `Unhandled test route: ${method} ${path}` };
      }

      await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
    });

    await gotoMarketplace(page);
    await page.locator('.marketplace-card-chip-action').first().click();

    const statusLine = page.locator('.marketplace-panel .status-bar');
    await expect(statusLine).toContainText('Marketplace API returned an HTML page instead of JSON.');
    await expect(statusLine).not.toContainText('Unexpected token');
    await expect(statusLine).not.toContainText('<!doctype');
  });

  test('shows the same gateway diagnostic when install returns a 200 HTML shell', async ({ page }) => {
    await page.route('**/admin/api/**', async (route) => {
      const url = new URL(route.request().url());
      const path = url.pathname.replace(/^\/admin\/api/, '');
      const method = route.request().method();

      if (path === '/marketplace/install' && method === 'POST') {
        await route.fulfill({
          status: 200,
          contentType: 'text/html',
          body: '<!doctype html><title>Vite dev shell</title>',
        });
        return;
      }

      let body: unknown;
      let status = 200;
      if (path === '/health') {
        body = { status: 'ok', instances_ready: 1, instances_total: 2, uptime_secs: 3723, version: '0.17.7', rss_bytes: 2097152 };
      } else if (path === '/marketplace/catalog') {
        body = { entries: [{ name: 'maya-modeling', description: 'Maya tools.', dcc: ['maya'], tags: [], version: '2.1.0' }] };
      } else if (path === '/marketplace/installed') {
        body = { packages: [] };
      } else if (path === '/marketplace/sources') {
        body = { sources: [] };
      } else if (path === '/marketplace/outdated') {
        body = { dcc: null, count: 0, packages: [] };
      } else {
        status = 404;
        body = { error: `Unhandled test route: ${method} ${path}` };
      }

      await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
    });

    await gotoMarketplace(page);
    await page.locator('.marketplace-card-chip-action').first().click();

    const statusLine = page.locator('.marketplace-panel .status-bar');
    await expect(statusLine).toContainText('Marketplace API returned an HTML page instead of JSON.');
    await expect(statusLine).not.toContainText('Unexpected token');
    await expect(statusLine).not.toContainText('<!doctype');
  });
});
