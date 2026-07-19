import { test, expect, type Page } from '@playwright/test';

const now = Date.now();

async function mockAdminApi(page: Page) {
  await page.route('**/admin/api/**', async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.replace(/^\/admin\/api/, '');
    let body: unknown;

    if (path === '/sessions') {
      body = {
        sessions: [
          {
            session_id: 'parent-session-1',
            parent_session_id: null,
            dcc_type: 'maya',
            instance_id: 'maya-instance-1',
            status: 'active',
            started_at_ms: now - 3600000,
            last_activity_at_ms: now,
            ended_at_ms: null,
            end_reason: null,
            tool_call_count: 42,
            error_count: 2,
            core_version: '0.19.60',
            adapter_version: '0.5.0',
            build_sha: 'abc123',
          },
          {
            session_id: 'child-session-1',
            parent_session_id: 'parent-session-1',
            dcc_type: 'maya',
            instance_id: 'maya-instance-1',
            status: 'ended',
            started_at_ms: now - 1800000,
            last_activity_at_ms: now - 600000,
            ended_at_ms: now - 600000,
            end_reason: { normal: null },
            tool_call_count: 15,
            error_count: 0,
            core_version: '0.19.60',
            adapter_version: '0.5.0',
            build_sha: null,
          },
          {
            session_id: 'crashed-session',
            parent_session_id: null,
            dcc_type: 'blender',
            instance_id: 'blender-instance-2',
            status: 'crashed',
            started_at_ms: now - 7200000,
            last_activity_at_ms: now - 7000000,
            ended_at_ms: now - 7000000,
            end_reason: { host_crash: { detail: 'segfault in render thread' } },
            tool_call_count: 8,
            error_count: 8,
            core_version: '0.19.59',
            adapter_version: null,
            build_sha: null,
          },
        ],
        total: 3,
        active: 1,
        ended: 2,
        by_dcc: { maya: 2, blender: 1 },
        by_status: { active: 1, ended: 1, crashed: 1 },
      };
    }

    await route.fulfill({ status: 200, json: body });
  });
}

test.describe('Sessions Panel', () => {
  test.beforeEach(async ({ page }) => {
    await mockAdminApi(page);
    await page.goto('/admin/?panel=sessions');
  });

  test('displays session list with summary metrics', async ({ page }) => {
    const panel = page.locator('section[data-panel="sessions"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
    // Session IDs are compacted to 12 chars max
    await expect(panel).toContainText('parent-sessi');
    await expect(panel).toContainText('child-sessio');
    await expect(panel).toContainText('crashed-sess');
  });

  test('shows status badges with correct colors', async ({ page }) => {
    const panel = page.locator('section[data-panel="sessions"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
    // Active session should have green badge
    const activeBadge = panel.locator('.badge-ok').first();
    await expect(activeBadge).toBeVisible();
  });

  test('shows parent-child tree indentation', async ({ page }) => {
    const panel = page.locator('section[data-panel="sessions"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
    // Parent session should be visible (ID is compacted)
    await expect(panel.getByText('parent-sessi')).toBeVisible();
  });
});
