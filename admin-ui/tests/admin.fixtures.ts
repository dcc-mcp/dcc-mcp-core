import { type Page, expect } from '@playwright/test';

export const analyticsSeriesFixture = [
  { date: '2025-06-02', calls: 1, failures: 0, tokens_input: 120, tokens_output: 90, avg_duration_ms: '36' },
  { date: '2025-07-08', calls: 2, failures: 0, tokens_input: 210, tokens_output: 140, avg_duration_ms: '48' },
  { date: '2025-08-14', calls: 2, failures: 0, tokens_input: 260, tokens_output: 160, avg_duration_ms: '54' },
  { date: '2025-09-03', calls: 1, failures: 0, tokens_input: 160, tokens_output: 110, avg_duration_ms: '42' },
  { date: '2025-10-20', calls: 3, failures: 0, tokens_input: 410, tokens_output: 260, avg_duration_ms: '62' },
  { date: '2025-11-18', calls: 2, failures: 0, tokens_input: 280, tokens_output: 180, avg_duration_ms: '58' },
  { date: '2025-12-09', calls: 2, failures: 0, tokens_input: 310, tokens_output: 220, avg_duration_ms: '66' },
  { date: '2026-01-22', calls: 3, failures: 0, tokens_input: 460, tokens_output: 330, avg_duration_ms: '72' },
  { date: '2026-02-11', calls: 1, failures: 0, tokens_input: 180, tokens_output: 120, avg_duration_ms: '39' },
  { date: '2026-03-04', calls: 4, failures: 0, tokens_input: 580, tokens_output: 390, avg_duration_ms: '82' },
  { date: '2026-04-15', calls: 3, failures: 0, tokens_input: 520, tokens_output: 360, avg_duration_ms: '76' },
  { date: '2026-05-01', calls: 3, failures: 0, tokens_input: 620, tokens_output: 430, avg_duration_ms: '88' },
  { date: '2026-05-04', calls: 2, failures: 0, tokens_input: 420, tokens_output: 300, avg_duration_ms: '70' },
  { date: '2026-05-06', calls: 3, failures: 1, tokens_input: 760, tokens_output: 520, avg_duration_ms: '210' },
  { date: '2026-05-08', calls: 2, failures: 0, tokens_input: 500, tokens_output: 340, avg_duration_ms: '79' },
  { date: '2026-05-11', calls: 3, failures: 0, tokens_input: 900, tokens_output: 680, avg_duration_ms: '94' },
  { date: '2026-05-13', calls: 2, failures: 0, tokens_input: 620, tokens_output: 440, avg_duration_ms: '84' },
  { date: '2026-05-15', calls: 3, failures: 0, tokens_input: 1100, tokens_output: 820, avg_duration_ms: '98' },
  { date: '2026-05-17', calls: 2, failures: 0, tokens_input: 780, tokens_output: 560, avg_duration_ms: '92' },
  { date: '2026-05-18', calls: 3, failures: 1, tokens_input: 1280, tokens_output: 940, avg_duration_ms: '240' },
];

export const analyticsTotals = analyticsSeriesFixture.reduce(
  (totals, point) => ({
    calls: totals.calls + point.calls,
    failures: totals.failures + point.failures,
    tokensInput: totals.tokensInput + point.tokens_input,
    tokensOutput: totals.tokensOutput + point.tokens_output,
  }),
  { calls: 0, failures: 0, tokensInput: 0, tokensOutput: 0 },
);

export const now = '2026-05-18T08:00:00.000Z';

export async function chooseSidebarSelectOption(page: Page, triggerId: string, optionName: RegExp | string) {
  await page.locator(`#${triggerId}`).click();
  await page.getByRole('option', { name: optionName }).click();
}

export async function openIntegrationEditor(page: Page, kind: string) {
  const card = page.locator(`.integration-card[data-kind="${kind}"]`);
  await expect(card).toBeVisible();
  await card.locator('button').first().click({ force: true });
  await expect(page.locator('.integration-edit-form')).toBeVisible();
}

export async function disableTestMotion(page: Page) {
  await page.addInitScript(() => {
    const install = () => {
      if (document.getElementById('admin-test-disable-motion')) return;
      const style = document.createElement('style');
      style.id = 'admin-test-disable-motion';
      style.textContent = `
        *, *::before, *::after {
          animation-delay: 0s !important;
          animation-duration: 0s !important;
          scroll-behavior: auto !important;
          transition-delay: 0s !important;
          transition-duration: 0s !important;
        }
      `;
      document.head.appendChild(style);
    };
    if (document.head) {
      install();
    } else {
      document.addEventListener('DOMContentLoaded', install, { once: true });
    }
  });
}

