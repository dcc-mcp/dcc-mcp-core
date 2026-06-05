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
            adapter_version: '0.5.0',
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
            adapter_version: null,
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
        spans: [{ name: 'dispatch', duration_ns: 42000000, ok: true }],
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
    } else if (path === '/integrations') {
      const sentryConfig = {
        dsn: 'https://examplePublicKey@o0.ingest.sentry.io/0',
        environment: 'production',
        release: '0.18.0',
        sample_rate: 1.0,
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
              config: { config_path: '' },
              env_locked_fields: [
                { key: 'config_path', locked: false, env_var: 'DCC_MCP_WEBHOOKS_CONFIG' },
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
    await expect(page.locator('html')).toHaveAttribute('lang', 'en');
    await expect(page.locator('html')).toHaveAttribute('data-admin-locale', 'en');
    await expect(page.getByRole('img', { name: 'DCC MCP' })).toBeVisible();
    await expect(page.locator('.brand-tag')).toContainText('DCC-MCP Gateway');
    await expect(page.locator('h1')).toContainText('Admin Dashboard');
    await expect(page.getByRole('navigation').getByRole('link', { name: 'Connect IDE' })).toHaveClass(/active/);
    for (const label of ['Connect IDE', 'Debug', 'Activity', 'Health', 'Instances', 'Tools', 'Workflows', 'Tasks', 'Calls', 'Traces', 'Stats', 'Governance', 'Skills', 'Integrations', 'Logs', 'Docs']) {
      await expect(page.getByRole('navigation').getByRole('link', { name: label })).toBeVisible();
    }
    await expect(page.getByRole('navigation').getByRole('link', { name: 'Docs' })).toHaveAttribute('href', 'https://github.com/dcc-mcp/dcc-mcp-core/tree/main/docs');
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
          const copy = card.querySelector('.copy-btn')?.getBoundingClientRect();
          const open = card.querySelector('.refresh-btn')?.getBoundingClientRect();
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
      const copy = card.querySelector('.copy-btn')?.getBoundingClientRect();
      const open = card.querySelector('.refresh-btn')?.getBoundingClientRect();
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

  test('normalizes the browser locale onto the document element', async ({ browser }) => {
    const context = await browser.newContext({ locale: 'ja-JP' });
    const page = await context.newPage();
    await mockAdminApi(page);

    await page.goto('/admin/');

    await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
    await expect(page.locator('html')).toHaveAttribute('data-admin-locale-source', 'navigator');
    await expect(page.locator('.brand-tag')).toContainText('DCC-MCP ゲートウェイ');
    await expect(page.getByLabel('言語')).toHaveValue('ja');

    await context.close();
  });

  test('switches language from the visible selector and persists the override', async ({ page }) => {
    await page.goto('/admin/');

    await page.getByLabel('Language').selectOption('zh-CN');

    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
    await expect(page.locator('html')).toHaveAttribute('data-admin-locale-source', 'override');
    await expect(page.getByRole('navigation').getByRole('link', { name: '日志' })).toBeVisible();

    await page.reload();

    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
    await expect(page.getByLabel('语言')).toHaveValue('zh-CN');
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

  test('switches to instances, renders DCC cards, and filters rows', async ({ page }) => {
    await page.goto('/admin/');
    await page.getByRole('navigation').getByRole('link', { name: 'Instances' }).click();
    await expect(page.locator('.instances-panel')).toBeVisible();
    await expect(page.locator('.dcc-icon')).toHaveCount(2);
    await expect(page.locator('.instances-panel')).toContainText('app-type: maya');
    await expect(page.locator('.instances-panel')).toContainText('app-type: blender');
    await expect(page.locator('.instances-panel')).toContainText('Access URL');
    await expect(page.locator('.instances-panel')).toContainText('http://127.0.0.1:8765');
    await expect(page.locator('.instances-panel')).toContainText('Dispatch');
    await expect(page.locator('.instances-panel')).toContainText('ready callable');
    await expect(page.locator('.instances-panel')).toContainText('unavailable not callable');
    await expect(page.locator('.instances-panel')).toContainText('Host RPC');
    await expect(page.locator('.instances-panel')).toContainText('commandport');
    await expect(page.locator('.instances-panel')).toContainText('host-rpc connect failed');
    const mayaCard = page.locator('.instance-card').filter({ hasText: 'Maya Layout' });
    await expect(mayaCard.getByRole('link', { name: 'docs' }).first()).toHaveAttribute('href', 'http://127.0.0.1:8765/docs');
    await page.getByLabel('Filter current panel').fill('blender');
    await expect(page.locator('.instance-card')).toHaveCount(1);
    await expect(page.locator('.instance-card')).toContainText('Blender Lookdev');
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
    const callsPanel = page.locator('.calls-panel');

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
    await expect(tracesPanel).toContainText('slowest upload_texture 5.40 s');

    await tracesPanel.locator('.trace-item', { hasText: 'req-slow' }).click();
    await expect(page.locator('.trace-detail-panel')).toContainText('req-slow');
    await expect(page.locator('.trace-detail-panel')).toContainText('TAIL');
    await expect(page.locator('.span-row.latency-critical')).toContainText('upload_texture');
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
    const panel = page.locator('.traffic-panel');
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
    await page.goto('/admin/?panel=stats&range=1h');
    await expect(page.locator('.stats-panel')).toBeVisible();
    await expect(page.getByLabel('Range')).toHaveValue('1h');
    await expect(page.locator('.stats-panel')).toContainText('Response tokens returned');
    await expect(page.locator('.stats-panel')).toContainText('160');
    await expect(page.locator('.stats-panel')).toContainText('Payload tokens');
    await expect(page.locator('.stats-panel')).toContainText('250');
    await expect(page.locator('.stats-panel')).toContainText('Input / Output tokens');
    const hero = page.locator('.stats-hero');
    await expect(hero).toBeVisible();
    await expect(hero).toContainText('Total tokens');
    await expect(hero).toContainText('Tokens saved');
    await expect(hero).toContainText('Total calls');
    await expect(hero).toContainText('success rate');
    await expect(page.locator('.stats-panel')).toContainText('p99 latency');
    await expect(page.locator('.stats-panel')).toContainText('Slow calls');
    await expect(page.locator('.stats-panel')).toContainText('slowest req-slow 6.20 s; slowest upload_texture 5.40 s');
    await expect(page.locator('.stats-panel')).toContainText('Response tokens saved');
    await expect(page.locator('.stats-panel')).toContainText('140');
    await expect(page.locator('.stats-panel')).toContainText('Top app types');
    await expect(page.locator('.stats-panel')).toContainText('maya');
    await expect(page.locator('.stats-panel')).toContainText('Top actors');
    await expect(page.locator('.stats-panel')).toContainText('Layout Artist');
    await expect(page.locator('.stats-panel')).toContainText('Top client platforms');
    await expect(page.locator('.stats-panel')).toContainText('cursor');
    await expect(page.locator('.stats-panel')).toContainText('Top source IPs');
    await expect(page.locator('.stats-panel')).toContainText('192.0.2.44');
    await expect(page.locator('.stats-panel')).toContainText('Token savings by transport');
    await expect(page.locator('.stats-panel')).toContainText('rest');
    await expect(page.locator('.stats-panel')).toContainText('json');
    await page.getByLabel('Range').selectOption('7d');
    await expect(page).toHaveURL(/range=7d/);
    await expect(page.locator('.stats-panel')).toContainText('7d window');
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
    await expect(page.locator('.skill-paths-panel')).toContainText('Searched / used');
    await expect(page.locator('.skill-paths-panel')).toContainText('best rank 2');
    await expect(page.locator('.skill-paths-panel')).toContainText('1 calls, 0 failures');
    await expect(page.locator('.skill-paths-panel')).toContainText('low adoption');
    await expect(page.locator('.skill-paths-panel')).toContainText('admin_custom #7');
    await expect(page.locator('.skill-paths-panel')).not.toContainText('G:/custom/admin-skills');
    await page.getByLabel('New skill path').fill('G:/new/team-skills');
    await page.getByRole('button', { name: 'Add path' }).click();
    await expect(page.locator('.skill-paths-panel')).toContainText('admin_custom #8');
    await expect(page.locator('.skill-paths-panel')).not.toContainText('G:/new/team-skills');
    await page.getByRole('button', { name: 'Remove' }).first().click();
    await expect(page.locator('.skill-paths-panel')).not.toContainText('admin_custom #7');
  });

  test('opens rendered markdown details for a skill', async ({ page }) => {
    await page.goto('/admin/?panel=skill-paths');
    await page.getByRole('button', { name: 'maya-modeling' }).click();

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
    await page.getByRole('button', { name: 'maya-modeling' }).click();

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
    const themeSelect = page.getByLabel('Theme');
    await expect(themeSelect).toBeVisible();

    await themeSelect.selectOption('dark');
    await expect(page.locator('html')).toHaveClass(/dark/);
    await expect(page.locator('html')).toHaveAttribute('data-admin-theme', 'dark');
    expect(await page.evaluate(() => localStorage.getItem('dcc-mcp-admin-theme'))).toBe('dark');

    await themeSelect.selectOption('light');
    await expect(page.locator('html')).not.toHaveClass(/dark/);
    await expect(page.locator('html')).toHaveAttribute('data-admin-theme', 'light');

    // The persisted choice survives a reload.
    await themeSelect.selectOption('dark');
    await page.reload();
    await expect(page.locator('html')).toHaveClass(/dark/);
    await expect(page.getByLabel('Theme')).toHaveValue('dark');
  });

  test.describe('Integrations panel', () => {
    test('shows three integration cards with empty state for webhooks/otlp', async ({ page }) => {
      await page.goto('/admin/?panel=integrations');
      await expect(page.locator('.integrations-panel')).toBeVisible();
      await expect(page.locator('.integrations-panel h2')).toContainText('Integrations');
      // Three cards
      await expect(page.locator('.integration-card')).toHaveCount(3);
      // Sentry is active
      await expect(page.locator('.integration-card[data-kind="sentry"]')).toContainText('Sentry Error Monitoring');
      await expect(page.locator('.integration-card[data-kind="sentry"] .badge-ok')).toContainText('Active');
      // Webhooks is inactive (placeholder)
      await expect(page.locator('.integration-card[data-kind="webhooks"]')).toContainText('Event Webhooks');
      await expect(page.locator('.integration-card[data-kind="webhooks"] .badge-muted')).toContainText('Inactive');
      // OTLP is inactive (placeholder)
      await expect(page.locator('.integration-card[data-kind="otlp"]')).toContainText('OTLP Telemetry');
      await expect(page.locator('.integration-card[data-kind="otlp"] .badge-muted')).toContainText('Inactive');
    });

    test('shows env-locked Sentry with masked DSN', async ({ page }) => {
      await page.goto('/admin/?panel=integrations');
      // Click edit on Sentry
      await page.locator('.integration-card[data-kind="sentry"] button').click();
      // Edit form visible
      await expect(page.locator('.integration-edit-form')).toBeVisible();
      // DSN field is env-locked
      const dsnField = page.locator('.integration-edit-field.env-locked').first();
      await expect(dsnField).toBeVisible();
      await expect(dsnField.locator('input')).toBeDisabled();
      // Shows env var hint
      await expect(page.locator('.integration-env-hint code')).toContainText('DCC_MCP_SENTRY_DSN');
      // Other fields are editable
      await expect(page.locator('#integration-sentry-environment')).toBeEnabled();
      await expect(page.locator('#integration-sentry-release')).toBeEnabled();
      await expect(page.locator('#integration-sentry-sample_rate')).toBeEnabled();
    });

    test('save shows pending_restart badge', async ({ page }) => {
      await page.goto('/admin/?panel=integrations');
      // Click edit on Sentry
      await page.locator('.integration-card[data-kind="sentry"] button').click();
      await expect(page.locator('.integration-edit-form')).toBeVisible();
      // Change environment
      await page.locator('#integration-sentry-environment').fill('staging');
      // Save
      await page.locator('.integration-edit-actions button[type="submit"]').click();
      // Wait for edit form to close
      await expect(page.locator('.integration-edit-form')).not.toBeVisible({ timeout: 5000 });
      // After save, the panel should re-render with pending_restart
      await expect(page.locator('.integration-card[data-kind="sentry"].pending-restart')).toBeVisible({ timeout: 5000 });
      await expect(page.locator('.integration-card[data-kind="sentry"] .integration-card-head .badge-warn')).toContainText('Pending Restart');
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
      // Click edit on Sentry
      await page.locator('.integration-card[data-kind="sentry"] button').click();
      await expect(page.locator('.integration-edit-form')).toBeVisible();
      // Enter invalid DSN (doesn't start with 'http')
      await page.locator('#integration-sentry-dsn').fill('bad-dsn-string');
      // Submit
      await page.locator('.integration-edit-actions button[type="submit"]').click();
      // Should see field-level error for invalid DSN
      await expect(page.locator('.integration-field-error')).toContainText(/Invalid DSN|error/i, { timeout: 5000 });
    });
  });
});
