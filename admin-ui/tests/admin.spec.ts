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
            finished_at: '2026-05-18T08:00:04.000Z',
            duration_ms: 4000,
            step_count: 4,
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
              request_ids: ['req-search', 'req-describe', 'req-load', 'req-123'],
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
        total: 3,
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
        total_ms: 42,
        ok: true,
        spans: [{ name: 'dispatch', duration_ns: 42000000, ok: true }],
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
        top_agents: [{ name: 'Scene Builder', count: 2 }],
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
    for (const label of ['Connect IDE', 'Debug', 'Activity', 'Health', 'Instances', 'Tools', 'Workflows', 'Tasks', 'Calls', 'Traces', 'Stats', 'Governance', 'Skills', 'Logs', 'Docs']) {
      await expect(page.getByRole('navigation').getByRole('link', { name: label })).toBeVisible();
    }
    await expect(page.getByRole('navigation').getByRole('link', { name: 'Docs' })).toHaveAttribute('href', 'https://github.com/loonghao/dcc-mcp-core/tree/main/docs');
    await expect(page.locator('.setup-panel')).toContainText('Claude Desktop');
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
    await expect(page.locator('.debug-panel')).toContainText('Traffic Shape');
    await page.getByRole('navigation').getByRole('link', { name: 'Health' }).click();
    await expect(page.locator('.health-panel')).toContainText('0.17.7');
    await expect(page.locator('.health-panel')).toContainText('toon / dcc-mcp-byte4-v1');
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
    const callsPanel = page.locator('.calls-panel');
    await expect(callsPanel).toContainText('toon');
    await expect(callsPanel).toContainText('40');
    await expect(callsPanel).toContainText('60 (60.0%)');
    await expect(callsPanel).toContainText('json');
    await expect(callsPanel).toContainText('0 (0.0%)');
    await expect(callsPanel).toContainText('req-legacy');
    await expect(callsPanel.locator('tr', { hasText: 'req-legacy' })).toContainText('-');
    await page.getByRole('button', { name: 'req-123' }).click();
    await expect(page).toHaveURL(/panel=traces/);
    await expect(page).toHaveURL(/trace=req-123/);
    await expect(page.locator('.trace-detail-panel')).toContainText('req-123');
    await expect(page.locator('.trace-detail-panel')).toContainText('dispatch');
    await expect(page.locator('.trace-detail-panel')).toContainText('Token accounting');
    await expect(page.locator('.trace-detail-panel')).toContainText('dcc-mcp-byte4-v1');
    await expect(page.locator('.trace-detail-panel')).toContainText('Returned40');
    await expect(page.locator('.trace-detail-panel')).toContainText('Savings60.0%');
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

  test('shows agent workflows with discovery quality and trace links', async ({ page }) => {
    await page.goto('/admin/?panel=workflows');
    const panel = page.locator('.workflows-panel');
    await expect(panel).toContainText('Scene Builder');
    await expect(panel).toContainText('turn turn-1');
    await expect(panel).toContainText('Create a sphere with the least risky MCP path.');
    await expect(panel).toContainText('reply 220 chars');
    await expect(panel).toContainText('search create sphere');
    await expect(panel).toContainText('describe');
    await expect(panel).toContainText('load_skill');
    await expect(panel).toContainText('best rank 2');
    await expect(panel).toContainText('zero-result');
    await page.getByLabel('Filter current panel').fill('missing tool');
    await expect(page.locator('.workflow-card')).toHaveCount(1);
    await expect(panel).toContainText('zero_results');
    await page.getByLabel('Filter current panel').fill('');
    await panel.getByRole('button', { name: 'Trace' }).last().click();
    await expect(page).toHaveURL(/panel=traces/);
    await expect(page).toHaveURL(/trace=req-123/);
  });

  test('updates stats when the range selector changes', async ({ page }) => {
    await page.goto('/admin/?panel=stats&range=1h');
    await expect(page.locator('.stats-panel')).toBeVisible();
    await expect(page.getByLabel('Range')).toHaveValue('1h');
    await expect(page.locator('.stats-panel')).toContainText('Returned tokens');
    await expect(page.locator('.stats-panel')).toContainText('160');
    await expect(page.locator('.stats-panel')).toContainText('Saved tokens');
    await expect(page.locator('.stats-panel')).toContainText('140');
    await expect(page.locator('.stats-panel')).toContainText('Token savings by transport');
    await expect(page.locator('.stats-panel')).toContainText('rest');
    await expect(page.locator('.stats-panel')).toContainText('json');
    await page.getByLabel('Range').selectOption('7d');
    await expect(page).toHaveURL(/range=7d/);
    await expect(page.locator('.stats-panel')).toContainText('Range 7d');
    await page.getByLabel('Filter current panel').fill('rest');
    await expect(page.locator('.stats-panel')).toContainText('rest');
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
