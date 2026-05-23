import { test, expect, type Page } from '@playwright/test';

const now = '2026-05-18T08:00:00.000Z';

async function mockAdminApi(page: Page) {
  const state = {
    skillPaths: [
      { source: 'env:DCC_MCP_SKILL_PATHS', path: 'G:/studio/skills' },
      { id: 7, source: 'admin_custom', path: 'G:/custom/admin-skills' },
    ],
  };

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
        limits: {
          body_max_bytes: 1048576,
          rate_limit_per_minute_per_ip: 60,
          xff_trusted_depth: 1,
          read_retry_max: 2,
          circuit_failure_threshold: 3,
          circuit_open_secs: 30,
        },
        circuits: { tracked_backends: 2, circuits_open: 0 },
      };
    } else if (path === '/activity') {
      body = {
        total: 2,
        events: [
          {
            event_id: 'audit:req-123',
            timestamp: now,
            kind: 'tool_call',
            severity: 'info',
            status: 'ok',
            message: 'tools/call maya-1234__create_sphere',
            tool: 'maya-1234__create_sphere',
            duration_ms: 42,
            correlation: {
              request_id: 'req-123',
              session_id: 'session-1',
              instance_id: 'maya-1234567890',
              dcc_type: 'maya',
            },
          },
          {
            event_id: 'gateway:1',
            timestamp: '2026-05-18T08:00:01.000Z',
            kind: 'gateway_elected',
            severity: 'info',
            status: 'ok',
            message: 'gateway elected dcc_type=gateway instance=local',
            correlation: { instance_id: 'local', dcc_type: 'gateway' },
          },
        ],
      };
    } else if (path === '/workers') {
      body = {
        total: 2,
        summary: { live: 1, stale: 0, unhealthy: 1 },
        workers: [
          {
            instance_id: 'maya-1234567890',
            display_name: 'Maya Layout',
            dcc_type: 'maya',
            status: 'ready',
            stale: false,
            pid: 4242,
            uptime_secs: 120,
            version: '2026',
            adapter_version: '0.5.0',
            cpu_percent: 3.5,
            memory_bytes: 734003200,
            mcp_url: 'http://localhost:8765/mcp',
            scene: 'shot010.ma',
          },
          {
            instance_id: 'blender-abcdef1234',
            display_name: 'Blender Lookdev',
            dcc_type: 'blender',
            status: 'booting',
            stale: false,
            pid: null,
            uptime_secs: null,
            version: null,
            adapter_version: null,
            cpu_percent: null,
            memory_bytes: null,
            mcp_url: 'http://127.0.0.1:0/mcp',
            scene: null,
            failure_reason: 'host-rpc connect failed',
          },
        ],
      };
    } else if (path === '/tools') {
      body = {
        total: 2,
        tools: [
          {
            slug: 'maya-1234__create_sphere',
            dcc_type: 'maya',
            summary: 'Create a polygon sphere.',
            skill_name: 'modeling',
            name: 'create_sphere',
            instance_id: 'maya-1234567890',
            instance_prefix: 'maya-123',
          },
          {
            slug: 'blender-abcd__render_preview',
            dcc_type: 'blender',
            summary: 'Render a viewport preview.',
            skill_name: 'rendering',
            name: 'render_preview',
            instance_id: 'blender-abcdef1234',
            instance_prefix: 'blender-',
          },
        ],
      };
    } else if (path === '/tasks') {
      body = {
        total: 2,
        tasks: [
          {
            task_id: 'req-123',
            task_type: 'tool_call',
            status: 'completed',
            title: 'maya-1234__create_sphere',
            started_at: now,
            duration_ms: 42,
            correlation: {
              request_id: 'req-123',
              instance_id: 'maya-1234567890',
              dcc_type: 'maya',
            },
          },
          {
            task_id: 'req-err',
            task_type: 'tool_call',
            status: 'failed',
            title: 'blender-abcd__render_preview',
            started_at: '2026-05-18T08:01:00.000Z',
            duration_ms: 87,
            correlation: {
              request_id: 'req-err',
              instance_id: 'blender-abcdef1234',
              dcc_type: 'blender',
            },
          },
        ],
      };
    } else if (path === '/calls') {
      body = {
        total: 1,
        calls: [
          {
            timestamp: now,
            request_id: 'req-123',
            tool: 'maya-1234__create_sphere',
            dcc_type: 'maya',
            status: 'ok',
            success: true,
            error: null,
            duration_ms: 42,
            instance_id: 'maya-1234567890',
          },
        ],
      };
    } else if (path === '/traces') {
      body = {
        total: 2,
        traces: [
          {
            timestamp: now,
            request_id: 'req-123',
            tool: 'maya-1234__create_sphere',
            dcc_type: 'maya',
            status: 'ok',
            success: true,
            total_ms: 42,
            instance_id: 'maya-1234567890',
          },
          {
            timestamp: '2026-05-18T08:01:00.000Z',
            request_id: 'req-err',
            tool: 'blender-abcd__render_preview',
            dcc_type: 'blender',
            status: 'failed',
            success: false,
            total_ms: 87,
            instance_id: 'blender-abcdef1234',
          },
        ],
      };
    } else if (path === '/traces/req-123') {
      body = {
        request_id: 'req-123',
        method: 'tools/call',
        spans: [{ name: 'dispatch', duration_ms: 42 }],
      };
    } else if (path === '/stats') {
      body = {
        range: url.searchParams.get('range') ?? '24h',
        total_calls: 4,
        successful_calls: 3,
        failed_calls: 1,
        success_rate: 75,
        latency_ms: { p50_ms: 20, p95_ms: 90 },
        top_tools: [{ name: 'maya-1234__create_sphere', count: 3 }],
        top_instances: [{ name: 'maya-1234567890', count: 3 }],
        hourly_distribution: Array.from({ length: 24 }, (_, i) => (i === 8 ? 4 : 0)),
      };
    } else if (path === '/logs') {
      body = {
        total: 1,
        logs: [
          {
            timestamp: now,
            level: 'info',
            source: 'audit',
            message: 'tools/call ok 42ms — maya-1234__create_sphere',
            dcc_type: 'maya',
            instance_id: 'maya-1234567890',
            request_id: 'req-123',
            tool: 'maya-1234__create_sphere',
            success: true,
          },
          {
            timestamp: '2026-05-18T08:00:01.000Z',
            level: 'info',
            source: 'contention',
            event: 'gateway_elected',
            message: 'gateway elected dcc_type=gateway instance=local',
            dcc_type: 'gateway',
            instance_id: 'local',
          },
        ],
      };
    } else if (path === '/skills') {
      body = {
        total: 2,
        loaded: 2,
        unloaded: 0,
        action_count: 5,
        skills: [
          {
            name: 'maya-modeling',
            dcc_type: 'maya',
            loaded: true,
            action_count: 3,
            instance_count: 1,
            instances: ['12345678'],
            instance_ids: ['12345678-aaaa-bbbb-cccc-1234567890ab'],
            instance_details: [{ id: '12345678-aaaa-bbbb-cccc-1234567890ab', prefix: '12345678', dcc_type: 'maya' }],
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
            instance_ids: ['abcdef12-aaaa-bbbb-cccc-1234567890ab'],
            instance_details: [{ id: 'abcdef12-aaaa-bbbb-cccc-1234567890ab', prefix: 'abcdef12', dcc_type: 'blender' }],
            tools: ['render_preview', 'assign_material'],
            summary: 'Lookdev tools currently loaded by Blender.',
          },
        ],
      };
    } else if (path === '/skill-detail') {
      body = {
        skill: {
          name: url.searchParams.get('name') ?? 'maya-modeling',
          description: 'Modeling tools currently loaded by Maya.',
          dcc: 'maya',
          dcc_type: 'maya',
          state: 'loaded',
          instance_id: url.searchParams.get('instance_id') ?? '12345678-aaaa-bbbb-cccc-1234567890ab',
          instance_short: '12345678',
          skill_path: 'G:/studio/skills/maya-modeling',
          skill_md_path: 'G:/studio/skills/maya-modeling/SKILL.md',
          markdown: '---\nname: maya-modeling\nmetadata:\n  dcc-mcp:\n    dcc: maya\n---\n# Maya Modeling\n\n- Create a polygon sphere\n\n```python\ncmds.polySphere()\n```',
          tools: [{ name: 'create_sphere' }, { name: 'delete_sphere' }],
        },
        instances: [],
      };
    } else if (path === '/skill-paths' && method === 'GET') {
      body = { paths: state.skillPaths };
    } else if (path === '/skill-paths' && method === 'POST') {
      const payload = route.request().postDataJSON() as { path?: string };
      state.skillPaths.push({ id: 8, source: 'admin_custom', path: payload.path ?? '' });
      body = { ok: true, path: payload.path };
    } else if (path === '/skill-paths/7' && method === 'DELETE') {
      state.skillPaths = state.skillPaths.filter((row) => row.id !== 7);
      body = { ok: true, id: 7 };
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

test.beforeEach(async ({ page }) => {
  await mockAdminApi(page);
});

test.describe('Admin Page', () => {
  test('loads the connect panel and navigation', async ({ page }) => {
    await page.goto('/admin/');
    await expect(page.locator('.brand-tag')).toContainText('DCC-MCP Gateway');
    await expect(page.locator('h1')).toContainText('Admin Dashboard');
    await expect(page.getByRole('navigation').getByRole('link', { name: 'Connect IDE' })).toHaveClass(/active/);
    for (const label of ['Connect IDE', 'Debug', 'Activity', 'Health', 'Instances', 'Tools', 'Tasks', 'Calls', 'Traces', 'Stats', 'Skills', 'Logs', 'Docs']) {
      await expect(page.getByRole('navigation').getByRole('link', { name: label })).toBeVisible();
    }
    await expect(page.getByRole('navigation').getByRole('link', { name: 'Docs' })).toHaveAttribute('href', 'http://127.0.0.1:3721/docs');
    await expect(page.locator('.setup-panel')).toContainText('Claude Desktop');
    await expect(page.locator('.setup-panel')).toContainText('http://127.0.0.1:3721/mcp');
    await expect(page.locator('.setup-panel img.ide-icon')).toHaveCount(6);
    await expect(page.locator('.setup-panel .ide-config-preview').first()).toContainText('"dcc-mcp-gateway"');
    await page.locator('.setup-panel .ide-card').first().getByRole('button', { name: 'Copy' }).click();
    await expect(page.locator('.setup-panel')).toContainText('Copied Claude Desktop config');
    await page.getByRole('button', { name: 'Direct' }).click();
    await expect(page.locator('.setup-panel')).toContainText('Maya Layout');
    await expect(page.locator('.setup-panel .ide-config-preview').first()).toContainText('http://127.0.0.1:8765/mcp');
    await page.getByRole('navigation').getByRole('link', { name: 'Debug' }).click();
    await expect(page.locator('.debug-panel')).toContainText('Debug Workbench');
    await expect(page.locator('.debug-panel')).toContainText('Traffic Shape');
    await page.getByRole('navigation').getByRole('link', { name: 'Health' }).click();
    await expect(page.locator('.health-panel')).toContainText('0.17.7');
  });

  test('switches to instances, renders DCC cards, and filters rows', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Instances' }).click();
    await expect(page.locator('.instances-panel')).toBeVisible();
    await expect(page.locator('.dcc-icon')).toHaveCount(2);
    await expect(page.locator('.instances-panel')).toContainText('Access URL');
    await expect(page.locator('.instances-panel')).toContainText('http://127.0.0.1:8765');
    await expect(page.locator('.instances-panel')).toContainText('host-rpc connect failed');
    await expect(page.locator('.instances-panel').getByRole('link', { name: 'docs' }).first()).toHaveAttribute('href', 'http://127.0.0.1:8765/docs');
    await page.getByLabel('Filter current panel').fill('blender');
    await expect(page.locator('.worker-card')).toHaveCount(1);
    await expect(page.locator('.worker-card')).toContainText('Blender Lookdev');
  });

  test('opens trace detail from the calls panel and keeps the URL shareable', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Calls' }).click();
    await page.getByRole('button', { name: 'req-123' }).click();
    await expect(page).toHaveURL(/panel=traces/);
    await expect(page).toHaveURL(/trace=req-123/);
    await expect(page.locator('.trace-detail-panel')).toContainText('req-123');
    await expect(page.locator('.trace-detail-panel')).toContainText('dispatch');
  });

  test('shows reconstructed tasks and links them to traces', async ({ page }) => {
    await page.goto('/admin/?panel=tasks');
    await expect(page.locator('.tasks-panel')).toContainText('maya-1234__create_sphere');
    await expect(page.locator('.tasks-panel .badge-ok')).toContainText('completed');
    await expect(page.locator('.tasks-panel .badge-err')).toContainText('failed');
    await page.getByRole('button', { name: 'req-123' }).click();
    await expect(page).toHaveURL(/panel=traces/);
    await expect(page).toHaveURL(/trace=req-123/);
    await expect(page.locator('.trace-detail-panel')).toContainText('req-123');
  });

  test('updates stats when the range selector changes', async ({ page }) => {
    await page.goto('/admin/?panel=stats&range=1h');
    await expect(page.locator('.stats-panel')).toBeVisible();
    await expect(page.getByLabel('Range')).toHaveValue('1h');
    await page.getByLabel('Range').selectOption('7d');
    await expect(page).toHaveURL(/range=7d/);
    await expect(page.locator('.stats-panel')).toContainText('Range 7d');
  });

  test('adds and removes SQLite-backed skill paths', async ({ page }) => {
    await page.goto('/admin/?panel=skill-paths');
    await expect(page.locator('.skill-paths-panel')).toContainText('Skills & paths');
    await expect(page.locator('.skill-paths-panel')).toContainText('maya-modeling');
    await expect(page.locator('.skill-paths-panel')).toContainText('create_sphere');
    await expect(page.locator('.skill-paths-panel')).toContainText('Loaded skills');
    await expect(page.locator('.skill-paths-panel')).toContainText('G:/custom/admin-skills');
    await page.getByLabel('New skill path').fill('G:/new/team-skills');
    await page.getByRole('button', { name: 'Add path' }).click();
    await expect(page.locator('.skill-paths-panel')).toContainText('G:/new/team-skills');
    await page.getByRole('button', { name: 'Remove' }).first().click();
    await expect(page.locator('.skill-paths-panel')).not.toContainText('G:/custom/admin-skills');
  });

  test('opens rendered markdown details for a skill', async ({ page }) => {
    await page.goto('/admin/?panel=skill-paths');
    await page.getByRole('button', { name: 'maya-modeling' }).click();

    const detail = page.locator('.skill-detail-panel');
    await expect(detail).toContainText('G:/studio/skills/maya-modeling/SKILL.md');
    await expect(detail.locator('.skill-markdown-preview h3')).toHaveText('Maya Modeling');
    await expect(detail.locator('.skill-markdown-preview li')).toContainText('Create a polygon sphere');
    await expect(detail.locator('.skill-code-block')).toContainText('cmds.polySphere()');
    await expect(detail.locator('.skill-frontmatter')).toContainText('dcc: maya');
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
    await expect(page.locator('.logs-panel')).toContainText('Step 1: maya-1234__create_sphere');
    await expect(page.locator('.logs-panel')).toContainText('Gateway events');
    await expect(page.locator('.logs-panel')).toContainText('tools/call ok');
    await page.getByLabel('Filter current panel').fill('missing');
    await expect(page.locator('.list-search-meta')).toHaveText('0 / 2');
    await expect(page.locator('.logs-panel')).toContainText('No log lines match your search.');
  });
});
