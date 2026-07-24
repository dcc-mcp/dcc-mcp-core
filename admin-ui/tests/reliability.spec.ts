import { test, expect, type Page } from '@playwright/test';

async function mockAdminApi(page: Page) {
  await page.route('**/admin/api/**', async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.replace(/^\/admin\/api/, '');
    let body: unknown;

    if (path === '/health') {
      body = {
        status: 'ok',
        instances_ready: 3,
        instances_total: 5,
        uptime_secs: 86400,
        version: '0.19.60',
        gateway: {
          current: {
            name: 'gateway-primary',
            role: 'active',
            host: '127.0.0.1',
            port: 9765,
            instance_id: 'gw-001',
            version: '0.19.60',
          },
          candidates: [
            {
              name: 'gateway-standby',
              role: 'standby',
              host: '127.0.0.1',
              port: 9766,
              instance_id: 'gw-002',
              version: '0.19.60',
            },
          ],
        },
        limits: {
          body_max_bytes: 1048576,
          rate_limit_per_minute_per_ip: 60,
          xff_trusted_depth: 1,
          read_retry_max: 2,
          circuit_failure_threshold: 3,
          circuit_open_secs: 30,
        },
        circuits: { tracked_backends: 5, circuits_open: 1 },
      };
    } else if (path === '/instances') {
      body = [
        {
          instance_id: 'maya-001',
          display_name: 'Maya Layout',
          dcc_type: 'maya',
          status: 'ready',
          stale: false,
          pid: 1001,
          uptime_secs: 7200,
          version: '2026',
          server_version: '0.19.60',
          mcp_url: 'http://localhost:8765/mcp',
        },
        {
          instance_id: 'blender-001',
          display_name: 'Blender Render',
          dcc_type: 'blender',
          status: 'booting',
          stale: false,
          pid: null,
          uptime_secs: null,
          version: null,
          server_version: null,
          mcp_url: 'http://localhost:0/mcp',
          failure_reason: 'host-rpc connect failed',
        },
      ];
    } else if (path === '/stats') {
      body = {
        range: '24h',
        total_calls: 1500,
        successful_calls: 1470,
        failed_calls: 30,
        success_rate: 98.0,
        total_tokens: 500000,
        p50_ms: 120,
        p95_ms: 450,
        p99_ms: 890,
      };
    } else if (path === '/skills') {
      body = {
        skills: [],
        total: 50,
        loaded: 40,
        unloaded: 10,
        action_count: 180,
      };
    } else if (path === '/reliability') {
      body = {
        generated_at: new Date().toISOString(),
        status: 'ok',
        uptime_secs: 86400,
        version: '0.19.60',
        gateway: {
          status: 'ok',
          uptime_secs: 86400,
          version: '0.19.60',
          election: {
            current: {
              name: 'gateway-primary',
              role: 'active',
              host: '127.0.0.1',
              port: 9765,
              instance_id: 'gw-001',
              version: '0.19.60',
            },
            candidates: [
              {
                name: 'gateway-standby',
                role: 'standby',
                host: '127.0.0.1',
                port: 9766,
                instance_id: 'gw-002',
                version: '0.19.60',
              },
            ],
          },
          limits: {
            body_max_bytes: 1048576,
            rate_limit_per_minute_per_ip: 60,
            circuit_failure_threshold: 3,
            circuit_open_secs: 30,
          },
          circuits: { tracked_backends: 5, circuits_open: 1 },
        },
        capability_funnel: {
          instances_ready: 3,
          instances_total: 5,
          skills_loaded: 40,
          skills_total: 50,
          tools_registered: 180,
          resources_exposed: 60,
        },
        artifact_verification: {
          builds_verified: 12,
          builds_total: 15,
          verification_errors: 1,
        },
        stability: {
          crashes_24h: 2,
          reconnects_24h: 5,
          recoveries_24h: 4,
          uptime_pct: 99.7,
          p50_latency_ms: 120,
        },
      };
    } else if (path === '/artifacts') {
      body = {
        total: 1,
        artifacts: [{
          uri: 'artefact://sha256/abc',
          display_name: 'ui-control-snapshot-evidence-accessibility-1.png',
          session_id: 'evidence',
          verification: { status: 'verified' },
        }],
        summary: { verified: 1, unverified: 0, failed: 0 },
      };
    }

    await route.fulfill({ status: 200, json: body });
  });
}

test.describe('Reliability Panel', () => {
  test.beforeEach(async ({ page }) => {
    await mockAdminApi(page);
    await page.goto('/admin/?panel=reliability');
  });

  test('displays gateway health section', async ({ page }) => {
    const panel = page.locator('section[data-panel="reliability"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
    await expect(panel.getByText('0.19.60')).toBeVisible();
    await expect(panel.getByText('gateway-primary')).toBeVisible();
  });

  test('shows circuit breaker state', async ({ page }) => {
    const panel = page.locator('section[data-panel="reliability"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
    // Circuits tracked (5) should be visible in the panel
    await expect(panel).toContainText('5');
  });

  test('shows capability funnel metrics', async ({ page }) => {
    const panel = page.locator('section[data-panel="reliability"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
    await expect(panel).toContainText('99.7');
  });

  test('shows stability data from /api/reliability', async ({ page }) => {
    const panel = page.locator('section[data-panel="reliability"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
    // Stability section should show crash count
    await expect(panel).toContainText('2');
    // Reconnect count
    await expect(panel).toContainText('5');
    // Recovery count
    await expect(panel).toContainText('4');
  });

  test('shows capability funnel with real data', async ({ page }) => {
    const panel = page.locator('section[data-panel="reliability"]');
    await expect(panel).toBeVisible({ timeout: 10_000 });
    // Instances ready / total
    await expect(panel).toContainText('1 / 2');
    // Skills loaded / total
    await expect(panel).toContainText('40 / 50');
    // Tools registered
    await expect(panel).toContainText('180');
  });

  test('shows traceable UI Control artifacts', async ({ page }) => {
    const panel = page.locator('section[data-panel="reliability"]');
    await expect(panel).toContainText('ui-control-snapshot-evidence-accessibility-1.png');
    await expect(panel).toContainText('evidence');
  });
});
