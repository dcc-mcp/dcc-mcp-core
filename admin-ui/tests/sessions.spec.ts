import { test, expect, type Page } from '@playwright/test';
import { disableTestMotion, mockAdminApi } from './admin.fixtures';

// Match the production projections in gateway_admin_session_sqlite.rs and
// dcc-mcp-gateway-admin/src/sessions.rs, including fields absent from dev mocks.
async function mockSessionWireApi(page: Page) {
  const start = Date.parse('2026-05-18T08:00:00Z');
  const row = { dcc_type: 'maya', instance_id: 'maya-instance-1', started_at_ms: start,
    last_activity_at_ms: start + 1000, ended_at_ms: null, end_reason_json: null,
    tool_call_count: 42, error_count: 0, core_version: '0.19.60', adapter_version: '0.5.0', build_sha: null };
  await page.route('**/admin/api/sessions', (route) => route.fulfill({ json: {
    total: 3,
    summary: { active: 1, ended: 1, crashed: 1, disconnected: 0, by_dcc: { maya: 2, blender: 1 } },
    sessions: [
      { ...row, session_id: 'parent-session-1', parent_session_id: null, status: 'active' },
      { ...row, session_id: 'child-session-1', parent_session_id: 'parent-session-1', status: 'ended',
        ended_at_ms: start + 1000, end_reason_json: '{"normal":null}' },
      { ...row, session_id: 'crashed-session', parent_session_id: null, status: 'crashed', dcc_type: 'blender',
        ended_at_ms: start + 2000, end_reason_json: '{"host_crash":{"detail":"render stopped"}}' },
    ],
  } }));
}

test.describe('Sessions Panel', () => {
  test.beforeEach(async ({ page }) => {
    await disableTestMotion(page);
    await mockAdminApi(page);
    await mockSessionWireApi(page);
    await page.goto('/admin/?panel=sessions');
  });

  test('displays session list with summary metrics', async ({ page }) => {
    const panel = page.locator('section[data-panel="sessions"]');
    await expect(panel).toBeVisible();
    await expect(panel.locator('.sessions-row')).toHaveCount(3);
    await expect(panel.getByTitle('parent-session-1', { exact: true })).toBeVisible();
    await expect(panel.getByTitle('crashed-session', { exact: true })).toBeVisible();
    await expect(panel.locator('.sessions-summary dd')).toHaveText(['3', '1', '1', '2']);
  });

  test('shows status badges with correct colors', async ({ page }) => {
    const panel = page.locator('section[data-panel="sessions"]');
    await expect(panel.locator('.sessions-row').filter({ has: page.getByTitle('parent-session-1', { exact: true }) }).locator('.badge-ok')).toHaveText('Active');
    await expect(panel.locator('.sessions-row').filter({ has: page.getByTitle('crashed-session', { exact: true }) }).locator('.badge-err')).toHaveText('Crashed');
  });

  test('shows and collapses the parent-child tree', async ({ page }) => {
    const panel = page.locator('section[data-panel="sessions"]');
    const child = panel.locator('.sessions-row.child');
    await expect(child.getByTitle('child-session-1', { exact: true })).toBeVisible();
    const toggle = panel.locator('.sessions-row').filter({ has: page.getByTitle('parent-session-1', { exact: true }) }).locator('.sessions-tree-btn');
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await toggle.click();
    await expect(child).toHaveCount(0);
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await toggle.click();
    await expect(child.getByTitle('child-session-1', { exact: true })).toBeVisible();
  });

  test('opens session details and follows the session memory action', async ({ page }) => {
    const panel = page.locator('section[data-panel="sessions"]');
    const opener = panel.locator('.sessions-row').filter({ has: page.getByTitle('parent-session-1', { exact: true }) }).locator('.sessions-detail-btn');
    await opener.click();
    await expect(opener).toHaveAttribute('aria-expanded', 'true');
    const detail = panel.locator('.sessions-detail-row');
    await expect(detail).toContainText('parent-session-1');
    await expect(detail).toContainText('maya-instance-1');
    await expect(detail).toContainText('0.19.60');
    await expect(panel.locator('.sessions-row').first().locator('time')).toHaveAttribute('datetime', '2026-05-18T08:00:00.000Z');
    await expect(panel.locator('.sessions-row').first().locator('.session-activity small')).toHaveText('— Turns');
    await detail.getByRole('button', { name: 'View context', exact: true }).click();
    await expect(page).toHaveURL(/panel=sessions.*sessionsTab=memory/);
    await expect(page.locator('.memory-panel')).toBeVisible();
    await expect(page.locator('.memory-panel').getByPlaceholder('Session', { exact: true })).toHaveValue('parent-session-1');
  });
});
