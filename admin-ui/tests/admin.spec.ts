import { test, expect, type Page } from '@playwright/test';
import { chooseSidebarSelectOption, openIntegrationEditor, disableTestMotion, mockAdminApi } from './admin.fixtures';

test.beforeEach(async ({ page }) => {
  await disableTestMotion(page);
  await mockAdminApi(page);
});

test.describe('Admin Page', () => {
  test('loads the command center panel and navigation', async ({ page }) => {
    await page.goto('/admin/');
    await expect(page.locator('html')).toHaveAttribute('lang', 'en');
    await expect(page.locator('html')).toHaveAttribute('data-admin-locale', 'en');
    await expect(page.getByRole('img', { name: 'DCC MCP' })).toBeVisible();
    const preferenceBox = await page.locator('.sidebar-preferences').boundingBox();
    expect(preferenceBox?.height ?? 0).toBeLessThanOrEqual(48);
    await expect(page.locator('.sidebar-preferences .preference-icon')).toHaveCount(2);
    await expect(page.locator('#admin-locale-select .preference-select-visible-value')).toHaveText('EN');
    await expect(page.locator('#admin-locale-select')).toHaveAttribute('aria-label', 'Language: English');
    await expect(page.locator('#admin-locale-select')).toHaveText('EN');
    await expect(page.locator('.sidebar-preferences')).not.toContainText('navigator');
    await page.locator('#admin-locale-select').click();
    const localeMenu = page.locator('[data-slot="select-content"]').last();
    await expect(localeMenu).toBeVisible();
    const localeMenuStyle = await localeMenu.evaluate((node) => {
      const style = window.getComputedStyle(node as HTMLElement);
      return {
        backgroundColor: style.backgroundColor,
        borderRadius: style.borderRadius,
        color: style.color,
        opacity: style.opacity,
      };
    });
    expect(parseFloat(localeMenuStyle.borderRadius)).toBeLessThanOrEqual(8);
    expect(localeMenuStyle.backgroundColor).not.toBe('rgba(0, 0, 0, 0)');
    expect(localeMenuStyle.color).not.toBe(localeMenuStyle.backgroundColor);
    expect(localeMenuStyle.opacity).toBe('1');
    const localeMenuBox = await localeMenu.boundingBox();
    const localeTriggerBox = await page.locator('#admin-locale-select').boundingBox();
    expect(localeMenuBox?.width ?? 0).toBeGreaterThanOrEqual(localeTriggerBox?.width ?? 0);
    expect(localeMenuBox?.width ?? 0).toBeGreaterThanOrEqual(150);
    const localeItemMetrics = await page.locator('[data-slot="select-item"]').evaluateAll((nodes) =>
      nodes.map((node) => {
        const element = node as HTMLElement;
        const style = window.getComputedStyle(element);
        return {
          clientWidth: element.clientWidth,
          scrollWidth: element.scrollWidth,
          text: element.textContent?.replace(/\s+/g, ' ').trim() ?? '',
          whiteSpace: style.whiteSpace,
        };
      }),
    );
    expect(localeItemMetrics.length).toBeGreaterThanOrEqual(4);
    for (const metric of localeItemMetrics) {
      expect(metric.whiteSpace).toBe('nowrap');
      expect(metric.scrollWidth).toBeLessThanOrEqual(metric.clientWidth + 1);
    }
    const chineseOption = page.getByRole('option', { name: '简体中文' });
    await chineseOption.hover();
    const chineseOptionStyle = await chineseOption.evaluate((node) => {
      const style = window.getComputedStyle(node as HTMLElement);
      return {
        backgroundColor: style.backgroundColor,
        borderRadius: style.borderRadius,
      };
    });
    expect(parseFloat(chineseOptionStyle.borderRadius)).toBeLessThanOrEqual(6);
    expect(chineseOptionStyle.backgroundColor).not.toBe('rgb(0, 120, 215)');
    await page.keyboard.press('Escape');
    await chooseSidebarSelectOption(page, 'admin-theme-select', 'Light');
    await expect(page.locator('.brand-logo-image-light')).toBeVisible();
    await expect(page.locator('.brand-logo-image-dark')).toBeHidden();
    await chooseSidebarSelectOption(page, 'admin-theme-select', 'Dark');
    await expect(page.locator('.brand-logo-image-light')).toBeHidden();
    await expect(page.locator('.brand-logo-image-dark')).toBeVisible();
    await expect(page.locator('.command-agent-prompt [data-slot="button"]').first()).toBeVisible();
    await expect(page.locator('.brand-tag')).toContainText('DCC-MCP Gateway');
    await expect(page.locator('h1')).toContainText('Admin Dashboard');
    await expect(page.getByRole('navigation').getByRole('link', { name: 'Command Center' })).toHaveClass(/active/);
    for (const label of ['Command Center', 'Debug', 'Activity', 'Health', 'Instances', 'Tools', 'Workflows', 'Tasks', 'Calls', 'Traces', 'Overview', 'Analytics', 'Memory', 'Governance', 'OpenAPI Inspector', 'Skills', 'Marketplace', 'Integrations', 'Logs', 'Docs']) {
      await expect(page.getByRole('navigation').getByRole('link', { name: label })).toBeVisible();
    }
    await expect(page.getByRole('navigation').getByRole('link', { name: 'Docs' })).toHaveAttribute('href', 'https://github.com/dcc-mcp/dcc-mcp-core/tree/main/docs');
    await expect(page.locator('.setup-panel')).toContainText('Claude Desktop');
    await expect(page.locator('.setup-panel')).toContainText('DCC CLI Command Center');
    await expect(page.getByRole('tab', { name: /Agent Prompt/ })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByRole('tab', { name: /Agent CLI/ })).toBeVisible();
    await expect(page.getByRole('tab', { name: /Human CLI/ })).toBeVisible();
    await expect(page.locator('.command-agent-prompt')).toContainText('Gateway target: http://127.0.0.1:9765/mcp');
    await expect(page.locator('.command-agent-prompt')).toContainText('search for a real tool slug');
    const promptCopy = page.locator('.command-agent-prompt [data-slot="button"]');
    await promptCopy.click();
    await expect(promptCopy).toContainText('Copied');
    await expect(page.locator('.setup-panel')).toContainText('Copied Agent prompt');
    await page.getByRole('tab', { name: /Agent CLI/ }).click();
    await expect(page.getByRole('tab', { name: /Agent CLI/ })).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli list');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli search --query "create sphere" --dcc-type <dcc_type> --limit 20');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli describe <tool_slug>');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli call <tool_slug> --json');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli load-skill <skill_name>');
    await expect(page.locator('.setup-panel')).not.toContainText('dcc-mcp-cli update apply');
    await page.getByRole('tab', { name: /Human CLI/ }).click();
    await expect(page.getByRole('tab', { name: /Human CLI/ })).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli list');
    await expect(page.locator('.setup-panel')).not.toContainText('dcc-mcp-cli gateway ensure');
    await expect(page.locator('.setup-panel')).not.toContainText('dcc-mcp-cli call <tool_slug> --json');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli marketplace search --query "rigging" --dcc maya --limit 20');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli marketplace install <package_name> --dcc maya');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli marketplace update <package_name> --dcc maya');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli update check --binary dcc-mcp-server --current-version <server_version>');
    await expect(page.locator('.command-aside-action')).toHaveCount(2);
    await expect(page.locator('.command-aside-action').first()).toContainText('Manage loaded skills');
    await expect(page.locator('.command-aside-action').last()).toContainText('Install and update skill packages');
    await expect(page.locator('.command-center-aside')).toContainText('Session context');
    await expect(page.locator('.command-center-aside')).toContainText('Agent flow');
    await expect(page.locator('.command-center-aside')).toContainText('Search');
    await expect(page.locator('.command-center-aside')).toContainText('Describe');
    await expect(page.locator('.command-center-aside')).toContainText('Call');
    await expect(page.locator('.command-center-aside .command-aside-runtime-link[data-slot="button"]')).toHaveCount(1);
    await expect(page.locator('.command-center-aside .command-aside-runtime-link')).toContainText('Instances');
    const firstActionStyle = await page.locator('.command-aside-action').first().evaluate((button) => {
      const styles = getComputedStyle(button);
      return {
        borderStyle: styles.borderStyle,
        textDecoration: styles.textDecorationLine,
      };
    });
    expect(firstActionStyle.borderStyle).toBe('solid');
    expect(firstActionStyle.textDecoration).toBe('none');
    const firstCommandCopy = page.locator('.cli-command-row .cli-command-action').first();
    await expect(firstCommandCopy).toContainText('Copy');
    await firstCommandCopy.click();
    await expect(firstCommandCopy).toContainText('Copied');
    await expect(firstCommandCopy).toHaveAttribute('data-copied', 'true');
    await expect(page.locator('.setup-panel')).toContainText('Copied Auto gateway');
    await expect(page.locator('.setup-panel')).toContainText('http://127.0.0.1:9765/mcp');
    await expect(page.locator('.setup-panel')).not.toContainText('http://127.0.0.1:3721/mcp');
    await expect(page.locator('.setup-panel img.ide-icon')).toHaveCount(6);
    await expect(page.locator('.setup-panel .ide-config-preview').first()).toContainText('"dcc-mcp-gateway"');
    const codexCard = page.locator('.setup-panel .ide-card').filter({ hasText: 'Codex / OpenAI' });
    await expect(codexCard).toContainText('%USERPROFILE%\\.codex\\config.toml');
    await expect(codexCard.locator('.ide-config-preview')).toContainText('[mcp_servers.dcc-mcp-gateway]');
    await expect(codexCard.locator('.ide-config-preview')).toContainText('url = "http://127.0.0.1:9765/mcp"');
    await page.locator('.setup-panel .ide-card').first().getByRole('button', { name: 'Copy' }).click();
    await expect(page.locator('.setup-panel')).toContainText('Copied Claude Desktop config');
    await page.getByRole('button', { name: 'Direct' }).click();
    await expect(page.locator('.setup-panel')).toContainText('Maya Layout');
    await expect(page.locator('.setup-panel .ide-config-preview').first()).toContainText('http://127.0.0.1:8765/mcp');
    await page.getByRole('navigation').getByRole('link', { name: 'Debug' }).click();
    await expect(page.locator('.debug-panel')).toContainText('Debug Workbench');
    await expect(page.locator('.debug-panel')).toContainText('Agent Triage');
    await expect(page.locator('.debug-panel')).toContainText('Failed execution');
    await expect(page.locator('.debug-panel')).toContainText('req-err');
    await expect(page.locator('.debug-panel')).toContainText('Traffic Shape');
    await expect(page.locator('.debug-panel')).toContainText('Token Pressure');
    await expect(page.locator('.debug-panel')).toContainText('250 payload tokens');
    await page.getByRole('navigation').getByRole('link', { name: 'Health' }).click();
    await expect(page.locator('.health-panel')).toContainText('0.17.7');
    await expect(page.locator('.health-panel')).toContainText('toon / dcc-mcp-byte4-v1');
  });

  test('keeps setup IDE card action rows aligned', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto('/admin/');
    const cards = page.locator('.setup-panel .ide-card');
    await expect(cards).toHaveCount(6);

    const firstRow = await cards.evaluateAll((elements) => {
      const firstTop = elements[0]?.getBoundingClientRect().top ?? 0;
      return elements
        .filter((card) => Math.abs(card.getBoundingClientRect().top - firstTop) < 4)
        .map((card) => {
          const cardRect = card.getBoundingClientRect();
          const actions = card.querySelector('.ide-card-actions')?.getBoundingClientRect();
          const cardActions = Array.from(card.querySelectorAll('.ide-card-action'));
          const copy = cardActions[0]?.getBoundingClientRect();
          const open = cardActions[1]?.getBoundingClientRect();
          return {
            cardBottom: cardRect.bottom,
            actionsTop: actions?.top ?? 0,
            actionsBottom: actions?.bottom ?? 0,
            copyTop: copy?.top ?? 0,
            copyBottom: copy?.bottom ?? 0,
            openTop: open?.top ?? 0,
            openBottom: open?.bottom ?? 0,
          };
        });
    });
    expect(firstRow.length).toBeGreaterThan(1);
    const actionTops = firstRow.map((row) => row.actionsTop);
    const actionBottomOffsets = firstRow.map((row) => row.cardBottom - row.actionsBottom);
    expect(Math.max(...actionTops) - Math.min(...actionTops)).toBeLessThanOrEqual(4);
    expect(Math.max(...actionBottomOffsets) - Math.min(...actionBottomOffsets)).toBeLessThanOrEqual(2);
    for (const row of firstRow) {
      expect(row.cardBottom - row.actionsBottom).toBeLessThanOrEqual(20);
      expect(Math.abs(row.copyTop - row.openTop)).toBeLessThanOrEqual(2);
      expect(Math.abs(row.copyBottom - row.openBottom)).toBeLessThanOrEqual(2);
    }

    await page.setViewportSize({ width: 420, height: 900 });
    await page.goto('/admin/');
    const narrowRows = await cards.evaluateAll((elements) => elements.map((card) => {
      const cardRect = card.getBoundingClientRect();
      const actions = card.querySelector('.ide-card-actions')?.getBoundingClientRect();
      const cardActions = Array.from(card.querySelectorAll('.ide-card-action'));
      const copy = cardActions[0]?.getBoundingClientRect();
      const open = cardActions[1]?.getBoundingClientRect();
      return {
        cardLeft: cardRect.left,
        cardRight: cardRect.right,
        cardBottom: cardRect.bottom,
        actionsBottom: actions?.bottom ?? 0,
        copyLeft: copy?.left ?? 0,
        copyRight: copy?.right ?? 0,
        copyTop: copy?.top ?? 0,
        copyBottom: copy?.bottom ?? 0,
        openLeft: open?.left ?? 0,
        openRight: open?.right ?? 0,
        openTop: open?.top ?? 0,
        openBottom: open?.bottom ?? 0,
      };
    }));
    for (const row of narrowRows) {
      expect(row.copyLeft).toBeGreaterThanOrEqual(row.cardLeft);
      expect(row.openLeft).toBeGreaterThanOrEqual(row.cardLeft);
      expect(row.copyRight).toBeLessThanOrEqual(row.cardRight);
      expect(row.openRight).toBeLessThanOrEqual(row.cardRight);
      expect(row.cardBottom - row.actionsBottom).toBeLessThanOrEqual(20);
      const separatedHorizontally = row.openLeft >= row.copyRight || row.copyLeft >= row.openRight;
      const separatedVertically = row.openTop >= row.copyBottom || row.copyTop >= row.openBottom;
      expect(separatedHorizontally || separatedVertically).toBeTruthy();
    }
  });

  test('keeps command center guide usable on mobile', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto('/admin/?panel=setup&lang=zh-CN');

    await expect(page.locator('.setup-panel')).toBeVisible();
    await expect(page.locator('.command-agent-prompt')).toContainText('Agent 交接提示词');
    await expect(page.getByRole('tab', { name: /提示词/ })).toHaveAttribute('aria-selected', 'true');

    const promptLayout = await page.locator('.command-center-layout').evaluate((layout) => {
      const tabs = Array.from(layout.querySelectorAll('.command-guide-tab')) as HTMLElement[];
      const aside = layout.querySelector('.command-center-aside') as HTMLElement | null;
      const center = layout.querySelector('.command-center') as HTMLElement | null;
      return {
        documentWidth: document.documentElement.scrollWidth,
        viewportWidth: window.innerWidth,
        columns: window.getComputedStyle(layout as HTMLElement).gridTemplateColumns.split(' ').length,
        tabsFit: tabs.every((tab) => tab.scrollWidth <= tab.clientWidth + 1),
        asideAfterCenter: Boolean(aside && center && aside.getBoundingClientRect().top >= center.getBoundingClientRect().bottom),
      };
    });

    expect(promptLayout.documentWidth).toBeLessThanOrEqual(promptLayout.viewportWidth + 2);
    expect(promptLayout.columns).toBe(1);
    expect(promptLayout.tabsFit).toBe(true);
    expect(promptLayout.asideAfterCenter).toBe(true);

    await page.getByRole('tab', { name: /Human CLI/ }).click();
    await expect(page.getByRole('tab', { name: /Human CLI/ })).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('.setup-panel')).toContainText('dcc-mcp-cli marketplace update <package_name> --dcc maya');

    const cliLayout = await page.locator('.command-center-layout').evaluate((layout) => {
      const rows = Array.from(layout.querySelectorAll('.cli-command-row')) as HTMLElement[];
      const codes = Array.from(layout.querySelectorAll('.cli-command-copy code')) as HTMLElement[];
      return {
        rowCount: rows.length,
        documentWidth: document.documentElement.scrollWidth,
        viewportWidth: window.innerWidth,
        rowsFit: rows.every((row) => row.getBoundingClientRect().right <= window.innerWidth + 2),
        codesFit: codes.every((code) => code.scrollWidth <= code.clientWidth + 1),
      };
    });

    expect(cliLayout.rowCount).toBeGreaterThanOrEqual(6);
    expect(cliLayout.documentWidth).toBeLessThanOrEqual(cliLayout.viewportWidth + 2);
    expect(cliLayout.rowsFit).toBe(true);
    expect(cliLayout.codesFit).toBe(true);
  });

  test('normalizes the browser locale onto the document element', async ({ browser }) => {
    const context = await browser.newContext({ locale: 'ja-JP' });
    const page = await context.newPage();
    await mockAdminApi(page);

    await page.goto('/admin/');

    await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
    await expect(page.locator('html')).toHaveAttribute('data-admin-locale-source', 'navigator');
    await expect(page.locator('.brand-tag')).toContainText('DCC-MCP ゲートウェイ');
    await expect(page.locator('#admin-locale-select .preference-select-visible-value')).toHaveText('JA');
    await expect(page.locator('#admin-locale-select')).toHaveAttribute('aria-label', '言語: 日本語');

    await context.close();
  });

  test('switches language from the visible selector and persists the override', async ({ page }) => {
    await page.goto('/admin/');

    await chooseSidebarSelectOption(page, 'admin-locale-select', '简体中文');

    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
    await expect(page.locator('html')).toHaveAttribute('data-admin-locale-source', 'override');
    await expect(page.getByRole('navigation').getByRole('link', { name: '日志' })).toBeVisible();
    await expect(page.locator('#admin-locale-select .preference-select-visible-value')).toHaveText('ZH');
    const localeTriggerMetrics = await page.locator('#admin-locale-select').evaluate((node) => {
      const trigger = node as HTMLElement;
      return {
        clientWidth: trigger.clientWidth,
        scrollWidth: trigger.scrollWidth,
      };
    });
    expect(localeTriggerMetrics.scrollWidth).toBeLessThanOrEqual(localeTriggerMetrics.clientWidth + 1);

    await page.reload();

    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
    await expect(page.locator('#admin-locale-select .preference-select-visible-value')).toHaveText('ZH');
    await expect(page.locator('#admin-locale-select')).toHaveAttribute('aria-label', '语言: 简体中文');
  });

  test('renders an admin flow under Simplified Chinese without translating machine data', async ({ browser }) => {
    const context = await browser.newContext({ locale: 'zh-CN' });
    const page = await context.newPage();
    await mockAdminApi(page);

    await page.goto('/admin/?panel=governance');

    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
    await expect(page.getByRole('navigation').getByRole('link', { name: '治理' })).toHaveClass(/active/);
    const panel = page.locator('.governance-panel');
    await expect(panel).toContainText('流量治理');
    await expect(panel).toContainText('生效策略');
    await expect(panel).toContainText('最近请求决策');
    await expect(panel).toContainText('结果');
    await expect(panel).toContainText('捕获');

    await expect(panel).toContainText('req-policy');
    await expect(panel).toContainText('maya, customhost');
    await expect(panel).toContainText('body.data.params.arguments.api_key');

    await page.getByLabel('筛选当前面板').fill('quota');
    await expect(page.locator('.list-search-meta')).toContainText('1 / 2');
    await expect(panel).toContainText('req-quota');
    await expect(panel).not.toContainText('req-policy');

    await context.close();
  });

  test('shows platform-specific IDE config paths', async ({ page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(navigator, 'userAgentData', {
        configurable: true,
        get: () => ({ platform: 'macOS' }),
      });
      Object.defineProperty(navigator, 'platform', {
        configurable: true,
        get: () => 'MacIntel',
      });
    });

    await page.goto('/admin/');

    const setup = page.locator('.setup-panel');
    await expect(setup).toContainText('~/Library/Application Support/Claude/claude_desktop_config.json');
    await expect(setup).toContainText('~/.cursor/mcp.json');
    await expect(setup).toContainText('~/Library/Application Support/Code/User/mcp.json');
    await expect(setup).toContainText('~/.codex/config.toml');
    await expect(setup).not.toContainText('%APPDATA%\\Claude');
  });

  test('uses the default local gateway port when the dev server has no gateway sentinel', async ({ page }) => {
    await page.route('**/admin/api/health', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          status: 'ok',
          instances_ready: 0,
          instances_total: 0,
          uptime_secs: 1,
          version: '0.17.7',
          rss_bytes: 0,
        }),
      });
    });

    await page.goto('/admin/');

    const setup = page.locator('.setup-panel');
    await expect(setup).toContainText('http://127.0.0.1:9765/mcp');
    await expect(setup).not.toContainText('http://127.0.0.1:3721/mcp');
  });

  test('switches to instances, renders DCC rows, and filters rows', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Instances' }).click();
    await expect(page.locator('.instances-panel')).toBeVisible();
    await expect(page.locator('.dcc-icon')).toHaveCount(2);
    await expect(page.locator('.instances-list')).toHaveCount(2);
    await expect(page.locator('.instances-list').first()).toBeVisible();
    await expect(page.locator('.instance-row')).toHaveCount(2);
    await expect(page.locator('.instance-card')).toHaveCount(0);
    await expect(page.locator('.instances-panel')).toContainText('app-type: maya');
    await expect(page.locator('.instances-panel')).toContainText('app-type: blender');
    await expect(page.locator('.instances-panel')).toContainText('type: gui');
    await expect(page.locator('.instances-panel')).toContainText('type: standalone');
    await expect(page.locator('.instances-panel')).toContainText('DCC version');
    await expect(page.locator('.instances-panel')).toContainText('Server version');
    await expect(page.locator('.instances-panel')).toContainText('Access URL');
    await expect(page.locator('.instances-panel')).toContainText('http://127.0.0.1:8765');
    await expect(page.locator('.instances-panel')).toContainText('Dispatch');
    await expect(page.locator('.instances-panel')).toContainText('ready callable');
    await expect(page.locator('.instances-panel')).toContainText('unavailable not callable');
    await expect(page.locator('.instances-panel')).toContainText('Host RPC');
    await expect(page.locator('.instances-panel')).toContainText('commandport');
    await expect(page.locator('.instances-panel')).toContainText('host-rpc connect failed');
    const rowLayout = await page.locator('.instance-row').first().evaluate((row) => {
      const details = row.querySelector('.instance-row-details');
      const links = row.querySelector('.instance-link-groups');
      const value = row.querySelector('.instance-detail-item strong');
      const valueStyle = value ? window.getComputedStyle(value as HTMLElement) : null;
      return {
        rowHeight: Math.round(row.getBoundingClientRect().height),
        detailColumns: details ? window.getComputedStyle(details as HTMLElement).gridTemplateColumns.split(' ').length : 0,
        linkColumns: links ? window.getComputedStyle(links as HTMLElement).gridTemplateColumns.split(' ').length : 0,
        valueOverflow: valueStyle?.overflow ?? '',
        valueTextOverflow: valueStyle?.textOverflow ?? '',
        valueWhiteSpace: valueStyle?.whiteSpace ?? '',
      };
    });
    expect(rowLayout.rowHeight).toBeLessThanOrEqual(260);
    expect(rowLayout.detailColumns).toBeGreaterThanOrEqual(3);
    expect(rowLayout.linkColumns).toBeGreaterThanOrEqual(2);
    expect(rowLayout.valueOverflow).toBe('hidden');
    expect(rowLayout.valueTextOverflow).toBe('ellipsis');
    expect(rowLayout.valueWhiteSpace).toBe('nowrap');
    const mayaRow = page.locator('.instance-row').filter({ hasText: 'Maya Layout' });
    await expect(mayaRow).toContainText('Update');
    await expect(mayaRow).toContainText('Current version: 0.19.56');
    await expect(mayaRow.locator('.instance-update-button')).toHaveAttribute('data-slot', 'button');
    await expect(mayaRow.locator('.instance-update-button')).toContainText('Check update');
    await expect(mayaRow.locator('.instance-update-help')).toContainText('dcc-mcp-server update apply');
    const updateChrome = await mayaRow.locator('.instance-update-cell').evaluate((cell) => {
      const cellStyle = window.getComputedStyle(cell as HTMLElement);
      const meta = cell.querySelector('.instance-update-meta') as HTMLElement | null;
      const metaStyle = meta ? window.getComputedStyle(meta) : null;
      const linkBlock = cell.parentElement?.querySelector('.instance-link-groups > span') as HTMLElement | null;
      const linkBlockStyle = linkBlock ? window.getComputedStyle(linkBlock) : null;
      return {
        backgroundColor: cellStyle.backgroundColor,
        borderTopWidth: cellStyle.borderTopWidth,
        metaRadius: metaStyle?.borderRadius ?? '',
        linkBlockBorderWidth: linkBlockStyle?.borderTopWidth ?? '',
        linkBlockRadius: linkBlockStyle?.borderRadius ?? '',
      };
    });
    expect(updateChrome.backgroundColor).toBe('rgba(0, 0, 0, 0)');
    expect(updateChrome.borderTopWidth).toBe('0px');
    expect(parseFloat(updateChrome.metaRadius)).toBeGreaterThanOrEqual(20);
    expect(updateChrome.linkBlockBorderWidth).toBe('1px');
    expect(parseFloat(updateChrome.linkBlockRadius)).toBeGreaterThanOrEqual(4);
    await expect(mayaRow).not.toContainText('dcc-mcp-cli update check');
    const mayaUpdateRequest = page.waitForRequest((request) =>
      request.method() === 'POST'
      && request.url().includes('/instances/maya-1234567890/update')
    );
    await mayaRow.getByRole('button', { name: 'Check server update for Maya Layout' }).click();
    await expect((await mayaUpdateRequest).postDataJSON()).toEqual({
      apply: false,
      binary: 'dcc-mcp-server',
    });
    await expect(mayaRow.locator('.instance-update-result.warn')).toContainText('Available 0.18.0');
    await expect(mayaRow.locator('.instance-update-result.warn')).not.toContainText('Restart this DCC backend');
    const blenderRow = page.locator('.instance-row').filter({ hasText: 'Blender Lookdev' });
    await expect(blenderRow).toContainText('Current version: version unknown');
    const blenderUpdateRequest = page.waitForRequest((request) =>
      request.method() === 'POST'
      && request.url().includes('/instances/blender-abcdef1234/update')
    );
    await blenderRow.getByRole('button', { name: 'Check server update for Blender Lookdev' }).click();
    await expect((await blenderUpdateRequest).postDataJSON()).toEqual({
      apply: false,
      binary: 'dcc-mcp-server',
    });
    await expect(blenderRow.locator('.instance-update-result.warn')).toContainText('dcc-mcp-server is not listed in the update manifest.');
    await expect(blenderRow).not.toContainText('Update API is unavailable');
    await expect(mayaRow.getByRole('link', { name: 'docs' }).first()).toHaveAttribute('href', 'http://127.0.0.1:8765/docs');
    await page.getByLabel('Filter current panel').fill('blender');
    await expect(page.locator('.instance-row')).toHaveCount(1);
    await expect(page.locator('.instance-row')).toContainText('Blender Lookdev');
  });

  test('copies the selected instance context route for an agent', async ({ page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(navigator, 'clipboard', {
        value: {
          writeText: async (value: string) => {
            (window as typeof window & { __copiedInstanceRoute?: string }).__copiedInstanceRoute = value;
          },
        },
      });
    });
    await page.goto('/?panel=instances');
    await page.getByRole('button', { name: 'Copy the instance context route for Maya Layout' }).click();
    await expect
      .poll(() => page.evaluate(() => (window as typeof window & { __copiedInstanceRoute?: string }).__copiedInstanceRoute))
      .toBe('gateway://instances/maya-1234567890');
  });

  test('localizes instance field labels in Chinese', async ({ page }) => {
    await page.goto('/?panel=instances&lang=zh-CN');
    const panel = page.locator('.instances-panel');
    await expect(panel).toBeVisible();
    await expect(panel).toContainText('访问地址');
    await expect(panel).toContainText('状态');
    await expect(panel).toContainText('实例类型');
    await expect(panel).toContainText('服务端版本');
    await expect(panel).toContainText('调度');
    await expect(panel).toContainText('可调用');
    await expect(panel).toContainText('不可调用');
    await expect(panel).toContainText('摘要：在线 1，过期 0，异常 1');
    await expect(panel).not.toContainText('Summary: live');
  });

  test('keeps DCC, adapter, and server instance metadata distinct', async ({ page }) => {
    await page.goto('/?panel=instances');
    const mayaRow = page.locator('.instance-row').filter({ hasText: 'Maya Layout' });
    const blenderRow = page.locator('.instance-row').filter({ hasText: 'Blender Lookdev' });

    await expect(mayaRow).toContainText('type: gui');
    await expect(mayaRow).toContainText('DCC version');
    await expect(mayaRow).toContainText('2026');
    await expect(mayaRow).toContainText('Server version');
    await expect(mayaRow).toContainText('0.19.56');
    await expect(mayaRow).toContainText('Adapter');
    await expect(mayaRow).toContainText('0.5.0');
    await expect(mayaRow).toContainText('Current version: 0.19.56');
    await expect(blenderRow).toContainText('type: standalone');
    await expect(blenderRow).toContainText('Current version: version unknown');

    await page.getByLabel('Filter current panel').fill('standalone');
    await expect(page.locator('.instance-row')).toHaveCount(1);
    await expect(page.locator('.instance-row')).toContainText('Blender Lookdev');
  });

  test('keeps instance rows aligned at tablet width', async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 768 });
    await page.goto('/?panel=instances', { waitUntil: 'domcontentloaded' });
    const panel = page.locator('.instances-panel');
    const firstRow = panel.locator('.instance-row').first();
    await expect(firstRow).toBeVisible();

    const tabletMetrics = await firstRow.evaluate((row) => {
      const rowElement = row as HTMLElement;
      const actions = rowElement.querySelector('.instance-row-actions') as HTMLElement | null;
      const details = rowElement.querySelector('.instance-row-details') as HTMLElement | null;
      const actionStyle = actions ? window.getComputedStyle(actions) : null;
      return {
        rowColumns: window.getComputedStyle(rowElement).gridTemplateColumns.split(' ').length,
        rowOverflow: rowElement.scrollWidth - rowElement.clientWidth,
        detailColumns: details ? window.getComputedStyle(details).gridTemplateColumns.split(' ').length : 0,
        actionBorderLeft: actionStyle?.borderLeftWidth ?? '',
        actionBorderTop: actionStyle?.borderTopWidth ?? '',
      };
    });

    expect(tabletMetrics.rowColumns).toBe(1);
    expect(tabletMetrics.rowOverflow).toBeLessThanOrEqual(1);
    expect(tabletMetrics.detailColumns).toBeGreaterThanOrEqual(3);
    expect(tabletMetrics.actionBorderLeft).toBe('0px');
    expect(tabletMetrics.actionBorderTop).toBe('1px');
  });

  test('opens trace detail from the calls panel and keeps the URL shareable', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Calls' }).click();
    const callsPanel = page.locator('.traces-panel');
    await expect(callsPanel).toContainText('toon');
    await expect(callsPanel).toContainText('40');
    await expect(callsPanel).toContainText('60 (60.0%)');
    await expect(callsPanel).toContainText('json');
    await expect(callsPanel).toContainText('0 (0.0%)');
    await expect(callsPanel).toContainText('Layout Artist');
    await expect(callsPanel).toContainText('cursor / windows / workstation-7');
    await expect(callsPanel).toContainText('192.0.2.44');
    await expect(callsPanel).toContainText('self_reported');
    await expect(callsPanel).toContainText('server_derived');
    await page.getByLabel('Filter current panel').fill('192.0.2.44');
    await expect(page.locator('.list-search-meta')).toContainText('1 / 6');
    await expect(callsPanel).toContainText('req-123');
    await expect(callsPanel).not.toContainText('req-json');
    await page.getByLabel('Filter current panel').fill('');
    await expect(callsPanel).toContainText('req-legacy');
    await expect(callsPanel.locator('tr', { hasText: 'req-legacy' })).toContainText('-');
    await page.getByRole('button', { name: 'req-123' }).click();
    await expect(page).toHaveURL(/panel=traces/);
    await expect(page).toHaveURL(/trace=req-123/);
    await expect(page.locator('.trace-detail-panel')).toContainText('req-123');
    await expect(page.locator('.trace-detail-panel')).toContainText('dispatch');
    await expect(page.locator('.trace-detail-panel')).toContainText('Agent timeline');
    await expect(page.locator('.trace-related-head')).toContainText('Related evidence');
    await expect(page.locator('.trace-links a', { hasText: 'Calls' })).toHaveAttribute('href', /tracesTab=calls/);
    await expect(page.locator('.agent-swimlane')).toHaveCount(4);
    await expect(page.locator('.agent-timeline-card')).toContainText('Agent Invocation');
    await expect(page.locator('.agent-timeline-card')).toContainText('LLM Operations');
    await expect(page.locator('.agent-timeline-card')).toContainText('Tool Calls');
    await expect(page.locator('.agent-timeline-card')).toContainText('Failures');
    await expect(page.locator('.agent-timeline-card')).toContainText('Create a sphere and report scene status.');
    await expect(page.locator('.agent-timeline-card')).toContainText('Plan 2');
    await expect(page.locator('.agent-timeline-card')).toContainText('Call create_sphere');
    await expect(page.locator('.agent-timeline-card')).toContainText('Observation 1');
    await expect(page.locator('.agent-timeline-card')).toContainText('Sphere created.');
    await expect(page.locator('.trace-detail-panel')).toContainText('Token accounting');
    await expect(page.locator('.trace-detail-panel')).toContainText('dcc-mcp-byte4-v1');
    await expect(page.locator('.trace-detail-panel')).toContainText(/Input tokens\s*3/);
    await expect(page.locator('.trace-detail-panel')).toContainText(/Total tokens\s*6/);
    await expect(page.locator('.trace-detail-panel')).toContainText('Returned40');
    await expect(page.locator('.trace-detail-panel')).toContainText('Savings60.0%');
    await expect(page.locator('.trace-detail-panel')).toContainText('Layout Artist');
    await expect(page.locator('.trace-detail-panel')).toContainText('cursor / windows / workstation-7');
    await expect(page.locator('.trace-detail-panel')).toContainText('192.0.2.44');
    await expect(page.locator('.trace-detail-panel')).toContainText('trusted_proxy');
    await expect(page.locator('.caller-context-pre')).toContainText('"auth_subject": "user:artist-1"');
    await expect(page.locator('.caller-context-pre')).toContainText('"auth_subject": "auth"');
  });

  test('highlights slow calls, traces, and spans independently from failures', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Calls' }).click();
    const callsPanel = page.locator('.traces-panel');

    await expect(callsPanel.locator('tr.latency-critical', { hasText: 'req-slow' })).toContainText('TAIL');
    await expect(callsPanel.locator('tr.latency-slow', { hasText: 'req-failed-s' })).toContainText('SLOW');
    await expect(callsPanel.locator('tr', { hasText: 'req-failed-s' })).toContainText('failed');
    await expect(callsPanel.locator('tr', { hasText: 'req-failed-f' }).locator('.badge-latency')).toHaveCount(0);

    await page.getByRole('button', { name: 'Slow only' }).click();
    await expect(page.locator('.list-search-meta')).toContainText('2 / 6');
    await expect(callsPanel).toContainText('req-slow');
    await expect(callsPanel).toContainText('req-failed-s');
    await expect(callsPanel).not.toContainText('req-failed-f');

    await page.getByRole('navigation').getByRole('link', { name: 'Traces' }).click();
    const tracesPanel = page.locator('.traces-panel');
    await expect(page.locator('.list-search-meta')).toContainText('2 / 5');
    await expect(tracesPanel.locator('.trace-item.latency-critical', { hasText: 'req-slow' })).toContainText('TAIL');
    await expect(tracesPanel.locator('.trace-item.err.latency-slow', { hasText: 'req-failed-s' })).toContainText('SLOW');
    await expect(tracesPanel).toContainText('p99 latency');
    await expect(tracesPanel).toContainText('tail >= 5.00 s');
    await expect(tracesPanel).toContainText('slowest upload_texture 5.40 s');

    await tracesPanel.locator('.trace-item', { hasText: 'req-slow' }).click();
    await expect(page.locator('.trace-detail-panel')).toContainText('req-slow');
    await expect(page.locator('.trace-detail-panel')).toContainText('TAIL');
    await expect(page.locator('.span-row.latency-critical')).toContainText('upload_texture');
  });

  test('loads traces data from the root admin URL through the admin API base', async ({ page }) => {
    const apiTracePaths: string[] = [];
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.pathname.includes('/api/traces')) {
        apiTracePaths.push(url.pathname);
      }
    });

    await page.goto('/?panel=traces&tracesTab=traces');
    const tracesPanel = page.locator('.traces-panel');
    await expect(tracesPanel).toBeVisible();
    await expect(tracesPanel).toContainText('req-123');
    await expect(tracesPanel).toContainText('p99 latency');
    await expect(tracesPanel).not.toContainText('Admin API returned HTML');
    await expect(tracesPanel).not.toContainText('<!doctype');
    expect(apiTracePaths).toContain('/admin/api/traces');
    expect(apiTracePaths.every((path) => path.startsWith('/admin/api/'))).toBe(true);
  });

  test('shows reconstructed tasks and links them to traces', async ({ page }) => {
    await page.goto('/admin/?panel=tasks');
    await expect(page.locator('.tasks-panel')).toContainText('Create a sphere with the least risky MCP path.');
    await expect(page.locator('.tasks-panel')).toContainText('Produced viewport preview and validated the scene.');
    await expect(page.locator('.tasks-panel')).toContainText('6 call(s)');
    await expect(page.locator('.tasks-panel')).toContainText('render: viewport-preview.png');
    await expect(page.locator('.tasks-panel')).toContainText('validate sphere scene output');
    await expect(page.locator('.tasks-panel')).toContainText('workflow session-1');
    await expect(page.locator('.tasks-panel')).toContainText('client Layout Artist');
    await expect(page.locator('.tasks-panel')).toContainText('Backend failed while opening [path-redacted].');
    await expect(page.locator('.tasks-panel .metric-grid')).toContainText('Avg duration');
    await expect(page.locator('.tasks-panel .metric-grid')).toContainText('Success rate');
    await expect(page.locator('.task-card.ok', { hasText: 'Create a sphere' }).locator('.task-title-row .badge-ok')).toContainText('completed');
    await expect(page.locator('.task-card.err', { hasText: 'Render preview' }).locator('.task-title-row .badge-err')).toContainText('failed');
    await page.getByRole('button', { name: /trace req-123/ }).click();
    await expect(page).toHaveURL(/panel=traces/);
    await expect(page).toHaveURL(/trace=req-123/);
    await expect(page.locator('.trace-detail-panel')).toContainText('req-123');
  });

  test('shows agent workflows with discovery quality and trace links', async ({ page }) => {
    await page.goto('/admin/?panel=workflows');
    const panel = page.locator('.workflows-panel');
    await expect(panel).toContainText('Scene Builder');
    await expect(panel).toContainText('turn turn-1');
    await expect(panel).toContainText('Create a sphere with the least risky MCP path.');
    await expect(panel).toContainText('reply 220 chars');
    await expect(panel).toContainText('Discovery');
    await expect(panel).toContainText('Skill Load');
    await expect(panel).toContainText('Tool Calls');
    await expect(panel).toContainText('best rank 2');
    await expect(panel).toContainText('zero-result');
    await expect(panel).toContainText('Searches');
    await expect(panel).toContainText('avg steps');
    await expect(panel.locator('.metric-grid')).toContainText('Success rate');
    const sceneWorkflow = page.locator('.workflow-card', { hasText: 'Scene Builder' });
    await sceneWorkflow.getByRole('button', { name: 'Inspect' }).click();
    await expect(panel).toContainText('Stage graph');
    await expect(panel).toContainText('Fallbacks');
    await expect(panel).toContainText('escape hatch');
    await expect(panel).toContainText('execute python fallback for material check');
    await expect(panel).toContainText('Artifacts');
    await expect(panel).toContainText('Validation');
    await panel.getByRole('button', { name: /validate sphere scene output/ }).click();
    await expect(panel.locator('.workflow-node-detail')).toContainText('validate sphere scene output');
    await expect(panel.locator('.workflow-node-detail')).toContainText('req-validate');
    await page.getByLabel('Filter current panel').fill('missing tool');
    await expect(page.locator('.workflow-card')).toHaveCount(1);
    await expect(page.locator('.workflow-detail-graph')).toHaveCount(0);
    await expect(panel).toContainText('1 zero-result');
    await page.getByLabel('Filter current panel').fill('');
    await sceneWorkflow.getByRole('button', { name: 'Trace' }).click();
    await expect(page).toHaveURL(/panel=traces/);
    await expect(page).toHaveURL(/trace=req-123/);
  });

  test('shows traffic capture state and metadata-only frames', async ({ page }) => {
    await page.goto('/admin/?panel=traffic');
    const panel = page.locator('.overview-panel');
    await expect(panel).toContainText('Capture state');
    await expect(panel).toContainText('Captured');
    await expect(panel).toContainText('1 captured');
    await expect(panel).toContainText('1 redaction');
    await expect(panel).toContainText('req-traffic');
    await expect(panel).toContainText('tools/call');
    await panel.getByRole('button', { name: 'View' }).click();
    const detail = panel.locator('.payload-pre');
    await expect(detail).toContainText('payload_omitted');
    await expect(detail).toContainText('admin-traffic-metadata-only');
    await expect(detail).not.toContainText('jsonrpc');
    await expect(detail).not.toContainText('secret');
  });

  test('updates stats when the range selector changes', async ({ page }) => {
    await page.goto('/admin/?panel=overview&overviewTab=stats&range=1h');
    const overviewPanel = page.locator('.overview-panel');
    await expect(overviewPanel).toBeVisible();
    await expect(page.locator('#overview-stats-range-select')).toContainText('1h');
    await expect(overviewPanel).toContainText('Response tokens returned');
    await expect(overviewPanel).toContainText('160');
    await expect(overviewPanel).toContainText('Payload tokens');
    await expect(overviewPanel).toContainText('250');
    await expect(overviewPanel).toContainText('Input / Output tokens');
    const hero = overviewPanel.locator('.stats-hero');
    await expect(hero).toBeVisible();
    await expect(hero.locator('.hero-label')).toContainText([
      'Total tokens',
      'Input tokens',
      'Tokens saved',
      'Total calls',
    ]);
    await expect(hero).toContainText('success rate');
    await expect(overviewPanel).toContainText('p99 latency');
    await expect(overviewPanel).toContainText('tail >= 5.00 s');
    await expect(overviewPanel).toContainText('Slow calls');
    await expect(overviewPanel).toContainText('slowest req-slow 6.20 s; slowest upload_texture 5.40 s');
    await expect(overviewPanel.locator('.overview-issues')).toContainText('Top issues now');
    await expect(overviewPanel.locator('.overview-issue.err')).toContainText('2 failed call(s)');
    await expect(overviewPanel.locator('.overview-issue.warn', { hasText: 'Slowest trace' })).toContainText('Slowest trace');
    await expect(overviewPanel).toContainText('Response tokens saved');
    await expect(overviewPanel).toContainText('140');
    await expect(overviewPanel).toContainText('Top app types');
    await expect(overviewPanel).toContainText('maya');
    await expect(overviewPanel).toContainText('Top actors');
    await expect(overviewPanel).toContainText('Layout Artist');
    await expect(overviewPanel).toContainText('Top client platforms');
    await expect(overviewPanel).toContainText('cursor');
    await expect(overviewPanel).toContainText('Top source IPs');
    await expect(overviewPanel).toContainText('192.0.2.44');
    await expect(overviewPanel).toContainText('Token savings by transport');
    await expect(overviewPanel).toContainText('rest');
    await expect(overviewPanel).toContainText('json');
    await chooseSidebarSelectOption(page, 'overview-stats-range-select', '7d');
    await expect(page).toHaveURL(/range=7d/);
    await expect(overviewPanel).toContainText('7d window');
    await page.getByLabel('Filter current panel').fill('rest');
    await expect(overviewPanel).toContainText('rest');
  });

  test('opens calls from overview issue recommendations', async ({ page }) => {
    await page.goto('/admin/?panel=overview&overviewTab=stats&range=1h');
    const issue = page.locator('.overview-issue.err');
    await expect(issue).toContainText('req-failed-f');
    await issue.getByRole('button', { name: 'Open calls' }).click();
    await expect(page).toHaveURL(/panel=traces/);
    await expect(page).toHaveURL(/tracesTab=calls/);
    await expect(page).toHaveURL(/trace=req-failed-fast/);
    await expect(page.locator('.traces-panel')).toContainText('req-failed-f');
  });

  test('loads overview stats when only the range query is present', async ({ page }) => {
    await page.goto('/admin/?panel=overview&range=24h');
    const overviewPanel = page.locator('.overview-panel');
    await expect(overviewPanel).toBeVisible();
    await expect(overviewPanel).toContainText('Total tokens');
  });

  test('loads analytics from the root admin URL through the admin API base', async ({ page }) => {
    const apiAnalyticsPaths: string[] = [];
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.pathname.includes('/api/analytics')) {
        apiAnalyticsPaths.push(`${url.pathname}${url.search}`);
      }
    });

    await page.goto('/?panel=analytics');
    const panel = page.locator('.analytics-panel');
    await expect(panel).toBeVisible();
    await expect(panel).toContainText('Analytics');
    await expect(panel).toContainText('Cumulative Tokens');
    await expect(panel).toContainText('Peak Tokens');
    await expect(panel).toContainText('Longest Task');
    await expect(panel).toContainText(`${((analyticsTotals.tokensInput + analyticsTotals.tokensOutput) / 1000).toFixed(1)}K`);
    await expect(panel).toContainText('maya-1234__create_sphere');
    await expect(panel.locator('.analytics-profile')).toContainText('Agent Activity');
    await expect(panel.locator('.analytics-profile')).toContainText('2 agents');
    await expect(panel.locator('.analytics-profile')).toContainText('top tool maya-1234__create_sphere');
    await expect(panel.locator('.analytics-mini-bar')).toHaveCount(analyticsSeriesFixture.length);
    await expect(panel.locator('.analytics-mini-bar.is-failed')).toHaveCount(2);
    await expect(panel).toContainText('Token Activity');
    await expect(panel.locator('.analytics-token-mode')).toHaveText(['Daily', 'Weekly', 'Cumulative']);
    await expect(panel.locator('.analytics-token-legend')).toContainText('Less');
    await expect(panel.locator('.analytics-token-legend')).toContainText('More');
    await expect(panel.locator('.analytics-token-legend i')).toHaveCount(6);
    await expect(panel.locator('.analytics-token-head .analytics-token-legend')).toHaveCount(0);
    await expect(panel.locator('.analytics-token-footer .analytics-token-legend')).toHaveCount(1);
    await expect(panel.locator('.analytics-token-day:not([data-level="0"])')).toHaveCount(analyticsSeriesFixture.length);
    await expect(panel.locator('.analytics-token-months')).toBeVisible();
    await expect(panel.locator('.analytics-token-months span').first()).toHaveText('Jun');
    await expect(panel).toContainText('Activity Insights');
    await expect(panel).toContainText('Active days');
    await expect(panel).toContainText('Longest streak');
    await expect(panel.locator('.analytics-top-tool-row')).toHaveCount(2);
    await expect(panel.locator('.analytics-top-tool-row').first()).toContainText('maya-1234__create_sphere');
    const calendarMetrics = await panel.locator('.analytics-token-calendar-grid').evaluate((element) => {
      const week = element.querySelector('.analytics-token-week');
      return {
        weeks: element.querySelectorAll('.analytics-token-week').length,
        daysInFirstWeek: week?.querySelectorAll('.analytics-token-day').length ?? 0,
        display: getComputedStyle(element).display,
        columnCount: getComputedStyle(element).gridTemplateColumns.split(' ').length,
      };
    });
    expect(calendarMetrics.display).toBe('grid');
    expect(calendarMetrics.weeks).toBeGreaterThanOrEqual(52);
    expect(calendarMetrics.columnCount).toBe(calendarMetrics.weeks);
    expect(calendarMetrics.daysInFirstWeek).toBe(7);
    const tokenActivityMetrics = await panel.locator('.analytics-token-activity').evaluate((element) => {
      const activeCell = element.querySelector('.analytics-token-day[data-level="5"]') as HTMLElement | null;
      const emptyCell = element.querySelector('.analytics-token-day[data-level="0"]') as HTMLElement | null;
      return {
        maxWidth: Number.parseFloat(getComputedStyle(element).maxWidth),
        activeCellBackground: activeCell ? getComputedStyle(activeCell).backgroundColor : '',
        emptyCellBackground: emptyCell ? getComputedStyle(emptyCell).backgroundColor : '',
      };
    });
    expect(tokenActivityMetrics.maxWidth).toBeGreaterThan(900);
    expect(tokenActivityMetrics.activeCellBackground).not.toBe(tokenActivityMetrics.emptyCellBackground);
    await expect(panel.locator('.status-bar')).toHaveCount(0);
    await expect(panel).not.toContainText('Admin API returned HTML');
    await expect(panel).not.toContainText('<!doctype');

    expect(apiAnalyticsPaths).toEqual(expect.arrayContaining([
      '/admin/api/analytics/overview?range=365d',
      '/admin/api/analytics/timeseries?range=365d&granularity=day',
    ]));
    expect(apiAnalyticsPaths.every((path) => path.startsWith('/admin/api/analytics/'))).toBe(true);
    expect(apiAnalyticsPaths.some((path) => path.includes('/heatmap'))).toBe(false);
  });

  test('localizes token activity calendar month labels', async ({ page }) => {
    await page.goto('/?panel=analytics&lang=zh-CN');
    const panel = page.locator('.analytics-panel');
    await expect(panel).toContainText('Token 活动');
    await expect(panel).toContainText('累计 Token 数');
    await expect(panel.locator('.analytics-profile')).toContainText('Agent 活动');
    await expect(panel.locator('.analytics-profile')).toContainText('2 个 Agent');
    await expect(panel.locator('.analytics-token-months span').first()).toHaveText('6月');
    await expect(panel.locator('.analytics-token-mode')).toHaveText(['每日', '每周', '累计']);
    await expect(panel.locator('.analytics-token-legend')).toContainText('低');
    await expect(panel.locator('.analytics-token-legend')).toContainText('高');
    await expect(panel).toContainText('活动洞察');
    await expect(panel).toContainText('最长连续天数');
  });

  test('shows memory records and allows forgetting a row', async ({ page }) => {
    const forgetRequests: string[] = [];
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.pathname === '/admin/api/memory/forget') {
        forgetRequests.push(request.postData() ?? '');
      }
    });

    await page.goto('/?panel=memory');
    const panel = page.locator('.memory-panel');
    await expect(panel).toContainText('Memory');
    await expect(panel).toContainText('75.0%');
    await expect(panel).toContainText('pattern:tool_call:create_cube:ok');
    await expect(panel).toContainText('create_cube');
    await expect(panel.getByRole('button', { name: 'Forget matches' })).toBeDisabled();

    await panel.getByRole('button', { name: /^Forget$/ }).first().click();
    await expect.poll(() => forgetRequests.length).toBeGreaterThan(0);
    expect(forgetRequests[0]).toContain('"id":1');
  });

  test('keeps analytics activity heatmap and insights readable on mobile', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto('/?panel=analytics');
    const panel = page.locator('.analytics-panel');
    await expect(panel.locator('.analytics-top-tool-row code').first()).toContainText('__create_sphere');

    const firstToolCode = await panel.locator('.analytics-top-tool-row code').first().evaluate((element) => ({
      textOverflow: getComputedStyle(element).textOverflow,
      whiteSpace: getComputedStyle(element).whiteSpace,
    }));
    expect(firstToolCode.textOverflow).toBe('ellipsis');
    expect(firstToolCode.whiteSpace).toBe('nowrap');

    const heatmapMetrics = await panel.locator('.analytics-token-scroll').evaluate((element) => ({
      clientWidth: element.clientWidth,
      scrollWidth: element.scrollWidth,
      overflowX: getComputedStyle(element).overflowX,
    }));
    expect(heatmapMetrics.overflowX).toBe('auto');
    expect(heatmapMetrics.scrollWidth).toBeGreaterThan(heatmapMetrics.clientWidth);

    const insightMetrics = await panel.locator('.analytics-insight-grid').evaluate((element) => ({
      clientWidth: element.clientWidth,
      scrollWidth: element.scrollWidth,
      gridTemplateColumns: getComputedStyle(element).gridTemplateColumns,
    }));
    expect(insightMetrics.scrollWidth).toBeLessThanOrEqual(insightMetrics.clientWidth + 1);
    expect(insightMetrics.gridTemplateColumns.split(' ')).toHaveLength(1);
  });

  test('shows governance controls and request decisions', async ({ page }) => {
    await page.goto('/admin/?panel=governance');
    const panel = page.locator('.governance-panel');
    await expect(panel).toContainText('Traffic Governance');
    await expect(panel).toContainText('high_sensitivity_capture');
    await expect(panel).toContainText('Read-only');
    await expect(panel).toContainText('maya, customhost');
    await expect(panel).toContainText('safe-');
    await expect(panel).toContainText('body.data.params.arguments.api_key');
    await expect(panel.locator('.governance-card').filter({ hasText: 'quota' })).toBeVisible();
    await expect(panel.locator('.governance-card').filter({ hasText: 'redaction' })).toBeVisible();
    await expect(panel).toContainText('req-policy');
    await expect(panel).toContainText('denied');
    await expect(panel).toContainText('throttled');
    await page.getByLabel('Filter current panel').fill('quota');
    await expect(page.locator('.list-search-meta')).toContainText('1 / 2');
    await expect(panel).toContainText('req-quota');
    await expect(panel).not.toContainText('req-policy');
  });

  test('adds and removes SQLite-backed skill paths', async ({ page }) => {
    await page.goto('/admin/?panel=skill-paths');
    await expect(page.locator('.skill-paths-panel')).toContainText('Skills & paths');
    await expect(page.locator('.skill-paths-panel')).toContainText('maya-modeling');
    await expect(page.locator('.skill-paths-panel')).toContainText('create_sphere');
    await expect(page.locator('.skill-paths-panel')).toContainText('Loaded skills');
    await expect(page.locator('.skill-inventory-list')).toBeVisible();
    await expect(page.locator('.skill-inventory-row')).toHaveCount(2);
    await expect(page.locator('.skill-card-grid')).toHaveCount(0);
    await expect(page.locator('.skill-paths-panel')).toContainText('Searched / used');
    await expect(page.locator('.skill-paths-panel')).toContainText('best rank 2');
    await expect(page.locator('.skill-paths-panel')).toContainText('1 calls, 0 failures');
    await expect(page.locator('.skill-insight-strip')).toHaveAttribute('aria-label', 'Operator focus');
    await expect(page.locator('.skill-insight-strip')).toContainText('Missing paths');
    await expect(page.locator('.skill-insight-strip')).toContainText('Needs adoption');
    await expect(page.locator('.skill-insight-strip')).toContainText('blender-lookdev');
    await expect(page.locator('.skill-paths-panel')).toContainText('low adoption');
    await expect(page.locator('.skill-paths-panel')).toContainText('admin_custom #7');
    await expect(page.locator('.skill-paths-panel')).not.toContainText('G:/custom/admin-skills');
    await expect(page.getByRole('button', { name: 'Add path' })).toBeDisabled();
    const newPathInput = page.getByLabel('New skill path');
    await newPathInput.fill('G:/new/team-skills');
    await expect(page.getByRole('button', { name: 'Add path' })).toBeEnabled();
    await newPathInput.press('Enter');
    await expect(page.locator('.skill-paths-panel')).toContainText('admin_custom #8');
    await expect(page.locator('.skill-paths-panel')).not.toContainText('G:/new/team-skills');
    await page.getByRole('button', { name: 'Remove' }).first().click();
    await expect(page.locator('.skill-paths-panel')).not.toContainText('admin_custom #7');
  });

  test('keeps the Discover search toolbar from covering skills content', async ({ page }) => {
    await page.setViewportSize({ width: 884, height: 511 });
    await page.goto('/admin/?panel=discover&discoverTab=skills');

    const search = page.locator('.list-search-wrap[data-panel="discover"]');
    const tabs = page.locator('.discover-tabs');
    const header = page.locator('.skill-paths-panel .panel-header');
    await expect(search).toBeVisible();
    await expect(page.getByRole('search').getByLabel('Filter current panel')).toBeVisible();
    await expect(tabs).toBeVisible();
    await expect(header).toBeVisible();

    const layout = await page.evaluate(() => {
      const rectFor = (selector: string) => {
        const element = document.querySelector(selector);
        if (!element) throw new Error(`Missing ${selector}`);
        const rect = element.getBoundingClientRect();
        return {
          top: rect.top,
          bottom: rect.bottom,
          left: rect.left,
          right: rect.right,
          width: rect.width,
        };
      };
      const searchRect = rectFor('.list-search-wrap[data-panel="discover"]');
      const tabsRect = rectFor('.discover-tabs');
      const headerRect = rectFor('.skill-paths-panel .panel-header');
      const firstRowRect = rectFor('.skill-inventory-row');
      const firstRowHitTarget = document.elementFromPoint(
        Math.min(firstRowRect.right - 4, firstRowRect.left + 24),
        Math.min(firstRowRect.bottom - 4, firstRowRect.top + 24),
      );
      return {
        searchBeforeTabs: searchRect.bottom <= tabsRect.top,
        tabsBeforeHeader: tabsRect.bottom <= headerRect.top,
        firstRowHeight: firstRowRect.bottom - firstRowRect.top,
        firstRowCoveredBySearch: Boolean(firstRowHitTarget?.closest('.list-search-wrap')),
        noHorizontalOverflow: document.documentElement.scrollWidth <= document.documentElement.clientWidth + 2,
        searchFits: searchRect.right <= document.documentElement.clientWidth + 2 && searchRect.left >= -2,
      };
    });

    expect(layout.searchBeforeTabs).toBe(true);
    expect(layout.tabsBeforeHeader).toBe(true);
    expect(layout.firstRowHeight).toBeLessThanOrEqual(260);
    expect(layout.firstRowCoveredBySearch).toBe(false);
    expect(layout.noHorizontalOverflow).toBe(true);
    expect(layout.searchFits).toBe(true);
  });

  test('keeps skill inventory rows compact on mobile', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto('/admin/?panel=discover&discoverTab=skills');

    const panel = page.locator('.skill-paths-panel');
    await expect(panel).toBeVisible();
    await expect(panel.locator('.skill-inventory-row')).toHaveCount(2);
    await expect(panel.locator('.skill-inventory-list-header')).toBeHidden();
    await expect(panel.locator('.skill-insight-strip')).toBeVisible();

    const metrics = await panel.evaluate((root) => {
      const rows = Array.from(root.querySelectorAll('.skill-inventory-row'));
      const insights = root.querySelector('.skill-insight-strip');
      return {
        documentWidth: document.documentElement.scrollWidth,
        viewportWidth: window.innerWidth,
        rowHeights: rows.map((row) => Math.round(row.getBoundingClientRect().height)),
        insightWidth: insights?.scrollWidth ?? 0,
        insightClientWidth: insights?.clientWidth ?? 0,
        firstRowText: rows[0]?.textContent?.replace(/\s+/g, ' ').trim() ?? '',
      };
    });
    expect(metrics.documentWidth).toBeLessThanOrEqual(metrics.viewportWidth + 2);
    expect(metrics.insightWidth).toBeLessThanOrEqual(metrics.insightClientWidth + 1);
    expect(Math.max(...metrics.rowHeights)).toBeLessThanOrEqual(250);
    expect(metrics.firstRowText).toContain('maya-modeling');
    expect(metrics.firstRowText).toContain('create_sphere');
    await expect(panel.getByRole('button', { name: /Open maya-modeling detail|查看 maya-modeling 详情/ })).toBeVisible();
  });

  test('mounts only the selected Discover tab content', async ({ page }) => {
    await page.goto('/admin/?panel=discover&discoverTab=skills');
    await expect(page.locator('.skill-paths-panel')).toBeVisible();
    await expect(page.locator('.marketplace-panel')).toHaveCount(0);
    await expect(page.locator('.integrations-panel')).toHaveCount(0);
    await expect(page.locator('.discover-panel')).not.toContainText('Sentry Error Monitoring');

    await page.goto('/admin/?panel=discover&discoverTab=integrations');
    await expect(page.locator('.integrations-panel')).toBeVisible();
    await expect(page.locator('.skill-paths-panel')).toHaveCount(0);
    await expect(page.locator('.marketplace-panel')).toHaveCount(0);
    await expect(page.locator('.discover-panel')).not.toContainText('maya-modeling');
  });

  test('opens rendered markdown details for a skill', async ({ page }) => {
    await page.goto('/admin/?panel=skill-paths');
    await page.locator('.skill-inventory-title', { hasText: 'maya-modeling' }).click();

    const detail = page.locator('.skill-detail-panel');
    await expect(detail.locator('.skill-detail-path')).toContainText('very-long-team-folder');
    await expect(detail.locator('.skill-markdown-preview h3')).toHaveText('Maya Modeling');
    await expect(detail.locator('.skill-markdown-preview li').first()).toContainText('Create a polygon sphere');
    await expect(detail.locator('.inline-code')).toContainText('maya_modeling__long_inline_identifier');
    await expect(detail.locator('.skill-markdown-preview table')).toContainText('safe');
    await expect(detail.locator('.skill-table-wrap')).toBeVisible();
    await expect(detail.locator('.skill-code-language')).toHaveText('python');
    await expect(detail.locator('.skill-code-copy')).toHaveText('Copy');
    await expect(detail.locator('.skill-code-block')).toContainText('cmds.polySphere');
    await expect(detail.locator('.skill-frontmatter')).toContainText('dcc: maya');
    await expect(detail.locator('.skill-tool-row').first()).toContainText('idempotent');
    await expect(detail.locator('.skill-tool-row').first()).toContainText('thread:main');
  });

  test('keeps skill detail content inside the viewport on narrow screens', async ({ page }) => {
    await page.setViewportSize({ width: 430, height: 900 });
    await page.goto('/admin/?panel=skill-paths');
    await page.locator('.skill-inventory-title', { hasText: 'maya-modeling' }).click();

    const noPageOverflow = await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 2);
    expect(noPageOverflow).toBe(true);
    const pathFits = await page.locator('.skill-detail-path').evaluate((node) => node.scrollWidth <= node.clientWidth + 2);
    expect(pathFits).toBe(true);
    const toolNameFits = await page.locator('.skill-tool-row code').first().evaluate((node) => node.scrollWidth <= node.clientWidth + 2);
    expect(toolNameFits).toBe(true);
  });

  test('refreshes the skills inventory on demand', async ({ page }) => {
    await page.goto('/admin/?panel=skill-paths');
    await expect(page.locator('.skill-paths-panel')).toContainText('maya-modeling');
    await expect(page.locator('.skill-paths-panel')).not.toContainText('houdini-fx');

    await page.route('**/admin/api/skills', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          total: 3,
          loaded: 3,
          unloaded: 0,
          action_count: 6,
          skills: [
            {
              name: 'maya-modeling',
              dcc_type: 'maya',
              loaded: true,
              action_count: 3,
              instance_count: 1,
              instances: ['12345678'],
              tools: ['create_sphere', 'delete_sphere', 'set_transform'],
              summary: 'Modeling tools currently loaded by Maya.',
            },
            {
              name: 'blender-lookdev',
              dcc_type: 'blender',
              loaded: true,
              action_count: 2,
              instance_count: 1,
              instances: ['abcdef12'],
              tools: ['render_preview', 'assign_material'],
              summary: 'Lookdev tools currently loaded by Blender.',
            },
            {
              name: 'houdini-fx',
              dcc_type: 'houdini',
              loaded: true,
              action_count: 1,
              instance_count: 1,
              instances: ['fedcba98'],
              tools: ['simulate_smoke'],
              summary: 'FX tools discovered after manual refresh.',
            },
          ],
        }),
      });
    });
    await page.locator('.skill-paths-panel').getByRole('button', { name: 'Refresh' }).click();

    await expect(page.locator('.skill-paths-panel')).toContainText('houdini-fx');
    await expect(page.locator('.skill-paths-panel')).toContainText('simulate_smoke');
    await expect(page.locator('.skill-summary-grid')).toContainText('3 indexed');
  });

  test('shows logs and panel search metadata', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Logs' }).click();
    await expect(page.locator('.logs-panel')).toContainText('Request req-123');
    await expect(page.locator('.logs-panel')).toContainText('Gateway server version: 0.19.56');
    await expect(page.locator('.logs-panel')).toContainText('Step 1: maya-1234__create_sphere');
    await expect(page.locator('.logs-panel')).toContainText('Gateway events');
    await expect(page.locator('.logs-panel')).toContainText('tools/call ok');
    await expect(page.locator('.logs-panel .severity-badge.severity-error').first()).toContainText('Error');
    await expect(page.locator('.logs-panel .severity-badge.severity-debug').first()).toContainText('Debug');
    await page.locator('.logs-panel .log-severity-card.severity-error').click();
    await expect(page.locator('.logs-panel')).toContainText('req-err');
    await expect(page.locator('.logs-panel')).not.toContainText('req-123');
    await page.locator('.logs-panel .log-severity-card.severity-debug').click();
    await expect(page.locator('.logs-panel')).toContainText('dispatch cache hit');
    await expect(page.locator('.logs-panel')).toContainText('dcc_mcp_http_server:executor');
    await expect(page.locator('.logs-panel')).toContainText('ThreadId(01)');
    await expect(page.locator('.logs-panel')).not.toContainText('tools/call ok');
    await page.getByLabel('Filter current panel').fill('missing');
    await expect(page.locator('.list-search-meta')).toHaveText('0 / 4');
    await expect(page.locator('.logs-panel')).toContainText('No log lines match your search.');
  });

  test('collapses the sidebar into a horizontal nav on mobile widths', async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 900 });
    await page.goto('/admin/');
    // Let the SPA finish its initial mount (it normalizes the URL via
    // history.replaceState) before reading computed styles.
    await expect(page.locator('.main-stage')).toBeVisible();

    await expect
      .poll(() => page.locator('.app-shell').evaluate((node) => getComputedStyle(node).flexDirection))
      .toBe('column');

    const navDirection = await page
      .locator('.nav-links')
      .evaluate((node) => getComputedStyle(node).flexDirection);
    expect(navDirection).toBe('row');

    const noPageOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 2,
    );
    expect(noPageOverflow).toBe(true);
  });

  test('switches color scheme and persists the choice', async ({ page }) => {
    await page.goto('/admin/');
    const themeSelect = page.locator('#admin-theme-select');
    await expect(themeSelect).toBeVisible();

    await chooseSidebarSelectOption(page, 'admin-theme-select', 'Light');
    await expect(page.locator('html')).not.toHaveClass(/dark/);
    await chooseSidebarSelectOption(page, 'admin-theme-select', 'Dark');
    await expect(page.locator('html')).toHaveClass(/dark/);
    await expect(page.locator('html')).toHaveAttribute('data-admin-theme', 'dark');
    expect(await page.evaluate(() => localStorage.getItem('dcc-mcp-admin-theme'))).toBe('dark');

    await chooseSidebarSelectOption(page, 'admin-theme-select', 'Light');
    await expect(page.locator('html')).not.toHaveClass(/dark/);
    await expect(page.locator('html')).toHaveAttribute('data-admin-theme', 'light');

    // The persisted choice survives a reload.
    await chooseSidebarSelectOption(page, 'admin-theme-select', 'Dark');
    await page.reload();
    await expect(page.locator('html')).toHaveClass(/dark/);
    await expect(page.locator('#admin-theme-select .preference-select-visible-value')).toHaveText('Dark');
    await expect(page.locator('#admin-theme-select')).toHaveAttribute('aria-label', 'Theme: Dark');
  });

  test.describe('Integrations panel', () => {
    test('shows integration rows with empty state for webhooks/wecom/otlp', async ({ page }) => {
      await page.goto('/admin/?panel=integrations');
      await expect(page.locator('.integrations-panel')).toBeVisible();
      await expect(page.locator('.integrations-panel h2')).toContainText('Integrations');
      await expect(page.locator('.integration-card')).toHaveCount(4);
      await expect(page.locator('.integrations-list')).toHaveCount(1);
      await expect(page.locator('.integrations-grid')).toHaveCount(0);
      await expect(page.locator('.integration-card[data-kind="sentry"]')).toContainText('Sentry Error Monitoring');
      await expect(page.locator('.integration-card[data-kind="sentry"] .badge-ok')).toContainText('Active');
      await expect(page.locator('.integration-card[data-kind="sentry"] .integration-config-value').first()).toContainText('https://********@o0.ingest.sentry.io/0');
      await expect(page.locator('.integration-card[data-kind="sentry"]')).not.toContainText('%E2');
      await expect(page.locator('.integration-card[data-kind="sentry"]')).not.toContainText('examplePublicKey');
      await expect(page.locator('.integration-card[data-kind="webhooks"]')).toContainText('Event Webhooks');
      await expect(page.locator('.integration-card[data-kind="webhooks"] .badge-muted')).toContainText('Inactive');
      await expect(page.locator('.integration-card[data-kind="webhooks"] .integration-config-value').first()).toContainText('~/dcc-mcp/etc/webhooks.yaml');
      await expect(page.locator('.integration-card[data-kind="wecom"]')).toContainText('WeCom Message Push');
      await expect(page.locator('.integration-card[data-kind="wecom"] .badge-muted')).toContainText('Inactive');
      await expect(page.locator('.integration-card[data-kind="wecom"] .integration-config-value').first()).toContainText('Not set');
      await expect(page.locator('.integration-card[data-kind="otlp"]')).toContainText('OTLP Telemetry');
      await expect(page.locator('.integration-card[data-kind="otlp"] .badge-muted')).toContainText('Inactive');
      await expect(page.locator('.integration-card[data-kind="otlp"] .integration-config-value').first()).toContainText('Not set');
    });

    test('keeps integration rows compact and readable on mobile', async ({ page }) => {
      await page.setViewportSize({ width: 390, height: 844 });
      await page.goto('/admin/?panel=integrations&lang=zh-CN');

      const panel = page.locator('.integrations-panel');
      await expect(panel).toBeVisible();
      await expect(panel.locator('.integration-card')).toHaveCount(4);

      const layout = await panel.evaluate((node) => {
        const cards = Array.from(node.querySelectorAll('.integration-card'));
        return {
          bodyScrollWidth: document.body.scrollWidth,
          viewportWidth: window.innerWidth,
          cards: cards.map((card) => {
            const cardRect = card.getBoundingClientRect();
            const configRect = card.querySelector('.integration-config-preview')?.getBoundingClientRect();
            const desc = card.querySelector('.integration-card-desc') as HTMLElement | null;
            const descStyle = desc ? window.getComputedStyle(desc) : null;
            return {
              height: cardRect.height,
              width: cardRect.width,
              configWidth: configRect?.width ?? 0,
              lineClamp: descStyle?.webkitLineClamp ?? '',
              descOverflow: descStyle?.overflow ?? '',
            };
          }),
        };
      });

      expect(layout.bodyScrollWidth).toBeLessThanOrEqual(layout.viewportWidth);
      for (const card of layout.cards) {
        expect(card.height).toBeLessThanOrEqual(245);
        expect(card.configWidth).toBeLessThanOrEqual(card.width);
        expect(card.lineClamp).toBe('2');
        expect(card.descOverflow).toBe('hidden');
      }
    });

    test('uses the local default path when an older gateway returns an empty webhook path', async ({ page }) => {
      await page.route('**/admin/api/integrations', async (route) => {
        if (route.request().method() !== 'GET') {
          await route.fallback();
          return;
        }
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            integrations: [{
              kind: 'webhooks',
              label: 'Event Webhooks',
              description: 'Outbound delivery of EventBus events.',
              status: 'inactive',
              config: {
                config_path: '',
                config_text: 'queue_capacity: 1024\nwebhooks:\n  - name: studio-events\n    url: http://127.0.0.1:9000/dcc-mcp-events\n    events:\n      - tool.failed\n',
              },
              env_locked_fields: [
                { key: 'config_path', locked: false, env_var: 'DCC_MCP_WEBHOOKS_CONFIG' },
              ],
            }],
          }),
        });
      });

      await page.goto('/admin/?panel=integrations');
      const webhooks = page.locator('.integration-card[data-kind="webhooks"]');
      await expect(webhooks).toContainText('~/dcc-mcp/etc/webhooks.yaml');
      await openIntegrationEditor(page, 'webhooks');
      const form = page.locator('.integration-edit-form');
      await expect(form.locator('.integration-config-path-note')).toContainText('~/dcc-mcp/etc/webhooks.yaml');
      await expect(form.locator('textarea#integration-webhooks-config_text')).toBeVisible();
    });

    test('shows write_config_path as the webhook edit destination', async ({ page }) => {
      await page.route('**/admin/api/integrations', async (route) => {
        if (route.request().method() !== 'GET') {
          await route.fallback();
          return;
        }
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            integrations: [{
              kind: 'webhooks',
              label: 'Event Webhooks',
              description: 'Outbound delivery of EventBus events.',
              status: 'active',
              config: {
                config_path: 'C:/runtime/webhooks.yaml',
                write_config_path: 'C:/Users/example/dcc-mcp/etc/webhooks.yaml',
                config_text: 'queue_capacity: 1024\nwebhooks:\n  - name: runtime\n    url: http://127.0.0.1:9000/dcc-mcp-events\n    events:\n      - tool.failed\n',
              },
              env_locked_fields: [
                { key: 'config_path', locked: true, env_var: 'DCC_MCP_WEBHOOKS_CONFIG' },
              ],
            }],
          }),
        });
      });

      await page.goto('/admin/?panel=integrations');
      const webhooks = page.locator('.integration-card[data-kind="webhooks"]');
      await expect(webhooks).toContainText('C:/runtime/webhooks.yaml');
      await openIntegrationEditor(page, 'webhooks');
      const form = page.locator('.integration-edit-form');
      await expect(form.locator('.integration-config-path-note')).toContainText('C:/Users/example/dcc-mcp/etc/webhooks.yaml');
      await expect(form.locator('.integration-config-path-note')).not.toContainText('C:/runtime/webhooks.yaml');
    });

    test('shows env-locked Sentry with masked DSN', async ({ page }) => {
      await page.goto('/admin/?panel=integrations');
      await openIntegrationEditor(page, 'sentry');
      // DSN comes from env, but can still be manually overridden for restart.
      const dsnField = page.locator('.integration-edit-field.env-locked').first();
      await expect(dsnField).toBeVisible();
      await expect(dsnField.locator('input')).toBeEnabled();
      await expect(dsnField.locator('input')).toHaveValue('');
      await expect(dsnField.locator('input')).toHaveAttribute('placeholder', /override after restart/i);
      // Shows env var hint
      await expect(page.locator('.integration-env-hint')).toContainText('DCC_MCP_SENTRY_DSN');
      // Other fields are editable
      await expect(page.locator('#integration-sentry-environment')).toBeEnabled();
      await expect(page.locator('#integration-sentry-release')).toBeEnabled();
      await expect(page.locator('#integration-sentry-sample_rate')).toBeEnabled();
    });

    test('save shows pending_restart badge', async ({ page }) => {
      await page.goto('/admin/?panel=integrations');
      await openIntegrationEditor(page, 'sentry');
      // Change environment
      await page.locator('#integration-sentry-environment').fill('staging');
      // Save
      const updateRequest = page.waitForRequest((request) =>
        request.url().endsWith('/admin/api/integrations') && request.method() === 'PUT',
      );
      await page.locator('.integration-edit-actions button[type="submit"]').click();
      const request = await updateRequest;
      const payload = request.postDataJSON() as { config: Record<string, unknown> };
      expect(payload.config).not.toHaveProperty('dsn');
      // Wait for edit form to close
      await expect(page.locator('.integration-edit-form')).not.toBeVisible({ timeout: 5000 });
      // After save, the panel should re-render with pending_restart
      await expect(page.locator('.integration-card[data-kind="sentry"].pending-restart')).toBeVisible({ timeout: 5000 });
      await expect(page.locator('.integration-card[data-kind="sentry"] .integration-card-head .badge-warn')).toContainText('Pending Restart');
    });

    test('saves WeCom robot event types and template', async ({ page }) => {
      await page.goto('/admin/?panel=integrations');
      await openIntegrationEditor(page, 'wecom');
      const form = page.locator('.integration-edit-form');
      await expect(form.locator('.integration-template-token-strip')).toContainText('Template Variables');
      await expect(form.locator('[data-template-token]')).toHaveText([
        '$event',
        '$event-id',
        '$dcc-type',
        '$instance-id',
        '$tool-slug',
        '$skill-name',
        '$url',
      ]);
      await page.locator('#integration-wecom-webhook_url').fill('https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123');
      await page.locator('#integration-wecom-event_types').fill('tool.failed, gateway.instance.*');
      const template = page.locator('#integration-wecom-template');
      await template.fill('DCC-MCP $event\nDCC: $dcc-type');
      await form.getByRole('button', { name: '$instance-id' }).click();
      await form.getByRole('button', { name: '$url' }).click();
      await expect(template).toHaveValue('DCC-MCP $event\nDCC: $dcc-type $instance-id $url');
      const updateRequest = page.waitForRequest((request) =>
        request.url().endsWith('/admin/api/integrations') && request.method() === 'PUT',
      );
      await page.locator('.integration-edit-actions button[type="submit"]').click();
      const request = await updateRequest;
      const payload = request.postDataJSON() as { kind: string; config: Record<string, unknown> };
      expect(payload.kind).toBe('wecom');
      expect(payload.config.webhook_url).toBe('https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123');
      expect(payload.config.event_types).toEqual(['tool.failed', 'gateway.instance.*']);
      expect(payload.config.template).toBe('DCC-MCP $event\nDCC: $dcc-type $instance-id $url');
      await expect(page.locator('.integration-card[data-kind="wecom"].pending-restart')).toBeVisible({ timeout: 5000 });
      await expect(page.locator('.integration-card[data-kind="wecom"] .integration-config-value').first()).toContainText('key=********');
    });

    test('tests WeCom robot push from the current edit form config', async ({ page }) => {
      await page.goto('/admin/?panel=integrations');
      await openIntegrationEditor(page, 'wecom');
      const form = page.locator('.integration-edit-form');
      await page.locator('#integration-wecom-webhook_url').fill('https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123');
      await page.locator('#integration-wecom-event_types').fill('tool.failed, gateway.instance.*');
      await page.locator('#integration-wecom-template').fill('DCC-MCP $event\nDCC: $dcc-type');

      const testRequest = page.waitForRequest((request) =>
        request.url().endsWith('/admin/api/integrations/test') && request.method() === 'POST',
      );
      await form.getByRole('button', { name: 'Test Send' }).click();
      const request = await testRequest;
      const payload = request.postDataJSON() as { kind: string; config: Record<string, unknown> };
      expect(payload.kind).toBe('wecom');
      expect(payload.config.webhook_url).toBe('https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123');
      expect(payload.config.event_types).toEqual(['tool.failed', 'gateway.instance.*']);
      expect(payload.config.template).toBe('DCC-MCP $event\nDCC: $dcc-type');
      await expect(page.locator('.status-bar')).toContainText('wecom test message sent');
      await expect(page.locator('.integration-edit-form')).toBeVisible();
    });

    test('saves Event Webhooks by editing YAML directly', async ({ page }) => {
      await page.goto('/admin/?panel=integrations');
      await openIntegrationEditor(page, 'webhooks');
      await expect(page.locator('.integration-edit-overlay')).toHaveCount(0);
      await expect(page.locator('.integration-edit-panel')).toBeVisible();
      const form = page.locator('.integration-edit-form');
      await expect(form).toBeVisible();
      await expect(form).toContainText('Webhooks YAML');
      await expect(form).toContainText('Saved to');
      await expect(form.locator('.integration-config-path-note')).toContainText('~/dcc-mcp/etc/webhooks.yaml');
      await expect(form.locator('#integration-webhooks-config_path')).toHaveCount(0);
      await expect(form.locator('.integration-edit-field.is-textarea')).toBeVisible();

      const editor = form.locator('textarea#integration-webhooks-config_text');
      await expect(editor).toBeVisible();
      await expect(editor).toHaveValue(/queue_capacity: 1024/);
      await editor.fill('webhooks:\n  - name: notify\n    url: http://127.0.0.1:9000/hook\n    events:\n      - tool.failed\n');

      const updateRequest = page.waitForRequest((request) =>
        request.url().endsWith('/admin/api/integrations') && request.method() === 'PUT',
      );
      await form.getByRole('button', { name: 'Save' }).click();
      const request = await updateRequest;
      const payload = request.postDataJSON() as { kind: string; config: Record<string, unknown> };
      expect(payload.kind).toBe('webhooks');
      expect(payload.config.config_text).toContain('name: notify');

      await expect(page.locator('.integration-card[data-kind="webhooks"].pending-restart')).toBeVisible({ timeout: 5000 });
      await expect(page.locator('.integration-card[data-kind="webhooks"] .badge-warn')).toContainText('Pending Restart');
      await expect(page.locator('.integration-card[data-kind="webhooks"]')).toContainText('~/dcc-mcp/etc/webhooks.yaml');
    });

    test('shows error for invalid DSN', async ({ page }) => {
      // Override mock: DSN is NOT env-locked (allows editing)
      await page.route('**/admin/api/integrations', async (route) => {
        const method = route.request().method();
        if (method === 'GET') {
          await route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              integrations: [{
                kind: 'sentry',
                label: 'Sentry Error Monitoring',
                description: 'Send panics.',
                status: 'inactive',
                config: { dsn: '', environment: '', release: '', sample_rate: 1.0 },
                env_locked_fields: [],
              }],
            }),
          });
        } else {
          await route.fulfill({ status: 200, contentType: 'application/json', body: '{}' });
        }
      });

      await page.goto('/admin/?panel=integrations');
      await openIntegrationEditor(page, 'sentry');
      // Enter invalid DSN (doesn't start with 'http')
      await page.locator('#integration-sentry-dsn').fill('bad-dsn-string');
      // Submit
      await page.locator('.integration-edit-actions button[type="submit"]').click();
      // Should see field-level error for invalid DSN
      await expect(page.locator('.integration-field-error')).toContainText(/Invalid DSN|error/i, { timeout: 5000 });
    });
  });
});