export async function mockAdminApi(page: Page) {
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
        response_format: {
          default: 'toon',
          legacy_mime: 'application/json',
          compact_mime: 'application/toon',
          token_estimator: 'dcc-mcp-byte4-v1',
        },
        gateway: {
          current: {
            name: 'local-gateway',
            role: 'active',
            host: '127.0.0.1',
            port: 9765,
            instance_id: 'gateway-1234567890',
            version: '0.17.7',
            adapter_version: null,
            adapter_dcc: null,
          },
          candidates: [],
        },
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
              actor_id: 'artist-1',
              actor_name: 'Layout Artist',
              client_platform: 'cursor',
              source_ip: '192.0.2.44',
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
            server_version: '0.19.56',
            adapter_version: '0.5.0',
            instance_type: 'gui',
            cpu_percent: 3.5,
            memory_bytes: 734003200,
            mcp_url: 'http://localhost:8765/mcp',
            scene: 'shot010.ma',
            dispatch_status: 'ready',
            dispatch_ready: true,
            dispatch_ready_at_unix: '1780367000',
            host_rpc_uri: 'commandport://127.0.0.1:6000',
            host_rpc_scheme: 'commandport',
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
            server_version: null,
            adapter_version: null,
            instance_type: 'standalone',
            cpu_percent: null,
            memory_bytes: null,
            mcp_url: 'http://127.0.0.1:0/mcp',
            scene: null,
            failure_reason: 'host-rpc connect failed',
            failure_stage: 'host-rpc-connect',
            dispatch_status: 'unavailable',
            dispatch_ready: false,
            host_rpc_uri: 'commandport://127.0.0.1:6001',
            host_rpc_scheme: 'commandport',
          },
        ],
      };
    } else if (/^\/instances\/[^/]+\/update$/.test(path) && method === 'POST') {
      const instanceId = decodeURIComponent(path.split('/')[2] ?? '');
      if (instanceId.startsWith('blender-')) {
        status = 404;
        body = {
          status: 'binary_not_found',
          error: 'binary_not_found',
          message: "Binary 'dcc-mcp-server' was not found in the update manifest.",
          instance_id: instanceId,
          instance_short: 'blender-ab',
          binary_name: 'dcc-mcp-server',
          current_version: null,
          current_version_source: 'unknown',
          update_available: false,
          requires_restart: false,
        };
      } else {
        body = {
          status: 'available',
          instance_id: instanceId,
          instance_short: 'maya-123',
          binary_name: 'dcc-mcp-server',
          current_version: '0.19.56',
          current_version_source: 'instance_metadata',
          latest_version: '0.18.0',
          update_available: true,
          requires_restart: false,
          message: 'An update is available.',
        };
      }
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
            task_id: 'session-1:turn-1',
            task_type: 'agent_turn',
            status: 'completed',
            title: 'Create a sphere with the least risky MCP path.',
            goal: 'Create a sphere after discovery.',
            summary: 'Create a sphere with the least risky MCP path.',
            final_result: 'Produced viewport preview and validated the scene.',
            started_at: now,
            finished_at: '2026-05-18T08:00:06.000Z',
            duration_ms: 6000,
            app_types: ['maya'],
            artifacts: [
              { name: 'viewport-preview.png', kind: 'render', request_id: 'req-artifact' },
            ],
            validation_checks: [
              { title: 'validate sphere scene output', status: 'completed', request_id: 'req-validate' },
            ],
            related: {
              workflow_ids: ['session-1'],
              request_ids: ['req-search', 'req-describe', 'req-load', 'req-123', 'req-artifact', 'req-validate'],
              trace_ids: ['trace-workflow'],
              session_ids: ['session-1'],
            },
            correlation: {
              request_id: 'req-123',
              workflow_id: 'session-1',
              trace_id: 'trace-workflow',
              session_id: 'session-1',
              instance_id: 'maya-1234567890',
              dcc_type: 'maya',
              agent_id: 'agent-1',
              actor_name: 'Layout Artist',
              client_platform: 'cursor',
            },
          },
          {
            task_id: 'lookdev-fail',
            task_type: 'session_task',
            status: 'failed',
            title: 'Render preview for lookdev review',
            goal: 'Render a lookdev preview.',
            failure_reason: 'Backend failed while opening [path-redacted].',
            started_at: '2026-05-18T08:01:00.000Z',
            finished_at: '2026-05-18T08:01:00.087Z',
            duration_ms: 87,
            app_types: ['blender'],
            artifacts: [
              { name: 'render preview', kind: 'render', request_id: 'req-err' },
            ],
            related: {
              workflow_ids: ['lookdev-fail'],
              request_ids: ['req-err'],
              trace_ids: ['trace-error'],
              session_ids: ['session-err'],
            },
            correlation: {
              request_id: 'req-err',
              workflow_id: 'lookdev-fail',
              trace_id: 'trace-error',
              session_id: 'session-err',
              instance_id: 'blender-abcdef1234',
              dcc_type: 'blender',
              client_platform: 'codebuddy',
            },
          },
        ],
      };
    } else if (path === '/workflows') {
      body = {
        total: 2,
        summary: { failed: 1, warning: 0, zero_result_workflows: 1 },
        workflows: [
          {
            workflow_id: 'session-1',
            group_kind: 'session',
            title: 'Scene Builder: maya-1234__create_sphere',
            status: 'completed',
            started_at: now,
            finished_at: '2026-05-18T08:00:06.000Z',
            duration_ms: 6000,
            step_count: 7,
            failed_steps: 0,
            agent: {
              agent_id: 'agent-1',
              agent_name: 'Scene Builder',
              model_provider: 'openai',
              model_version: 'gpt-5.1',
              model: 'gpt-test',
              reasoning_effort: 'medium',
              session_id: 'session-1',
              turn_id: 'turn-1',
              task: 'Create a sphere after discovery.',
              user_intent_summary: 'Create a sphere with the least risky MCP path.',
              agent_reply_summary: 'I found the tool and executed it successfully.',
              user_input_hash: 'sha256:user',
              agent_reply_hash: 'sha256:reply',
              user_input_chars: 180,
              agent_reply_chars: 220,
              tags: ['smoke'],
            },
            correlation: {
              session_id: 'session-1',
              trace_id: 'trace-workflow',
              agent_id: 'agent-1',
              turn_id: 'turn-1',
              request_ids: ['req-search', 'req-describe', 'req-load', 'req-123', 'req-fallback', 'req-artifact', 'req-validate'],
              trace_ids: ['trace-workflow'],
              session_ids: ['session-1'],
            },
            discovery: {
              search_count: 1,
              zero_result_count: 0,
              selected_count: 3,
              best_selected_rank: 2,
              time_to_first_success_ms: 310,
              search_ids: ['search-1'],
            },
            steps: [
              {
                step_id: 'search:search-1',
                kind: 'search',
                title: 'search create sphere',
                timestamp: now,
                status: 'ok',
                success: true,
                request_id: 'req-search',
                trace_id: 'trace-workflow',
                session_id: 'session-1',
                dcc_type: 'maya',
                transport: 'rest',
                search: { search_id: 'search-1', zero_results: false, result_count: 2, first_success_ms: 310 },
              },
              {
                step_id: 'describe:req-describe',
                kind: 'describe',
                title: 'maya-1234__create_sphere',
                timestamp: '2026-05-18T08:00:01.000Z',
                status: 'ok',
                success: true,
                request_id: 'req-describe',
                trace_id: 'trace-workflow',
                parent_request_id: 'req-search',
                session_id: 'session-1',
                dcc_type: 'maya',
                transport: 'rest',
                search: { search_id: 'search-1', selected_rank: 2, selected_score: 88, match_reasons: ['skill_match'] },
              },
              {
                step_id: 'load_skill:req-load',
                kind: 'load_skill',
                title: 'load_skill maya-modeling',
                timestamp: '2026-05-18T08:00:02.000Z',
                status: 'ok',
                success: true,
                request_id: 'req-load',
                trace_id: 'trace-workflow',
                parent_request_id: 'req-describe',
                session_id: 'session-1',
                dcc_type: 'maya',
                transport: 'rest',
                search: { search_id: 'search-1', selected_rank: 2, selected_score: 88 },
              },
              {
                step_id: 'call:req-123',
                kind: 'call',
                title: 'maya-1234__create_sphere',
                timestamp: '2026-05-18T08:00:04.000Z',
                status: 'ok',
                success: true,
                request_id: 'req-123',
                trace_id: 'trace-workflow',
                parent_request_id: 'req-load',
                session_id: 'session-1',
                dcc_type: 'maya',
                instance_id: 'maya-1234567890',
                transport: 'rest',
                duration_ms: 42,
                search: { search_id: 'search-1', selected_rank: 2, selected_score: 88, first_success_ms: 310 },
                links: {
                  debug_bundle_url: 'http://127.0.0.1:3721/admin/api/debug-bundle/req-123',
                  issue_report_url: 'http://127.0.0.1:3721/admin/api/issue-report/req-123',
                  openapi_docs_url: 'http://127.0.0.1:3721/docs',
                },
              },
              {
                step_id: 'fallback:req-fallback',
                kind: 'fallback_script',
                title: 'execute python fallback for material check',
                timestamp: '2026-05-18T08:00:05.000Z',
                status: 'warning',
                success: true,
                request_id: 'req-fallback',
                trace_id: 'trace-workflow',
                parent_request_id: 'req-123',
                session_id: 'session-1',
                dcc_type: 'maya',
                instance_id: 'maya-1234567890',
                transport: 'mcp',
                duration_ms: 220,
              },
              {
                step_id: 'artifact:req-artifact',
                kind: 'artifact',
                title: 'artifact viewport-preview.png',
                timestamp: '2026-05-18T08:00:05.500Z',
                status: 'ok',
                success: true,
                request_id: 'req-artifact',
                trace_id: 'trace-workflow',
                parent_request_id: 'req-fallback',
                session_id: 'session-1',
                dcc_type: 'maya',
                instance_id: 'maya-1234567890',
                transport: 'rest',
                duration_ms: 120,
              },
              {
                step_id: 'validation:req-validate',
                kind: 'validation',
                title: 'validate sphere scene output',
                timestamp: '2026-05-18T08:00:06.000Z',
                status: 'ok',
                success: true,
                request_id: 'req-validate',
                trace_id: 'trace-workflow',
                parent_request_id: 'req-artifact',
                session_id: 'session-1',
                dcc_type: 'maya',
                instance_id: 'maya-1234567890',
                transport: 'rest',
                duration_ms: 80,
              },
            ],
          },
          {
            workflow_id: 'search-zero',
            group_kind: 'search',
            title: 'search missing tool',
            status: 'warning',
            started_at: '2026-05-18T08:02:00.000Z',
            finished_at: '2026-05-18T08:02:00.000Z',
            duration_ms: 0,
            step_count: 1,
            failed_steps: 0,
            correlation: { request_ids: [], trace_ids: [], session_ids: [] },
            discovery: {
              search_count: 1,
              zero_result_count: 1,
              selected_count: 0,
              search_ids: ['search-zero'],
            },
            steps: [
              {
                step_id: 'search:search-zero',
                kind: 'search',
                title: 'search missing tool',
                timestamp: '2026-05-18T08:02:00.000Z',
                status: 'zero_results',
                success: false,
                dcc_type: 'blender',
                transport: 'mcp',
                search: { search_id: 'search-zero', zero_results: true, result_count: 0 },
              },
            ],
          },
        ],
      };
    } else if (path === '/calls') {
      body = {
        total: 6,
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
            transport: 'rest',
            actor: 'Layout Artist',
            actor_id: 'artist-1',
            actor_name: 'Layout Artist',
            client_platform: 'cursor',
            client_os: 'windows',
            client_host: 'workstation-7',
            auth_subject: 'user:artist-1',
            source_ip: '192.0.2.44',
            attribution_trust: {
              actor_id: 'self_reported',
              actor_name: 'self_reported',
              client_platform: 'header',
              client_os: 'header',
              client_host: 'header',
              auth_subject: 'auth',
              source_ip: 'server_derived',
            },
            token_accounting: {
              response_format: 'toon',
              token_estimator: 'dcc-mcp-byte4-v1',
              original_bytes: 400,
              returned_bytes: 160,
              original_tokens: 100,
              returned_tokens: 40,
              saved_tokens: 60,
              savings_pct: 60,
            },
          },
          {
            timestamp: '2026-05-18T08:00:30.000Z',
            request_id: 'req-json',
            tool: 'maya-1234__describe',
            dcc_type: 'maya',
            status: 'ok',
            success: true,
            error: null,
            duration_ms: 18,
            instance_id: 'maya-1234567890',
            transport: 'mcp',
            response_format: 'json',
            token_estimator: 'dcc-mcp-byte4-v1',
            original_tokens: 50,
            returned_tokens: 50,
            saved_tokens: 0,
            savings_pct: 0,
          },
          {
            timestamp: '2026-05-18T08:00:40.000Z',
            request_id: 'req-slow',
            tool: 'maya-1234__bake_cache',
            dcc_type: 'maya',
            status: 'ok',
            success: true,
            error: null,
            duration_ms: 6200,
            instance_id: 'maya-1234567890',
            transport: 'rest',
          },
          {
            timestamp: '2026-05-18T08:00:45.000Z',
            request_id: 'req-failed-fast',
            tool: 'maya-1234__validate_scene',
            dcc_type: 'maya',
            status: 'failed',
            success: false,
            error: 'Validation failed',
            duration_ms: 120,
            instance_id: 'maya-1234567890',
            transport: 'rest',
          },
          {
            timestamp: '2026-05-18T08:00:50.000Z',
            request_id: 'req-failed-slow',
            tool: 'blender-abcd__render_preview',
            dcc_type: 'blender',
            status: 'failed',
            success: false,
            error: 'Render timed out',
            duration_ms: 3200,
            instance_id: 'blender-abcdef1234',
            transport: 'rest',
          },
          {
            timestamp: '2026-05-18T08:01:00.000Z',
            request_id: 'req-legacy',
            tool: 'blender-abcd__render_preview',
            dcc_type: 'blender',
            status: 'ok',
            success: true,
            error: null,
            duration_ms: 77,
            instance_id: 'blender-abcdef1234',
          },
        ],
      };
    } else if (path === '/traces') {
      body = {
        total: 5,
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
            actor: 'Layout Artist',
            actor_id: 'artist-1',
            actor_name: 'Layout Artist',
            client_platform: 'cursor',
            client_os: 'windows',
            client_host: 'workstation-7',
            auth_subject: 'user:artist-1',
            source_ip: '192.0.2.44',
            attribution_trust: {
              actor_id: 'self_reported',
              actor_name: 'self_reported',
              client_platform: 'header',
              client_os: 'header',
              client_host: 'header',
              auth_subject: 'auth',
              source_ip: 'server_derived',
            },
            input_tokens: 28,
            output_tokens: 18,
            total_tokens: 46,
            payload_token_estimator: 'dcc-mcp-byte4-v1',
            slowest_span_name: 'dispatch',
            slowest_span_ms: 42,
            token_accounting: {
              response_format: 'toon',
              token_estimator: 'dcc-mcp-byte4-v1',
              original_bytes: 400,
              returned_bytes: 160,
              original_tokens: 100,
              returned_tokens: 40,
              saved_tokens: 60,
              savings_pct: 60,
            },
          },
          {
            timestamp: '2026-05-18T08:00:40.000Z',
            request_id: 'req-slow',
            tool: 'maya-1234__bake_cache',
            dcc_type: 'maya',
            status: 'ok',
            success: true,
            total_ms: 6200,
            instance_id: 'maya-1234567890',
            span_count: 3,
            slowest_span_name: 'upload_texture',
            slowest_span_ms: 5400,
          },
          {
            timestamp: '2026-05-18T08:00:45.000Z',
            request_id: 'req-failed-fast',
            tool: 'maya-1234__validate_scene',
            dcc_type: 'maya',
            status: 'failed',
            success: false,
            total_ms: 120,
            instance_id: 'maya-1234567890',
            span_count: 2,
            slowest_span_name: 'validate',
            slowest_span_ms: 40,
          },
          {
            timestamp: '2026-05-18T08:00:50.000Z',
            request_id: 'req-failed-slow',
            tool: 'blender-abcd__render_preview',
            dcc_type: 'blender',
            status: 'failed',
            success: false,
            total_ms: 3200,
            instance_id: 'blender-abcdef1234',
            span_count: 3,
            slowest_span_name: 'render',
            slowest_span_ms: 3100,
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
            span_count: 1,
          },
        ],
      };
    } else if (path === '/traces/req-123') {
      body = {
        request_id: 'req-123',
        method: 'tools/call',
        total_ms: 42,
        ok: true,
        spans: [{ name: 'dispatch', started_ns: 0, duration_ns: 42000000, ok: true }],
        agent_context: {
          actor_id: 'artist-1',
          actor_name: 'Layout Artist',
          actor_email_hash: 'sha256:artist-1',
          agent_id: 'agent-1',
          agent_name: 'Scene Builder',
          client_platform: 'cursor',
          client_os: 'windows',
          client_host: 'workstation-7',
          auth_subject: 'user:artist-1',
          source_ip: '192.0.2.44',
          forwarded_for: ['198.51.100.7'],
          trust: {
            actor_id: 'self_reported',
            actor_name: 'self_reported',
            client_platform: 'header',
            client_os: 'header',
            client_host: 'header',
            auth_subject: 'auth',
            source_ip: 'trusted_proxy',
            forwarded_for: 'trusted_proxy',
          },
          model: 'gpt-test',
          session_id: 'session-1',
          task: 'Create a sphere and report scene status.',
          user_intent_summary: 'Artist asked for a simple sphere.',
          plan: ['Validate the scene context', 'Call create_sphere'],
          observations: ['Dispatch completed on Maya'],
          agent_reply_summary: 'Sphere created.',
        },
        input: {
          content: '{"radius":1}',
          mime_type: 'application/json',
          truncated: false,
          original_size: 12,
          estimated_tokens: 3,
        },
        output: {
          content: '{"ok":true}',
          mime_type: 'application/json',
          truncated: false,
          original_size: 11,
          estimated_tokens: 3,
        },
        input_tokens: 3,
        output_tokens: 3,
        total_tokens: 6,
        estimated_total_tokens: 6,
        payload_token_estimator: 'dcc-mcp-byte4-v1',
        token_accounting: {
          response_format: 'toon',
          token_estimator: 'dcc-mcp-byte4-v1',
          original_bytes: 400,
          returned_bytes: 160,
          original_tokens: 100,
          returned_tokens: 40,
          saved_tokens: 60,
          savings_pct: 60,
        },
      };
    } else if (path === '/traces/req-slow') {
      body = {
        request_id: 'req-slow',
        method: 'tools/call',
        tool_slug: 'maya-1234__bake_cache',
        total_ms: 6200,
        ok: true,
        started_at: '2026-05-18T08:00:40.000Z',
        transport: 'rest',
        spans: [
          { name: 'queue', duration_ns: 300000000, ok: true },
          { name: 'upload_texture', duration_ns: 5400000000, ok: true },
          { name: 'dispatch', duration_ns: 500000000, ok: true },
        ],
      };
    } else if (path === '/stats') {
      body = {
        range: url.searchParams.get('range') ?? '24h',
        total_calls: 6,
        successful_calls: 4,
        failed_calls: 2,
        success_rate: 66.7,
        latency_ms: { p50_ms: 77, p95_ms: 6200, p99_ms: 6200 },
        total_input_tokens: 120,
        total_output_tokens: 130,
        total_tokens: 250,
        avg_input_tokens_per_call: 30,
        avg_output_tokens_per_call: 32.5,
        avg_total_tokens_per_call: 62.5,
        avg_tokens_per_call: 62.5,
        payload_token_estimator: 'dcc-mcp-byte4-v1',
        top_app_types: [{ name: 'maya', count: 3 }, { name: 'blender', count: 1 }],
        top_tools: [{ name: 'maya-1234__create_sphere', count: 3 }],
        top_instances: [{ name: 'maya-1234567890', count: 3 }],
        top_agents: [{ name: 'Scene Builder', count: 2 }],
        top_actors: [{ name: 'Layout Artist', count: 2, failed: 0, failure_rate: 0, mean_latency_ms: 42, p95_latency_ms: 42 }],
        top_client_platforms: [{ name: 'cursor', count: 2, failed: 0, failure_rate: 0, mean_latency_ms: 42, p95_latency_ms: 42 }],
        top_source_ips: [{ name: '192.0.2.44', count: 2, failed: 0, failure_rate: 0, mean_latency_ms: 42, p95_latency_ms: 42 }],
        token_usage: {
          total_original_bytes: 1200,
          total_returned_bytes: 640,
          total_original_tokens: 300,
          total_returned_tokens: 160,
          total_saved_tokens: 140,
          average_savings_pct: 46.67,
          by_tool: [{ name: 'maya-1234__create_sphere', calls: 2, returned_tokens: 80, saved_tokens: 120, savings_pct: 60 }],
          by_instance: [{ name: 'maya-1234567890', calls: 2, returned_tokens: 80, saved_tokens: 120, savings_pct: 60 }],
          by_agent: [{ name: 'Scene Builder', calls: 2, returned_tokens: 80, saved_tokens: 120, savings_pct: 60 }],
          by_transport: [
            { name: 'rest', calls: 2, returned_tokens: 80, saved_tokens: 120, savings_pct: 60 },
            { name: 'mcp', calls: 1, returned_tokens: 80, saved_tokens: 20, savings_pct: 20 },
          ],
          by_response_format: [
            { name: 'toon', calls: 2, returned_tokens: 110, saved_tokens: 140, savings_pct: 56 },
            { name: 'json', calls: 1, returned_tokens: 50, saved_tokens: 0, savings_pct: 0 },
          ],
        },
        hourly_distribution: Array.from({ length: 24 }, (_, i) => (i === 8 ? 4 : 0)),
        governance: {
          recent_allowed: 1,
          recent_policy_denied: 1,
          recent_throttled: 1,
          captured_frames: 1,
          skipped_capture_frames: 2,
          redacted_path_count: 1,
          redacted_paths: ['body.data.params.arguments.api_key'],
        },
      };
    } else if (path === '/analytics/overview') {
      body = {
        range: url.searchParams.get('range') ?? '30d',
        period_start: '2026-05-01T00:00:00.000Z',
        period_end: now,
        kpi: {
          calls_total: analyticsTotals.calls,
          calls_failed: analyticsTotals.failures,
          failure_rate_pct: ((analyticsTotals.failures / analyticsTotals.calls) * 100).toFixed(1),
          success_rate_pct: (((analyticsTotals.calls - analyticsTotals.failures) / analyticsTotals.calls) * 100).toFixed(1),
          tokens_input_total: analyticsTotals.tokensInput,
          tokens_output_total: analyticsTotals.tokensOutput,
          tokens_response_saved: 440,
          tokens_total: analyticsTotals.tokensInput + analyticsTotals.tokensOutput,
          llm_tokens_total: 880,
          avg_duration_ms: '84',
          avg_tokens_per_call: Math.round((analyticsTotals.tokensInput + analyticsTotals.tokensOutput) / analyticsTotals.calls).toString(),
          unique_instances: 2,
          unique_agents: 2,
        },
        top_tools: [
          { name: 'maya-1234__create_sphere', calls: 21, failures: 0, success_rate_pct: 100, avg_duration_ms: 42 },
          { name: 'houdini-5678__submit_render', calls: 6, failures: 2, success_rate_pct: 66.7, avg_duration_ms: 240 },
        ],
        daily_series: [],
      };
    } else if (path === '/analytics/timeseries') {
      body = {
        series: analyticsSeriesFixture,
      };
    } else if (path === '/analytics/heatmap') {
      body = {
        heatmap: [
          { weekday: 0, hour: 9, calls: 2, failures: 0, avg_duration_ms: 40, tokens_total: 80 },
          { weekday: 1, hour: 10, calls: 8, failures: 1, avg_duration_ms: 120, tokens_total: 240 },
          { weekday: 2, hour: 14, calls: 4, failures: 0, avg_duration_ms: 75, tokens_total: 160 },
        ],
      };
    } else if (path === '/governance') {
      body = {
        schema_version: 'dcc-mcp.admin.governance.v1',
        generated_at: now,
        mode: {
          admin_mutations: 'disabled',
          reason: 'Admin has no authentication by default, so governance is exposed as an operator-readable control plane.',
        },
        policy: {
          read_only: true,
          unrestricted: false,
          allowlists_active: {
            dcc_types: true,
            skill_names: false,
            skill_families: true,
            tool_slugs: false,
            tool_slug_prefixes: true,
          },
          allowed_dcc_types: ['maya', 'customhost'],
          allowed_skill_names: [],
          allowed_skill_families: ['safe-'],
          allowed_tool_slugs: [],
          allowed_tool_slug_prefixes: ['maya.abcdef01.safe_read'],
        },
        traffic_capture: {
          enabled: true,
          mode: 'high_sensitivity_capture',
          sink_count: 1,
          subscriber_enabled: false,
          sinks: [{ kind: 'jsonl', path: 'G:/capture/traffic.jsonl' }],
          redaction: {
            rule_count: 1,
            paths: ['body.data.params.arguments.api_key'],
          },
          filter: { include: [], exclude: [] },
          production_profile: true,
          force_capture: true,
          production_guardrail: 'forced',
          recent_decisions: [
            {
              timestamp: now,
              request_id: 'req-policy',
              trace_id: 'trace-governance',
              session_id: 'session-1',
              direction: 'inbound',
              leg: 'client_to_gateway',
              transport: 'http',
              http_url: '/mcp',
              mcp_method: 'tools/call',
              outcome: 'captured',
              redacted_paths: ['body.data.params.arguments.api_key'],
              body_size_bytes: 188,
            },
          ],
        },
        middleware: {
          before_count: 3,
          after_count: 1,
          controls: [
            {
              kind: 'quota',
              mode: 'reject',
              summary: 'Limits each session to 60 calls per 60s window.',
              config: {
                limit: 60,
                window_secs: 60,
                bucket_key: 'session_id_or_global',
                active_buckets: 2,
                allowed_total: 12,
                throttled_total: 1,
              },
            },
            {
              kind: 'redaction',
              mode: 'mutate',
              summary: 'Redacts 2 configured field name(s) before dispatch.',
              config: {
                fields: ['api_key', 'token'],
                replacement: '[REDACTED]',
                redacted_total: 4,
              },
            },
          ],
        },
        stats: {
          recent_allowed: 1,
          recent_policy_denied: 1,
          recent_throttled: 1,
          captured_frames: 1,
          skipped_capture_frames: 2,
          redacted_path_count: 1,
          redacted_paths: ['body.data.params.arguments.api_key'],
        },
        recent_decisions: [
          {
            timestamp: now,
            request_id: 'req-policy',
            trace_id: 'trace-governance',
            session_id: 'session-1',
            transport: 'rest',
            agent_id: 'agent-governance',
            agent_name: 'Governance Agent',
            agent_model: 'gpt-test',
            tool: 'maya.abcdef01.unsafe_write',
            dcc_type: 'maya',
            outcome: 'denied',
            success: false,
            reason: 'policy-denied: read-only',
            duration_ms: 12,
            policy: { read_only: true, denied: true, reason: 'read-only' },
            traffic_capture: { frame_count: 1, captured: 1, skipped: 0, reasons: [] },
            privacy: {
              redaction_middleware_active: true,
              redacted_paths: ['body.data.params.arguments.api_key'],
            },
            pressure: { quota_active: true, throttled: false },
          },
          {
            timestamp: '2026-05-18T08:00:02.000Z',
            request_id: 'req-quota',
            trace_id: 'trace-governance',
            session_id: 'session-1',
            transport: 'rest',
            agent_id: 'agent-governance',
            agent_name: 'Governance Agent',
            agent_model: 'gpt-test',
            tool: 'maya.abcdef01.safe_read_scene',
            dcc_type: 'maya',
            outcome: 'throttled',
            success: false,
            reason: 'quota exceeded',
            duration_ms: 2,
            policy: { read_only: false, denied: false, reason: null },
            traffic_capture: { frame_count: 0, captured: 0, skipped: 0, reasons: [] },
            privacy: { redaction_middleware_active: true, redacted_paths: [] },
            pressure: { quota_active: true, throttled: true },
          },
        ],
      };
    } else if (path === '/traffic') {
      body = {
        schema_version: 'dcc-mcp.admin.traffic.v1',
        total: 1,
        capture_status: {
          state: 'captured',
          message: 'Sanitized traffic metadata is retained in the admin live ring.',
          capture_enabled: true,
          live_sink_enabled: true,
          sink_count: 1,
          subscriber_enabled: false,
          retained_frames: 1,
          recent_decision_count: 2,
          captured_decision_count: 1,
          skipped_decision_count: 1,
          skip_reasons: ['filter'],
          redacted_path_count: 1,
          redacted_paths: ['body.data.params.arguments.api_key'],
          safe_to_share: true,
          payload_policy: 'metadata-only',
          retention: { admin_live_configured: true, ring_buffer_capacity: 5000 },
        },
        frames: [
          {
            schema_version: 1,
            name: 'traffic.frame',
            id: 'evt-traffic',
            timestamp_ns: 1779091200000000000,
            source: { service: 'dcc-mcp-gateway' },
            correlation: {
              request_id: 'req-traffic',
              trace_id: 'trace-traffic',
              session_id: 'session-1',
            },
            attributes: {
              capture_id: 'cap_0000000000000001',
              session_id: 'session-1',
              direction: 'inbound',
              leg: 'client_to_gateway',
              transport: 'mcp-http',
              http: {
                method: 'POST',
                url: '/mcp',
                status: 200,
                headers: { 'content-type': 'application/json' },
              },
              mcp: { kind: 'request', method: 'tools/call', id: 'req-traffic' },
              body: {
                encoding: 'json',
                size_bytes: 188,
                redacted_paths: ['body.data.params.arguments.api_key'],
                payload_omitted: true,
                omission_reason: 'admin-traffic-metadata-only',
              },
            },
          },
        ],
        links: {
          admin_traffic_url: '/admin?panel=traffic',
          traffic_api_url: '/admin/api/traffic',
          traffic_export_jsonl_url: '/admin/api/traffic/export',
        },
      };
    } else if (path === '/logs') {
      body = {
        total: 1,
        server_version: '0.19.56',
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
          {
            timestamp: '2026-05-18T08:00:02.000Z',
            level: 'warn',
            source: 'audit',
            message: 'tools/call err 87ms - blender-abcd__render_preview',
            dcc_type: 'blender',
            instance_id: 'blender-abcdef1234',
            request_id: 'req-err',
            tool: 'blender-abcd__render_preview',
            success: false,
            detail: 'backend timeout',
          },
          {
            timestamp: '2026-05-18T08:00:03.000Z',
            level: 'debug',
            source: 'file',
            message: 'dispatch cache hit for search_tools',
            target: 'dcc_mcp_http_server:executor',
            thread: 'ThreadId(01)',
            instance_id: 'local',
          },
        ],
      };
    } else if (path === '/memory') {
      body = {
        enabled: true,
        summary: {
          total: 2,
          by_dcc: { maya: 2 },
          positive: 1,
          negative: 1,
          ok_count: 3,
          fail_count: 1,
          hit_rate_pct: 75,
        },
        memory: [
          {
            id: 1,
            layer: 'longterm',
            key: 'pattern:tool_call:create_cube:ok',
            session_id: 'longterm',
            dcc_name: 'maya',
            score: 3,
            created_unix_secs: 1780367000,
            payload: { tool_name: 'create_cube', ok_count: 3, fail_count: 0 },
          },
          {
            id: 2,
            layer: 'longterm',
            key: 'pattern:tool_call:maya_python__execute:fail',
            session_id: 'longterm',
            dcc_name: 'maya',
            score: -1,
            created_unix_secs: 1780366900,
            payload: { tool_name: 'maya_python__execute', ok_count: 0, fail_count: 1 },
          },
        ],
      };
    } else if (path === '/memory/forget' && method === 'POST') {
      body = { ok: true };
    } else if (path === '/skills') {
      body = {
        total: 2,
        loaded: 2,
        unloaded: 0,
        action_count: 5,
        health: {
          searched_skills: 2,
          used_skills: 1,
          low_adoption_skills: 1,
          load_error_count: 0,
          missing_path_count: 1,
        },
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
            adoption: {
              search_hits: 3,
              best_rank: 2,
              average_rank: 2.4,
              selected_count: 2,
              call_count: 1,
              failure_count: 0,
              load_error_count: 0,
              last_searched: now,
              last_used: now,
              fallback_displaced_by_scripting: 0,
              searched: true,
              used: true,
              low_adoption: false,
            },
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
            adoption: {
              search_hits: 2,
              best_rank: 1,
              average_rank: 1,
              selected_count: 0,
              call_count: 0,
              failure_count: 0,
              load_error_count: 0,
              last_searched: now,
              last_used: null,
              fallback_displaced_by_scripting: 1,
              searched: true,
              used: false,
              low_adoption: true,
            },
          },
        ],
      };
    } else if (path === '/skill-detail') {
      const longToolName = 'maya_modeling__create_high_density_collision_proxy_with_extremely_long_namespace_and_variant_suffix';
      const longSkillPath = 'G:/studio/skills/maya-modeling/very-long-team-folder/shot-asset-pipeline/review/SKILL.md';
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
          skill_md_path: longSkillPath,
          markdown: [
            '---',
            'name: maya-modeling',
            'metadata:',
            '  dcc-mcp:',
            '    dcc: maya',
            '    layer: infrastructure',
            '---',
            '# Maya Modeling',
            '',
            '- Create a polygon sphere',
            '- Inspect `maya_modeling__long_inline_identifier_that_should_wrap_inside_the_panel` before destructive edits',
            '',
            '| Mode | Use | Very long column |',
            '| --- | --- | --- |',
            `| safe | preview | ${longToolName} |`,
            '',
            '```python',
            'import maya.cmds as cmds',
            "cmds.polySphere(name='preview_collision_proxy_with_long_name')",
            '```',
          ].join('\n'),
          tools: [
            { name: longToolName, summary: 'Creates a reviewable collision proxy.', annotations: { readOnlyHint: false, idempotentHint: true }, thread_affinity: 'main' },
            { name: 'delete_sphere', summary: 'Deletes the temporary preview sphere.', annotations: { destructiveHint: true } },
          ],
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
    } else if (path === '/integrations/test' && method === 'POST') {
      const payload = route.request().postDataJSON() as { kind?: string; config?: Record<string, unknown> };
      if (payload.kind === 'wecom') {
        status = 200;
        body = {
          kind: 'wecom',
          status: 'sent',
          message: 'ok',
          sent_at_ms: Date.now(),
          webhook_url: 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=********',
          wecom: { errcode: 0, errmsg: 'ok' },
        };
      } else {
        status = 400;
        body = { error: `Unsupported integration test: ${payload.kind}` };
      }
    } else if (path === '/integrations') {
      const sentryConfig = {
        dsn: 'https://********@o0.ingest.sentry.io/0',
        environment: 'production',
        release: '0.18.0',
        sample_rate: 1.0,
      };
      const wecomConfig = {
        webhook_url: '',
        event_types: [] as string[],
        template: '',
      };
      const webhooksConfig = {
        config_path: '~/dcc-mcp/etc/webhooks.yaml',
        write_config_path: '~/dcc-mcp/etc/webhooks.yaml',
        config_text: 'queue_capacity: 1024\nwebhooks:\n  - name: studio-events\n    url: http://127.0.0.1:9000/dcc-mcp-events\n    events:\n      - tool.failed\n      - gateway.instance.*\n',
      };
      if (method === 'PUT') {
        const payload = route.request().postDataJSON() as { kind?: string; config?: Record<string, unknown> };
        if (payload.kind === 'sentry') {
          // Simulate invalid DSN error
          if (payload.config?.dsn === 'invalid-dsn') {
            status = 400;
            body = { error: 'Invalid DSN format' };
          } else {
            status = 200;
            body = {
              kind: 'sentry',
              label: 'Sentry Error Monitoring',
              description: 'Send panics to Sentry.',
              status: 'pending_restart' as const,
              config: { ...sentryConfig, ...payload.config },
              env_locked_fields: [
                { key: 'dsn', locked: true, env_var: 'DCC_MCP_SENTRY_DSN' },
                { key: 'environment', locked: false, env_var: 'DCC_MCP_SENTRY_ENVIRONMENT' },
                { key: 'release', locked: false, env_var: 'DCC_MCP_SENTRY_RELEASE' },
                { key: 'sample_rate', locked: false, env_var: 'DCC_MCP_SENTRY_SAMPLE_RATE' },
              ],
            };
          }
        } else if (payload.kind === 'wecom') {
          status = 200;
          body = {
            kind: 'wecom',
            label: 'WeCom Message Push',
            description: 'Push selected DCC-MCP events to an Enterprise WeChat group robot.',
            status: 'pending_restart' as const,
            config: { ...wecomConfig, ...payload.config },
            env_locked_fields: [
              { key: 'webhook_url', locked: false, env_var: 'DCC_MCP_WECOM_WEBHOOK_URL' },
              { key: 'event_types', locked: false, env_var: 'DCC_MCP_WECOM_EVENTS' },
              { key: 'template', locked: false, env_var: 'DCC_MCP_WECOM_TEMPLATE' },
            ],
          };
        } else if (payload.kind === 'webhooks') {
          status = 200;
          body = {
            kind: 'webhooks',
            label: 'Event Webhooks',
            description: 'Outbound delivery of EventBus events.',
            status: 'pending_restart' as const,
            config: { ...webhooksConfig, ...payload.config },
            env_locked_fields: [
              { key: 'config_path', locked: false, env_var: 'DCC_MCP_WEBHOOKS_CONFIG' },
            ],
          };
        } else {
          status = 400;
          body = { error: `Unknown integration kind: ${payload.kind}` };
        }
      } else {
        // GET
        body = {
          integrations: [
            {
              kind: 'sentry',
              label: 'Sentry Error Monitoring',
              description: 'Send panics, error events, and span breadcrumbs to Sentry.',
              status: 'active',
              config: sentryConfig,
              env_locked_fields: [
                { key: 'dsn', locked: true, env_var: 'DCC_MCP_SENTRY_DSN' },
                { key: 'environment', locked: false, env_var: 'DCC_MCP_SENTRY_ENVIRONMENT' },
                { key: 'release', locked: false, env_var: 'DCC_MCP_SENTRY_RELEASE' },
                { key: 'sample_rate', locked: false, env_var: 'DCC_MCP_SENTRY_SAMPLE_RATE' },
              ],
            },
            {
              kind: 'webhooks',
              label: 'Event Webhooks',
              description: 'Outbound delivery of EventBus events.',
              status: 'inactive',
              config: webhooksConfig,
              env_locked_fields: [
                { key: 'config_path', locked: false, env_var: 'DCC_MCP_WEBHOOKS_CONFIG' },
              ],
            },
            {
              kind: 'wecom',
              label: 'WeCom Message Push',
              description: 'Push selected DCC-MCP events to an Enterprise WeChat group robot.',
              status: 'inactive',
              config: wecomConfig,
              env_locked_fields: [
                { key: 'webhook_url', locked: false, env_var: 'DCC_MCP_WECOM_WEBHOOK_URL' },
                { key: 'event_types', locked: false, env_var: 'DCC_MCP_WECOM_EVENTS' },
                { key: 'template', locked: false, env_var: 'DCC_MCP_WECOM_TEMPLATE' },
              ],
            },
            {
              kind: 'otlp',
              label: 'OTLP Telemetry',
              description: 'Export distributed traces via gRPC.',
              status: 'inactive',
              config: { endpoint: '', service_name: 'dcc-mcp', headers: '' },
              env_locked_fields: [
                { key: 'endpoint', locked: false, env_var: 'OTEL_EXPORTER_OTLP_ENDPOINT' },
              ],
            },
          ],
        };
      }
    } else if (path === '/sessions') {
      const sessionRows = [
        {
          session_id: 'sess-root-0001',
          parent_session_id: null,
          status: 'active',
          dcc_type: 'maya',
          instance_id: 'maya-inst-1',
          agent_id: 'agent-1',
          agent_name: 'Layout Agent',
          agent_model: 'claude-sonnet-5',
          started_at: '2026-05-18T07:00:00.000Z',
          ended_at: null,
          duration_ms: null,
          turn_count: 12,
          tool_call_count: 34,
          end_reason: null,
          version: '0.17.7',
          actor_id: 'actor-1',
          actor_name: 'alice',
          correlation: { request_id: 'req-root-1', trace_id: 'trace-root-1', workflow_id: null },
        },
        {
          session_id: 'sess-child-0002',
          parent_session_id: 'sess-root-0001',
          status: 'ended',
          dcc_type: 'maya',
          instance_id: 'maya-inst-1',
          agent_id: 'agent-2',
          agent_name: 'Rig Agent',
          agent_model: 'claude-sonnet-5',
          started_at: '2026-05-18T07:05:00.000Z',
          ended_at: '2026-05-18T07:20:00.000Z',
          duration_ms: 900000,
          turn_count: 5,
          tool_call_count: 9,
          end_reason: 'completed',
          version: '0.17.7',
          actor_id: 'actor-1',
          actor_name: 'alice',
          correlation: { request_id: 'req-child-2', trace_id: 'trace-root-1', workflow_id: 'wf-1' },
        },
        {
          session_id: 'sess-crash-0003',
          parent_session_id: null,
          status: 'crashed',
          dcc_type: 'houdini',
          instance_id: 'houdini-inst-1',
          agent_id: 'agent-3',
          agent_name: 'Sim Agent',
          agent_model: 'claude-sonnet-5',
          started_at: '2026-05-18T06:30:00.000Z',
          ended_at: '2026-05-18T06:45:00.000Z',
          duration_ms: 900000,
          turn_count: 3,
          tool_call_count: 4,
          end_reason: 'crash',
          version: '0.17.6',
          actor_id: 'actor-2',
          actor_name: 'bob',
          correlation: { request_id: 'req-crash-3', trace_id: null, workflow_id: null },
        },
      ];

      let filteredSessions = sessionRows;
      const dccTypeFilter = url.searchParams.get('dcc_type');
      const statusFilter = url.searchParams.get('status');
      const searchFilter = url.searchParams.get('search');
      if (dccTypeFilter) filteredSessions = filteredSessions.filter((row) => row.dcc_type === dccTypeFilter);
      if (statusFilter) filteredSessions = filteredSessions.filter((row) => row.status === statusFilter);
      if (searchFilter) {
        const needle = searchFilter.toLowerCase();
        filteredSessions = filteredSessions.filter((row) =>
          row.session_id.toLowerCase().includes(needle)
          || (row.agent_name ?? '').toLowerCase().includes(needle)
          || (row.actor_name ?? '').toLowerCase().includes(needle));
      }

      body = {
        sessions: filteredSessions,
        kpi: {
          total: sessionRows.length,
          active: sessionRows.filter((row) => row.status === 'active').length,
          ended: sessionRows.filter((row) => row.status === 'ended').length,
          crashed: sessionRows.filter((row) => row.status === 'crashed').length,
          by_dcc: { maya: 2, houdini: 1 },
        },
        total: sessionRows.length,
      };
    } else if (path === '/reliability') {
      body = {
        health: {
          status: 'ok',
          uptime_secs: 86400,
          leader: { name: 'gateway-primary', host: '127.0.0.1', port: 9765, version: '0.17.7' },
          candidates: 2,
          limits: {
            body_max_bytes: 10485760,
            rate_limit_per_minute_per_ip: 600,
            circuit_failure_threshold: 5,
            circuit_open_secs: 30,
          },
        },
        circuits: [
          { backend: 'maya-adapter', state: 'closed', failures: 0, last_failure: null, last_success: '2026-05-18T07:59:00.000Z' },
          { backend: 'houdini-adapter', state: 'open', failures: 7, last_failure: '2026-05-18T07:55:00.000Z', last_success: '2026-05-18T06:00:00.000Z' },
        ],
        funnel: { instances: 12, skills: 40, tools: 180, resources: 60 },
        stability_24h: {
          crashes: 0,
          reconnects: 3,
          recoveries: 3,
          success_rate_pct: 99.8,
          avg_latency_ms: 120,
          p95_latency_ms: 340,
        },
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