test.describe('Sessions and Reliability panels', () => {
  test('sessions panel renders KPIs, tree, and status badges', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Sessions' }).click();

    const panel = page.locator('section.sessions-panel');
    await expect(panel).toBeVisible();

    const kpiTiles = panel.locator('.sessions-kpi-row .metric-tile');
    await expect(kpiTiles).toHaveCount(4);
    await expect(kpiTiles.nth(0).locator('.metric-value')).toHaveText('3');
    await expect(kpiTiles.nth(1).locator('.metric-value')).toHaveText('1');
    await expect(kpiTiles.nth(2).locator('.metric-value')).toHaveText('1');
    await expect(kpiTiles.nth(3).locator('.metric-value')).toHaveText('1');

    const rows = panel.locator('table.sessions-table tbody tr.sessions-row');
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(0).locator('.badge')).toHaveClass(/badge-ok/);
    await expect(rows.nth(0).locator('.badge')).toHaveText('Active');
    await expect(rows.nth(1).locator('.badge')).toHaveClass(/badge-muted/);
    await expect(rows.nth(1).locator('.badge')).toHaveText('Ended');
    await expect(rows.nth(2).locator('.badge')).toHaveClass(/badge-err/);
    await expect(rows.nth(2).locator('.badge')).toHaveText('Crashed');

    // Root row has a tree toggle (has children); child row is visible by default.
    const rootRow = rows.nth(0);
    const treeToggle = rootRow.locator('.sessions-tree-btn');
    await expect(treeToggle).toBeVisible();
    await expect(panel.locator('table.sessions-table tbody tr.sessions-row')).toHaveCount(3);
    await treeToggle.click();
    await expect(panel.locator('table.sessions-table tbody tr.sessions-row')).toHaveCount(2);
    await treeToggle.click();
    await expect(panel.locator('table.sessions-table tbody tr.sessions-row')).toHaveCount(3);

    // Detail toggle reveals parent info / version / end reason for the child row.
    const childRow = panel.locator('table.sessions-table tbody tr.sessions-row').nth(1);
    await expect(panel.locator('tr.sessions-detail-row')).toHaveCount(0);
    await childRow.locator('.sessions-detail-btn').click();
    const detailRow = panel.locator('tr.sessions-detail-row');
    await expect(detailRow).toHaveCount(1);
    await expect(detailRow).toContainText('Version');
    await expect(detailRow).toContainText('0.17.7');
    await expect(detailRow).toContainText('End Reason');
    await expect(detailRow).toContainText('completed');
  });

  test('sessions panel filters by search text', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Sessions' }).click();

    const panel = page.locator('section.sessions-panel');
    await expect(panel).toBeVisible();
    await expect(panel.locator('table.sessions-table tbody tr.sessions-row')).toHaveCount(3);

    const [request] = await Promise.all([
      page.waitForRequest((req) => req.url().includes('/admin/api/sessions') && req.url().includes('search=')),
      panel.locator('.sessions-search-input').fill('bob'),
    ]);
    expect(request.url()).toContain('search=bob');

    // The mock server honors the search filter, so only the crashed session (actor bob) remains.
    await expect(panel.locator('table.sessions-table tbody tr.sessions-row')).toHaveCount(1);
    await expect(panel.locator('table.sessions-table tbody tr.sessions-row').first().locator('.badge')).toHaveText('Crashed');
  });

  test('reliability panel renders health, circuits, funnel, and stability sections', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Reliability' }).click();

    const panel = page.locator('section.reliability-panel');
    await expect(panel).toBeVisible();
    await expect(panel).toContainText('gateway-primary · 127.0.0.1:9765 · v0.17.7');

    const circuitCards = panel.locator('.reliability-circuit-card');
    await expect(circuitCards).toHaveCount(2);
    await expect(circuitCards.nth(0)).toHaveClass(/\bok\b/);
    await expect(circuitCards.nth(0)).toContainText('maya-adapter');
    await expect(circuitCards.nth(0).locator('.badge')).toHaveClass(/badge-ok/);
    await expect(circuitCards.nth(1)).toHaveClass(/\berr\b/);
    await expect(circuitCards.nth(1)).toContainText('houdini-adapter');
    await expect(circuitCards.nth(1).locator('.badge')).toHaveClass(/badge-err/);

    const funnelValues = panel.locator('.reliability-funnel-value');
    await expect(funnelValues).toHaveCount(4);
    await expect(funnelValues.nth(0)).toHaveText('12');
    await expect(funnelValues.nth(1)).toHaveText('40');
    await expect(funnelValues.nth(2)).toHaveText('180');
    await expect(funnelValues.nth(3)).toHaveText('60');

    const stabilityTiles = panel.locator('.reliability-section').last().locator('.metric-tile');
    await expect(stabilityTiles.nth(0).locator('.metric-value')).toHaveText('0');
    await expect(stabilityTiles.nth(3).locator('.metric-value')).toHaveText('99.8%');
  });

  test('switching between panels produces no console or page errors', async ({ page }) => {
    const consoleErrors: string[] = [];
    const pageErrors: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') consoleErrors.push(msg.text());
    });
    page.on('pageerror', (err) => pageErrors.push(err.message));

    await page.goto('/admin/');
    await expect(page.locator('.setup-panel')).toBeVisible();

    await page.getByRole('navigation').getByRole('link', { name: 'Sessions' }).click();
    await expect(page.locator('section.sessions-panel')).toBeVisible();

    await page.getByRole('navigation').getByRole('link', { name: 'Reliability' }).click();
    await expect(page.locator('section.reliability-panel')).toBeVisible();

    await page.getByRole('navigation').getByRole('link', { name: 'Command Center' }).click();
    await expect(page.locator('.setup-panel')).toBeVisible();

    await page.getByRole('navigation').getByRole('link', { name: 'Sessions' }).click();
    await expect(page.locator('section.sessions-panel')).toBeVisible();

    expect(consoleErrors).toEqual([]);
    expect(pageErrors).toEqual([]);
  });
});
