/// Vite dev-server mock middleware.
///
/// Returns canned fixture data on `/admin/api/*` and `/api/*` so the admin UI
/// renders end-to-end at `npm run dev` without a live gateway. The
/// plugin is only registered from `vite.config.ts` when the dev server
/// boots, so the production single-file bundle is unaffected.

import type { Plugin } from 'vite';
import type { IncomingMessage, ServerResponse } from 'node:http';

const NOW = new Date().toISOString();
const DEV_WECOM_TEST_TIMEOUT_MS = 10_000;
const WECOM_WEBHOOK_HOST = 'qyapi.weixin.qq.com';
const WECOM_WEBHOOK_PATH = '/cgi-bin/webhook/send';
const WECOM_WEBHOOK_URL_HINT = 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=...';

const HEALTH = {
  status: 'ok',
  instances_ready: 3,
  instances_total: 4,
  uptime_secs: 18342,
  version: '0.17.38-dev',
  response_format: { default: 'toon' },
  searched_skills: 5,
  used_skills: 3,
  low_adoption_skills: 1,
  load_error_count: 0,
};

type BrandingMock = { accent_color?: string; emoji?: string; tagline?: string };
type LinksMock = { docs?: string; repo?: string; homepage?: string; issues?: string };

function makeSkill(name: string, dcc: string, summary: string, opts: Partial<{
  loaded: boolean;
  actions: number;
  tools: string[];
  used: boolean;
  lowAdoption: boolean;
  failures: number;
  instances: number;
  branding: BrandingMock;
  links: LinksMock;
  example_prompts: string[];
}> = {}) {
  const loaded = opts.loaded ?? true;
  const id8 = name.padEnd(8, '0').slice(0, 8);
  const fullId = `${id8}-aaaa-bbbb-cccc-${'0'.repeat(12)}`;
  return {
    name,
    dcc_type: dcc,
    loaded,
    action_count: opts.actions ?? (opts.tools?.length ?? 3),
    instance_count: opts.instances ?? 1,
    instances: [id8],
    instance_ids: [fullId],
    instance_details: [{ id: fullId, prefix: id8, dcc_type: dcc }],
    tools: opts.tools ?? ['operate', 'inspect', 'render'],
    summary,
    branding: opts.branding ?? null,
    links: opts.links ?? null,
    example_prompts: opts.example_prompts ?? [],
    adoption: {
      search_hits: 4,
      best_rank: opts.used ? 1 : 7,
      average_rank: 3.2,
      selected_count: opts.used ? 3 : 0,
      call_count: opts.used ? 12 : 0,
      failure_count: opts.failures ?? 0,
      load_error_count: 0,
      last_searched: NOW,
      last_used: opts.used ? NOW : null,
      fallback_displaced_by_scripting: 0,
      searched: true,
      used: !!opts.used,
      low_adoption: !!opts.lowAdoption,
    },
  };
}

const SKILLS_PAYLOAD = {
  total: 7,
  loaded: 6,
  unloaded: 1,
  action_count: 21,
  health: {
    searched_skills: 5,
    used_skills: 3,
    low_adoption_skills: 1,
    load_error_count: 0,
    missing_path_count: 0,
  },
  skills: [
    makeSkill('maya-modeling', 'maya', 'Polygon modeling primitives for Autodesk Maya — bevel, extrude, retopo, mirror, and bridge edges.', {
      tools: ['create_sphere', 'extrude_edges', 'bevel', 'mirror_geometry'],
      used: true,
      actions: 4,
      branding: { accent_color: '#ff7a45', emoji: '🐉', tagline: 'High-impact bevel and retopo flow' },
      links: { docs: 'https://example.com/skills/maya-modeling', repo: 'https://github.com/example/maya-modeling' },
      example_prompts: [
        'Bevel the selected polygon edges with mitred corners',
        'Retopologise the hi-poly mesh down to a quad cage',
        'Mirror geometry across the world X-axis',
      ],
    }),
    makeSkill('blender-lookdev', 'blender', 'Material assignment, environment lighting and beauty preview rendering for Blender Cycles.', {
      tools: ['assign_material', 'set_envlight', 'render_preview'],
      used: true,
      actions: 3,
      branding: { accent_color: '#f5792a', emoji: '🎨', tagline: 'Lookdev fast-path for Cycles' },
      links: { docs: 'https://example.com/skills/blender-lookdev', homepage: 'https://example.com' },
      example_prompts: [
        'Assign a procedural metal material to the selected object',
        'Render a beauty preview at 720p with denoising',
      ],
    }),
    makeSkill('houdini-render', 'houdini', 'Karma / Mantra render submission, AOV control, and crop-region overrides.', {
      tools: ['submit_render', 'aov_setup', 'crop_window'],
      lowAdoption: true,
      actions: 3,
      branding: { accent_color: '#ff8f00', tagline: 'Submit Karma jobs without leaving the agent' },
      links: { docs: 'https://example.com/skills/houdini-render', issues: 'https://example.com/issues' },
      example_prompts: ['Submit a Karma render for shot 010_010 at full quality'],
    }),
    makeSkill('photoshop-comp', 'photoshop', 'Multi-pass compositing, smart object linking, and export bundles for downstream review pipelines.', { tools: ['merge_passes', 'export_review', 'link_smart_object'], used: true, actions: 3 }),
    makeSkill('unity-scene', 'unity', 'Scene graph inspection and asset import diagnostics for Unity Editor projects.', { tools: ['list_objects', 'check_imports'], actions: 2 }),
    makeSkill('unreal-bp', 'unreal', 'Blueprint scaffolding and node connection helpers for Unreal Engine 5.', { tools: ['create_blueprint', 'connect_nodes', 'rebuild_lighting'], failures: 1, actions: 3 }),
    makeSkill('dcc-diagnostics', 'maya', 'Infrastructure-layer diagnostics — screenshot, audit log dump, environment probe.', { loaded: false, tools: ['screenshot', 'audit_log'], actions: 2 }),
  ],
};

const SKILL_PATHS_PAYLOAD = {
  paths: [
    { source: 'env:DCC_MCP_SKILL_PATHS', path: 'G:/studio/skills', status: 'present', exists: true },
    { id: 7, source: 'admin_custom', path: 'G:/custom/admin-skills', status: 'present', exists: true },
    { id: 9, source: 'admin_custom', path: 'D:/old/skills', status: 'missing', exists: false },
  ],
};

const WORKERS_PAYLOAD = {
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
      instance_id: 'houdini-abcdef1234',
      display_name: 'Houdini FX',
      dcc_type: 'houdini',
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

const INSTANCES_PAYLOAD = {
  total: WORKERS_PAYLOAD.workers.length,
  live: WORKERS_PAYLOAD.summary.live,
  stale: WORKERS_PAYLOAD.summary.stale,
  unhealthy: WORKERS_PAYLOAD.summary.unhealthy,
  view: 'live',
  pruned_dead: 0,
  instances: WORKERS_PAYLOAD.workers,
};

const INTEGRATIONS_PAYLOAD = {
  integrations: [
    {
      kind: 'sentry',
      label: 'Sentry Error Monitoring',
      description: 'Send panics, error events, and span breadcrumbs to Sentry.',
      status: 'active',
      config: {
        dsn: 'https://********@o0.ingest.sentry.io/0',
        environment: 'production',
        release: '0.18.0',
        sample_rate: 1.0,
        write_config_path: '~/dcc-mcp/etc/sentry.json',
      },
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
      description: 'Outbound delivery of EventBus events to HTTP endpoints.',
      status: 'inactive',
      config: {
        config_path: '',
        write_config_path: '~/dcc-mcp/etc/webhooks.yaml',
      },
      env_locked_fields: [
        { key: 'config_path', locked: false, env_var: 'DCC_MCP_WEBHOOKS_CONFIG' },
      ],
    },
    {
      kind: 'wecom',
      label: 'WeCom Message Push',
      description: 'Push selected DCC-MCP events to an Enterprise WeChat group robot.',
      status: 'inactive',
      config: {
        webhook_url: '',
        event_types: [],
        template: '',
        write_config_path: '~/dcc-mcp/etc/webhooks.yaml',
      },
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
      config: {
        endpoint: '',
        service_name: 'dcc-mcp',
        headers: '',
        write_config_path: '~/dcc-mcp/etc/otlp.json',
      },
      env_locked_fields: [
        { key: 'endpoint', locked: false, env_var: 'OTEL_EXPORTER_OTLP_ENDPOINT' },
      ],
    },
  ],
};

let devWecomWebhookUrl: string | null = null;

const GOVERNANCE_PAYLOAD = {
  schema_version: 'dcc-mcp.admin.governance.v1',
  generated_at: NOW,
  mode: {
    admin_mutations: 'disabled',
    reason: 'Development mock governance state.',
  },
  policy: {
    read_only: false,
    unrestricted: true,
    allowlists_active: {
      dcc: false,
      skills: false,
      tools: false,
    },
    allowed_dcc_types: [],
    allowed_skill_names: [],
    allowed_skill_families: [],
    allowed_tool_slugs: [],
    allowed_tool_slug_prefixes: [],
  },
  traffic_capture: {
    enabled: false,
    mode: 'aggregate_only',
    sink_count: 0,
    subscriber_enabled: false,
    sinks: [],
    redaction: {
      rule_count: 2,
      paths: ['headers.authorization', 'body.api_key'],
    },
    filter: {
      include: [],
      exclude: [],
    },
    production_profile: true,
    force_capture: false,
    production_guardrail: 'safe_aggregate_only',
    recent_decisions: [],
  },
  middleware: {
    before_count: 2,
    after_count: 1,
    controls: [
      {
        kind: 'redaction',
        mode: 'enabled',
        summary: 'Redacts credentials from retained trace and traffic payloads.',
        config: { fields: ['authorization', 'api_key'] },
      },
      {
        kind: 'quota',
        mode: 'observe',
        summary: 'Tracks per-session pressure without blocking in the dev mock.',
        config: { calls_per_window: 120, window_secs: 60 },
      },
    ],
  },
  stats: {
    recent_allowed: 18,
    recent_policy_denied: 0,
    recent_throttled: 0,
    captured_frames: 0,
    skipped_capture_frames: 18,
    redacted_path_count: 2,
    redacted_paths: ['headers.authorization', 'body.api_key'],
  },
  recent_decisions: [
    {
      timestamp: NOW,
      request_id: 'mock-governance-001',
      trace_id: 'trace-mock-governance',
      session_id: 'session-dev',
      transport: 'rest',
      agent_name: 'Dev Admin UI',
      client_platform: 'vite',
      source_ip: '127.0.0.1',
      tool: 'maya_modeling__create_sphere',
      dcc_type: 'maya',
      outcome: 'allowed',
      success: true,
      reason: 'mock policy allowed',
      duration_ms: 24,
      policy: { read_only: false, denied: false, reason: 'unrestricted' },
      traffic_capture: { frame_count: 0, captured: 0, skipped: 1, reasons: ['aggregate_only'] },
      privacy: { redaction_middleware_active: true, redacted_paths: ['headers.authorization'] },
      pressure: { quota_active: true, throttled: false },
    },
  ],
};

const ACTIVITY_PAYLOAD = {
  total: 2,
  events: [
    {
      event_id: 'audit:req-123',
      timestamp: NOW,
      kind: 'tool_call',
      severity: 'info',
      status: 'ok',
      message: 'tools/call maya-dev__create_sphere',
      tool: 'maya-dev__create_sphere',
      duration_ms: 48,
      correlation: {
        request_id: 'req-123',
        session_id: 'session-dev',
        instance_id: 'maya-1234567890',
        dcc_type: 'maya',
        actor_name: 'Dev Admin UI',
        client_platform: 'vite',
        source_ip: '127.0.0.1',
      },
    },
    {
      event_id: 'gateway:ready',
      timestamp: NOW,
      kind: 'gateway_event',
      severity: 'info',
      status: 'ok',
      message: 'development gateway mock is serving admin API JSON',
      correlation: { instance_id: 'local-dev-gateway', dcc_type: 'gateway' },
    },
  ],
};

const TOOLS_PAYLOAD = {
  total: 2,
  tools: [
    {
      slug: 'maya-dev__create_sphere',
      dcc_type: 'maya',
      summary: 'Create a polygon sphere.',
      skill_name: 'maya-modeling',
      name: 'create_sphere',
      instance_id: 'maya-1234567890',
      instance_prefix: 'maya-123',
    },
    {
      slug: 'houdini-dev__inspect_scene',
      dcc_type: 'houdini',
      summary: 'Inspect scene state and report readiness.',
      skill_name: 'houdini-render',
      name: 'inspect_scene',
      instance_id: 'houdini-abcdef1234',
      instance_prefix: 'houdini-',
    },
  ],
};

const CALLS = [
  {
    timestamp: NOW,
    request_id: 'req-123',
    tool: 'maya-dev__create_sphere',
    dcc_type: 'maya',
    status: 'ok',
    success: true,
    error: null,
    duration_ms: 48,
    instance_id: 'maya-1234567890',
    transport: 'rest',
    actor_name: 'Dev Admin UI',
    client_platform: 'vite',
    source_ip: '127.0.0.1',
    token_accounting: {
      response_format: 'toon',
      token_estimator: 'dcc-mcp-byte4-v1',
      original_bytes: 320,
      returned_bytes: 144,
      original_tokens: 80,
      returned_tokens: 36,
      saved_tokens: 44,
      savings_pct: 55,
    },
  },
  {
    timestamp: NOW,
    request_id: 'req-slow',
    tool: 'houdini-dev__render_preview',
    dcc_type: 'houdini',
    status: 'failed',
    success: false,
    error: 'host-rpc connect failed',
    duration_ms: 3200,
    instance_id: 'houdini-abcdef1234',
    transport: 'rest',
  },
];

const TRACES = [
  {
    timestamp: NOW,
    request_id: 'req-123',
    tool: 'maya-dev__create_sphere',
    dcc_type: 'maya',
    status: 'ok',
    success: true,
    total_ms: 48,
    instance_id: 'maya-1234567890',
    span_count: 3,
    input_tokens: 24,
    output_tokens: 16,
    total_tokens: 40,
    payload_token_estimator: 'dcc-mcp-byte4-v1',
    slowest_span_name: 'backend.dispatch',
    slowest_span_ms: 36,
    token_accounting: CALLS[0].token_accounting,
  },
  {
    timestamp: NOW,
    request_id: 'req-slow',
    tool: 'houdini-dev__render_preview',
    dcc_type: 'houdini',
    status: 'failed',
    success: false,
    total_ms: 3200,
    instance_id: 'houdini-abcdef1234',
    span_count: 2,
    slowest_span_name: 'host_rpc.connect',
    slowest_span_ms: 3000,
  },
];

const TRACE_DETAIL = {
  request_id: 'req-123',
  method: 'tools/call',
  tool_slug: 'maya-dev__create_sphere',
  dcc_type: 'maya',
  transport: 'rest',
  started_at: NOW,
  total_ms: 48,
  ok: true,
  spans: [
    { name: 'gateway.received', duration_ns: 2_000_000, ok: true },
    { name: 'gateway.route', duration_ns: 10_000_000, ok: true },
    { name: 'backend.dispatch', duration_ns: 36_000_000, ok: true },
  ],
  agent_context: {
    agent_name: 'Dev Admin UI',
    client_platform: 'vite',
    session_id: 'session-dev',
    source_ip: '127.0.0.1',
  },
  input: {
    content: '{"radius":1}',
    mime_type: 'application/json',
    truncated: false,
    original_size: 12,
    estimated_tokens: 3,
  },
  output: {
    content: '{"ok":true,"object":"pSphere1"}',
    mime_type: 'application/json',
    truncated: false,
    original_size: 31,
    estimated_tokens: 8,
  },
  input_tokens: 3,
  output_tokens: 8,
  total_tokens: 11,
  payload_token_estimator: 'dcc-mcp-byte4-v1',
  token_accounting: CALLS[0].token_accounting,
};

const STATS_PAYLOAD = {
  range: '24h',
  total_calls: CALLS.length,
  successful_calls: 1,
  failed_calls: 1,
  success_rate: 50,
  latency_ms: { p50_ms: 48, p95_ms: 3200, p99_ms: 3200 },
  total_input_tokens: 24,
  total_output_tokens: 16,
  total_tokens: 40,
  avg_tokens_per_call: 20,
  payload_token_estimator: 'dcc-mcp-byte4-v1',
  top_app_types: [{ name: 'maya', count: 1 }, { name: 'houdini', count: 1 }],
  top_tools: [{ name: 'maya-dev__create_sphere', count: 1 }],
  top_instances: [{ name: 'maya-1234567890', count: 1 }],
  top_agents: [{ name: 'Dev Admin UI', count: 1 }],
  token_usage: {
    total_original_tokens: 80,
    total_returned_tokens: 36,
    total_saved_tokens: 44,
    average_savings_pct: 55,
    by_tool: [{ name: 'maya-dev__create_sphere', calls: 1, returned_tokens: 36, saved_tokens: 44, savings_pct: 55 }],
    by_transport: [{ name: 'rest', calls: 2, returned_tokens: 36, saved_tokens: 44, savings_pct: 55 }],
    by_response_format: [{ name: 'toon', calls: 1, returned_tokens: 36, saved_tokens: 44, savings_pct: 55 }],
  },
  hourly_distribution: Array.from({ length: 24 }, (_, hour) => (hour === 10 ? 2 : 0)),
  governance: {
    recent_allowed: 1,
    recent_policy_denied: 0,
    recent_throttled: 0,
    captured_frames: 1,
    skipped_capture_frames: 0,
    redacted_path_count: 1,
    redacted_paths: ['headers.authorization'],
  },
};

const TASKS_PAYLOAD = {
  total: 1,
  tasks: [
    {
      task_id: 'session-dev:turn-1',
      task_type: 'agent_turn',
      status: 'completed',
      title: 'Create a sphere through the gateway mock.',
      goal: 'Exercise the admin UI dev mock with real JSON.',
      summary: 'Search, describe, and call flow completed in the development mock.',
      final_result: 'Created pSphere1 and captured a trace.',
      started_at: NOW,
      finished_at: NOW,
      duration_ms: 48,
      app_types: ['maya'],
      related: {
        workflow_ids: ['workflow-dev'],
        request_ids: ['req-123'],
        trace_ids: ['req-123'],
        session_ids: ['session-dev'],
      },
      correlation: {
        request_id: 'req-123',
        workflow_id: 'workflow-dev',
        session_id: 'session-dev',
        instance_id: 'maya-1234567890',
        dcc_type: 'maya',
        actor_name: 'Dev Admin UI',
        client_platform: 'vite',
      },
    },
  ],
};

const WORKFLOWS_PAYLOAD = {
  total: 1,
  summary: { failed: 0, warning: 0, zero_result_workflows: 0 },
  workflows: [
    {
      workflow_id: 'workflow-dev',
      group_kind: 'session',
      title: 'Dev Admin UI: maya-dev__create_sphere',
      status: 'completed',
      started_at: NOW,
      finished_at: NOW,
      duration_ms: 48,
      step_count: 3,
      failed_steps: 0,
      agent: {
        agent_name: 'Dev Admin UI',
        model: 'mock',
        session_id: 'session-dev',
        turn_id: 'turn-1',
        task: 'Create a sphere through the gateway mock.',
        user_intent_summary: 'Verify admin API routes in development.',
        agent_reply_summary: 'The dev mock returned structured JSON.',
      },
      correlation: {
        session_id: 'session-dev',
        trace_id: 'req-123',
        request_ids: ['req-search', 'req-describe', 'req-123'],
        trace_ids: ['req-123'],
        session_ids: ['session-dev'],
      },
      discovery: {
        search_count: 1,
        zero_result_count: 0,
        selected_count: 1,
        best_selected_rank: 1,
        time_to_first_success_ms: 48,
        search_ids: ['search-dev'],
      },
      steps: [
        { step_id: 'search-dev', kind: 'search', title: 'search create sphere', timestamp: NOW, status: 'ok', success: true, request_id: 'req-search', dcc_type: 'maya', transport: 'rest' },
        { step_id: 'describe-dev', kind: 'describe', title: 'maya-dev__create_sphere', timestamp: NOW, status: 'ok', success: true, request_id: 'req-describe', dcc_type: 'maya', transport: 'rest' },
        { step_id: 'call-dev', kind: 'call', title: 'maya-dev__create_sphere', timestamp: NOW, status: 'ok', success: true, request_id: 'req-123', trace_id: 'req-123', dcc_type: 'maya', instance_id: 'maya-1234567890', transport: 'rest', duration_ms: 48 },
      ],
    },
  ],
};

const TRAFFIC_PAYLOAD = {
  schema_version: 'dcc-mcp.admin.traffic.v1',
  total: 1,
  capture_status: {
    state: 'captured',
    message: 'Development mock captured one gateway request.',
    capture_enabled: true,
    live_sink_enabled: true,
    sink_count: 1,
    retained_frames: 1,
    recent_decision_count: 1,
    captured_decision_count: 1,
    skipped_decision_count: 0,
    redacted_path_count: 1,
    redacted_paths: ['headers.authorization'],
    safe_to_share: true,
  },
  frames: [
    {
      schema_version: 1,
      name: 'traffic.frame',
      id: 'frame-dev-1',
      source: { component: 'admin-dev-mock' },
      correlation: { request_id: 'req-123', trace_id: 'req-123', session_id: 'session-dev' },
      attributes: {
        direction: 'inbound',
        leg: 'client_to_gateway',
        transport: 'http',
        http: { method: 'POST', url: '/mcp', status: 200 },
        mcp: { kind: 'request', method: 'tools/call', id: '1' },
        body: {
          encoding: 'json',
          size_bytes: 120,
          data: { method: 'tools/call', params: { name: 'maya-dev__create_sphere' } },
          redacted_paths: ['headers.authorization'],
        },
      },
    },
  ],
  links: {
    admin_traffic_url: '/admin?panel=traffic',
    traffic_api_url: '/admin/api/traffic',
    traffic_export_jsonl_url: '/admin/api/traffic/export?limit=1000',
  },
};

const LOGS_PAYLOAD = {
  logs: [
    {
      timestamp: NOW,
      level: 'info',
      source: 'admin-dev-mock',
      message: 'Served structured admin API JSON from Vite middleware.',
      request_id: 'req-123',
      dcc_type: 'gateway',
    },
  ],
};

function dateDaysAgo(days: number): string {
  const date = new Date(NOW);
  date.setDate(date.getDate() - days);
  return date.toISOString().slice(0, 10);
}

const ANALYTICS_TIMESERIES = [
  { daysAgo: 350, calls: 3, failures: 0, tokens_input: 420, tokens_output: 260, avg_duration_ms: '72' },
  { daysAgo: 292, calls: 5, failures: 0, tokens_input: 680, tokens_output: 410, avg_duration_ms: '88' },
  { daysAgo: 231, calls: 2, failures: 0, tokens_input: 310, tokens_output: 220, avg_duration_ms: '64' },
  { daysAgo: 128, calls: 4, failures: 0, tokens_input: 520, tokens_output: 380, avg_duration_ms: '96' },
  { daysAgo: 96, calls: 7, failures: 1, tokens_input: 1040, tokens_output: 780, avg_duration_ms: '180' },
  { daysAgo: 64, calls: 3, failures: 0, tokens_input: 460, tokens_output: 350, avg_duration_ms: '81' },
  { daysAgo: 45, calls: 6, failures: 0, tokens_input: 760, tokens_output: 610, avg_duration_ms: '92' },
  { daysAgo: 35, calls: 9, failures: 1, tokens_input: 1180, tokens_output: 920, avg_duration_ms: '210' },
  { daysAgo: 28, calls: 11, failures: 0, tokens_input: 1460, tokens_output: 990, avg_duration_ms: '110' },
  { daysAgo: 21, calls: 8, failures: 0, tokens_input: 1120, tokens_output: 860, avg_duration_ms: '104' },
  { daysAgo: 14, calls: 14, failures: 1, tokens_input: 1820, tokens_output: 1410, avg_duration_ms: '260' },
  { daysAgo: 12, calls: 7, failures: 0, tokens_input: 910, tokens_output: 740, avg_duration_ms: '99' },
  { daysAgo: 9, calls: 16, failures: 0, tokens_input: 2150, tokens_output: 1620, avg_duration_ms: '118' },
  { daysAgo: 7, calls: 19, failures: 1, tokens_input: 2580, tokens_output: 1860, avg_duration_ms: '320' },
  { daysAgo: 6, calls: 13, failures: 0, tokens_input: 1710, tokens_output: 1240, avg_duration_ms: '140' },
  { daysAgo: 5, calls: 22, failures: 0, tokens_input: 2940, tokens_output: 2140, avg_duration_ms: '132' },
  { daysAgo: 3, calls: 18, failures: 0, tokens_input: 2360, tokens_output: 1810, avg_duration_ms: '125' },
  { daysAgo: 2, calls: 24, failures: 1, tokens_input: 3180, tokens_output: 2290, avg_duration_ms: '360' },
  { daysAgo: 1, calls: 17, failures: 0, tokens_input: 2240, tokens_output: 1710, avg_duration_ms: '121' },
  { daysAgo: 0, calls: 20, failures: 1, tokens_input: 2680, tokens_output: 1960, avg_duration_ms: '280' },
].map(({ daysAgo, ...point }) => ({
  date: dateDaysAgo(daysAgo),
  ...point,
}));

const ANALYTICS_TOTALS = ANALYTICS_TIMESERIES.reduce(
  (totals, point) => ({
    calls: totals.calls + point.calls,
    failures: totals.failures + point.failures,
    tokensInput: totals.tokensInput + point.tokens_input,
    tokensOutput: totals.tokensOutput + point.tokens_output,
  }),
  { calls: 0, failures: 0, tokensInput: 0, tokensOutput: 0 },
);

const ANALYTICS_OVERVIEW_PAYLOAD = {
  range: '30d',
  period_start: NOW,
  period_end: NOW,
  kpi: {
    calls_total: ANALYTICS_TOTALS.calls,
    calls_failed: ANALYTICS_TOTALS.failures,
    failure_rate_pct: ((ANALYTICS_TOTALS.failures / ANALYTICS_TOTALS.calls) * 100).toFixed(1),
    success_rate_pct: (((ANALYTICS_TOTALS.calls - ANALYTICS_TOTALS.failures) / ANALYTICS_TOTALS.calls) * 100).toFixed(1),
    tokens_input_total: ANALYTICS_TOTALS.tokensInput,
    tokens_output_total: ANALYTICS_TOTALS.tokensOutput,
    tokens_response_saved: 8240,
    tokens_total: ANALYTICS_TOTALS.tokensInput + ANALYTICS_TOTALS.tokensOutput,
    llm_tokens_total: 9320,
    avg_duration_ms: '147',
    avg_tokens_per_call: Math.round((ANALYTICS_TOTALS.tokensInput + ANALYTICS_TOTALS.tokensOutput) / ANALYTICS_TOTALS.calls).toString(),
    unique_instances: 2,
    unique_agents: 1,
  },
  top_tools: [
    { name: 'maya-dev__create_sphere', calls: 1, failures: 0, success_rate_pct: 100, avg_duration_ms: 48 },
    { name: 'houdini-dev__render_preview', calls: 1, failures: 1, success_rate_pct: 0, avg_duration_ms: 3200 },
  ],
  daily_series: [
    ...ANALYTICS_TIMESERIES.map((point) => ({ ...point, dcc_type: point.failures > 0 ? 'houdini' : 'maya' })),
  ],
};

const ANALYTICS_TIMESERIES_PAYLOAD = {
  series: ANALYTICS_TIMESERIES,
};

const ANALYTICS_HEATMAP_PAYLOAD = {
  heatmap: ANALYTICS_TIMESERIES.slice(-14).map((point, index) => {
    const date = new Date(`${point.date}T12:00:00`);
    return {
      weekday: date.getDay(),
      hour: [9, 10, 11, 14, 15, 16, 20][index % 7],
      calls: point.calls,
      failures: point.failures,
      avg_duration_ms: Number(point.avg_duration_ms),
      tokens_total: point.tokens_input + point.tokens_output,
    };
  }),
};

const MARKETPLACE_CATALOG = [
  {
    name: 'maya-modeling',
    description: 'Production modeling primitives, bevel workflows, and retopo helpers for Maya.',
    dcc: ['maya'],
    tags: ['modeling', 'mesh', 'retopo'],
    version: '2.1.0',
    min_core_version: '0.17.0',
    maintainer: 'dcc-mcp',
    source_name: 'official',
    url: 'https://github.com/dcc-mcp/marketplace/tree/main/maya-modeling',
    install: { type: 'git', url: 'https://github.com/dcc-mcp/marketplace.git' },
  },
  {
    name: 'blender-lookdev',
    description: 'Material assignment, environment lighting, and Cycles preview rendering for Blender.',
    dcc: ['blender'],
    tags: ['lookdev', 'materials', 'render'],
    version: '0.8.0',
    min_core_version: '0.17.0',
    maintainer: 'dcc-mcp',
    source_name: 'official',
    url: 'https://github.com/dcc-mcp/marketplace/tree/main/blender-lookdev',
    install: { type: 'git', url: 'https://github.com/dcc-mcp/marketplace.git' },
  },
  {
    name: 'cross-dcc-utils',
    description: 'Shared diagnostics, scene inspection, and safe utility commands for multiple DCC hosts.',
    dcc: ['maya', 'blender', 'houdini'],
    tags: ['diagnostics', 'inspection', 'utilities'],
    version: '1.3.0',
    min_core_version: '0.17.0',
    maintainer: 'dcc-mcp',
    source_name: 'official',
    url: 'https://github.com/dcc-mcp/marketplace/tree/main/cross-dcc-utils',
    install: { type: 'git', url: 'https://github.com/dcc-mcp/marketplace.git' },
  },
];

let marketplaceInstalled = [
  {
    name: 'cross-dcc-utils',
    dcc: 'maya',
    version: '1.2.0',
    install_type: 'git',
    path: 'C:/Users/demo/.dcc-mcp/maya/skills/cross-dcc-utils',
    source_name: 'official',
    source_url: 'https://github.com/dcc-mcp/marketplace',
    install_url: 'https://github.com/dcc-mcp/marketplace.git',
    install_ref: null,
    installed_at_ms: Date.now() - 86_400_000,
  },
];

let marketplaceSources = [
  { name: 'official', url: 'https://github.com/dcc-mcp/marketplace', origin: 'builtin' },
];

let marketplaceOutdated = [
  {
    name: 'cross-dcc-utils',
    dcc: 'maya',
    installed_version: '1.2.0',
    latest_version: '1.3.0',
    source_name: 'official',
    source_url: 'https://github.com/dcc-mcp/marketplace',
    install_type: 'git',
    install_url: 'https://github.com/dcc-mcp/marketplace.git',
    install_ref: null,
    path: 'C:/Users/demo/.dcc-mcp/maya/skills/cross-dcc-utils',
  },
];

async function readJsonBody(req: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }
  const text = Buffer.concat(chunks).toString('utf8').trim();
  if (!text) return {};
  try {
    const parsed = JSON.parse(text);
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : {};
  } catch {
    return {};
  }
}

function stringRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function concreteWebhookUrlFromConfig(config: Record<string, unknown>): string | null {
  const raw = typeof config.webhook_url === 'string' ? config.webhook_url.trim() : '';
  if (!raw || raw.includes('********')) return null;
  return wecomWebhookUrlLooksValid(raw) ? raw : null;
}

function configuredWebhookUrl(config: Record<string, unknown>): string | null {
  const raw = typeof config.webhook_url === 'string' ? config.webhook_url.trim() : '';
  if (!raw || raw.includes('********')) return null;
  return raw;
}

function wecomWebhookUrlLooksValid(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === 'https:'
      && url.hostname.toLowerCase() === WECOM_WEBHOOK_HOST
      && (url.port === '' || url.port === '443')
      && url.pathname === WECOM_WEBHOOK_PATH
      && !url.username
      && !url.password
      && !url.hash
      && Boolean(url.searchParams.get('key')?.trim())
      && url.searchParams.get('key') !== '********';
  } catch {
    return false;
  }
}

function maskWebhookUrl(value: string): string {
  try {
    const url = new URL(value);
    if (url.username) url.username = '********';
    if (url.password) url.password = '********';
    for (const key of Array.from(url.searchParams.keys())) {
      if (['key', 'token', 'secret', 'access_token'].includes(key.toLowerCase())) {
        url.searchParams.set(key, '********');
      }
    }
    return url.toString();
  } catch {
    return value.length <= 12 ? '********' : `${value.slice(0, 4)}********${value.slice(-4)}`;
  }
}

function summarizeWecomResponse(text: string, response: Response): Record<string, unknown> {
  let parsed: Record<string, unknown> = {};
  try {
    parsed = stringRecord(JSON.parse(text));
  } catch {
    parsed = {};
  }
  const summary: Record<string, unknown> = {
    errmsg: typeof parsed.errmsg === 'string'
      ? parsed.errmsg
      : response.ok ? 'ok' : response.statusText || 'failed',
  };
  if (typeof parsed.errcode === 'number') {
    summary.errcode = parsed.errcode;
  }
  return summary;
}

class WecomTestTimeoutError extends Error {
  constructor() {
    super(`WeCom test request timed out after ${DEV_WECOM_TEST_TIMEOUT_MS}ms`);
    this.name = 'WecomTestTimeoutError';
  }
}

async function sendWecomTestMessage(webhookUrl: string) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), DEV_WECOM_TEST_TIMEOUT_MS);
  let response: Response;
  try {
    response = await fetch(webhookUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      signal: controller.signal,
      body: JSON.stringify({
        msgtype: 'text',
        text: {
          content: `DCC-MCP Admin WeCom test\nGateway: dev-mock\nSent at: ${Date.now()}`,
        },
      }),
    });
  } catch (error) {
    if (error instanceof Error && error.name === 'AbortError') {
      throw new WecomTestTimeoutError();
    }
    throw error;
  } finally {
    clearTimeout(timeout);
  }
  const text = await response.text();
  const body = summarizeWecomResponse(text, response);
  const errcode = typeof body.errcode === 'number' ? body.errcode : null;
  const errmsg = typeof body.errmsg === 'string' ? body.errmsg : response.statusText || 'ok';
  if (!response.ok || (errcode !== null && errcode !== 0)) {
    return {
      ok: false,
      status: response.status,
      body,
      message: errmsg,
    };
  }
  return {
    ok: true,
    status: response.status,
    body,
    message: errmsg,
  };
}

function marketplacePackage(name: string, dcc: string) {
  const catalogEntry = MARKETPLACE_CATALOG.find((entry) => entry.name === name);
  return {
    name,
    dcc,
    version: catalogEntry?.version ?? '1.0.0',
    install_type: 'git',
    path: `C:/Users/demo/.dcc-mcp/${dcc}/skills/${name}`,
    source_name: catalogEntry?.source_name ?? 'official',
    source_url: 'https://github.com/dcc-mcp/marketplace',
    install_url: catalogEntry?.install.url ?? 'https://github.com/dcc-mcp/marketplace.git',
    install_ref: null,
    installed_at_ms: Date.now(),
  };
}

function send(res: ServerResponse, status: number, body: unknown) {
  res.statusCode = status;
  res.setHeader('Content-Type', 'application/json');
  res.end(JSON.stringify(body));
}

function sendText(res: ServerResponse, status: number, contentType: string, body: string) {
  res.statusCode = status;
  res.setHeader('Content-Type', contentType);
  res.end(body);
}

function mockDebugBundle(requestId: string) {
  return {
    request_id: requestId,
    trace_id: `trace-${requestId || 'req-123'}`,
    generated_at: NOW,
    files: ['health.json', 'trace.json', 'logs.json'],
    trace: {
      ...TRACE_DETAIL,
      request_id: requestId || TRACE_DETAIL.request_id,
      trace_id: `trace-${requestId || 'req-123'}`,
    },
    traces: TRACES,
    calls: CALLS,
  };
}

function mockIssueReport(requestId: string) {
  return {
    schema_version: 'dcc-mcp.admin.issue-report.v1',
    request_id: requestId,
    generated_at: NOW,
    summary: 'Mock issue report for local admin UI development.',
    debug_bundle_path: `/admin/api/debug-bundle/${encodeURIComponent(requestId)}`,
    agent_trace_packet_path: `/v1/debug/agent-traces/${encodeURIComponent(requestId)}`,
    safe_issue_report_path: `/admin/api/issue-report/${encodeURIComponent(requestId)}`,
    raw_issue_report_path: `/admin/api/issue-report/${encodeURIComponent(requestId)}?mode=raw`,
    stable_safe_issue_report_path: `/v1/debug/issue-reports/${encodeURIComponent(requestId)}`,
    stable_raw_issue_report_path: `/v1/debug/issue-reports/${encodeURIComponent(requestId)}?mode=raw`,
    trace: {
      ...TRACE_DETAIL,
      request_id: requestId || TRACE_DETAIL.request_id,
      trace_id: `trace-${requestId || 'req-123'}`,
    },
  };
}

function mockAgentTracePacket(lookupId: string) {
  return {
    schema_version: 'dcc-mcp.admin.agent-trace-packet.v1',
    lookup_id: lookupId,
    trace_id: `trace-${lookupId || 'req-123'}`,
    request_id: lookupId || 'req-123',
    request_ids: [lookupId || 'req-123'],
    status: 'ok',
    tool: 'maya-dev__create_sphere',
    dcc_type: 'maya',
    transport: 'rest',
    total_ms: 48,
    span_count: 3,
    payload_tokens: {
      token_estimator: 'dcc-mcp-byte4-v1',
      input_tokens: 3,
      output_tokens: 8,
      total_tokens: 11,
      missing_payload_tokens: false,
    },
    response_token_accounting: CALLS[0].token_accounting,
    postmortem: {
      previous_call_count: 1,
      gateway_event_count: 1,
    },
    links: {
      admin_trace_url: `/admin?panel=traces&trace=${encodeURIComponent(lookupId)}`,
      trace_api_url: `/admin/api/traces/${encodeURIComponent(lookupId)}`,
      agent_trace_packet_url: `/v1/debug/agent-traces/${encodeURIComponent(lookupId)}`,
      debug_bundle_url: `/admin/api/debug-bundle/${encodeURIComponent(lookupId)}`,
      issue_report_url: `/admin/api/issue-report/${encodeURIComponent(lookupId)}`,
      openapi_spec_url: '/v1/openapi.json',
      openapi_docs_url: '/docs',
    },
    privacy_note: 'Agent trace packets omit request/response payload previews, prompts, scripts, and scene data.',
  };
}

const OPENAPI_PAYLOAD = {
  openapi: '3.1.0',
  info: {
    title: 'DCC-MCP Gateway REST API',
    version: HEALTH.version,
  },
  paths: {
    '/v1/search': {
      post: {
        operationId: 'searchTools',
        summary: 'Search indexed DCC-MCP tools.',
        responses: { '200': { description: 'Search results' } },
      },
    },
    '/v1/describe': {
      post: {
        operationId: 'describeTool',
        summary: 'Describe one DCC-MCP tool schema.',
        responses: { '200': { description: 'Tool description' } },
      },
    },
    '/v1/call': {
      post: {
        operationId: 'callTool',
        summary: 'Call a validated DCC-MCP tool.',
        responses: { '200': { description: 'Tool result' } },
      },
    },
    '/v1/debug/integrations': {
      get: {
        operationId: 'getDebugIntegrations',
        summary: 'Read masked integration configuration state.',
        responses: { '200': { description: 'Integration configuration summary' } },
      },
    },
  },
};

export function adminApiMockPlugin(): Plugin {
  return {
    name: 'dcc-mcp-admin-api-mock',
    configureServer(server) {
      const handleAdminApiMock = async (req: IncomingMessage, res: ServerResponse, next: () => void) => {
        const url = req.url ?? '';
        const requestUrl = new URL(url, 'http://admin.local');
        const pathname = requestUrl.pathname;
        if (url.startsWith('/health')) return send(res, 200, HEALTH);
        if (url.startsWith('/activity')) return send(res, 200, ACTIVITY_PAYLOAD);
        if (url.startsWith('/governance')) return send(res, 200, GOVERNANCE_PAYLOAD);
        if (url.startsWith('/workers')) return send(res, 200, WORKERS_PAYLOAD);
        if (url.startsWith('/tools')) return send(res, 200, TOOLS_PAYLOAD);
        if (url.startsWith('/calls')) {
          return send(res, 200, { total: CALLS.length, calls: CALLS });
        }
        if (pathname.startsWith('/traces/') && pathname.length > '/traces/'.length) {
          const requestId = decodeURIComponent(pathname.slice('/traces/'.length));
          return send(res, 200, {
            ...TRACE_DETAIL,
            request_id: requestId || TRACE_DETAIL.request_id,
            trace_id: `trace-${requestId || 'req-123'}`,
          });
        }
        if (url.startsWith('/traces')) {
          return send(res, 200, { total: TRACES.length, traces: TRACES });
        }
        if (url.startsWith('/traffic/export')) {
          return sendText(
            res,
            200,
            'application/x-ndjson',
            TRAFFIC_PAYLOAD.frames.map((frame) => JSON.stringify(frame)).join('\n'),
          );
        }
        if (url.startsWith('/traffic')) return send(res, 200, TRAFFIC_PAYLOAD);
        if (url.startsWith('/tasks')) return send(res, 200, TASKS_PAYLOAD);
        if (url.startsWith('/workflows')) return send(res, 200, WORKFLOWS_PAYLOAD);
        if (url.startsWith('/stats')) {
          return send(res, 200, {
            ...STATS_PAYLOAD,
            range: requestUrl.searchParams.get('range') ?? STATS_PAYLOAD.range,
          });
        }
        if (url.startsWith('/analytics/export')) {
          return sendText(
            res,
            200,
            'text/csv; charset=utf-8',
            'date,calls,errors\n2026-06-09,34,1\n2026-06-10,42,2\n2026-06-11,47,1\n',
          );
        }
        if (url.startsWith('/analytics/overview')) {
          return send(res, 200, {
            ...ANALYTICS_OVERVIEW_PAYLOAD,
            range: requestUrl.searchParams.get('range') ?? ANALYTICS_OVERVIEW_PAYLOAD.range,
          });
        }
        if (url.startsWith('/analytics/timeseries')) return send(res, 200, ANALYTICS_TIMESERIES_PAYLOAD);
        if (url.startsWith('/analytics/heatmap')) return send(res, 200, ANALYTICS_HEATMAP_PAYLOAD);
        if (url.startsWith('/logs')) return send(res, 200, LOGS_PAYLOAD);
        if (pathname.startsWith('/instances/') && pathname.endsWith('/update') && req.method === 'POST') {
          const instanceId = decodeURIComponent(pathname.slice('/instances/'.length, -'/update'.length));
          return send(res, 200, {
            instance_id: instanceId,
            binary: 'dcc-mcp-server',
            current_version: instanceId.startsWith('maya') ? '0.17.38-dev' : null,
            latest_version: '0.18.0',
            status: instanceId.startsWith('maya') ? 'staged' : 'binary_not_found',
            message: instanceId.startsWith('maya')
              ? 'Update staged. Restart the adapter to use 0.18.0.'
              : 'dcc-mcp-server binary was not found for this mocked instance.',
            download_url: 'https://github.com/dcc-mcp/dcc-mcp-core/releases/latest',
            staged_path: instanceId.startsWith('maya') ? 'C:/Users/demo/.dcc-mcp/updates/dcc-mcp-server.exe' : null,
          });
        }
        if (url.startsWith('/instances')) return send(res, 200, INSTANCES_PAYLOAD);
        if (pathname.startsWith('/issue-report/')) {
          const requestId = decodeURIComponent(pathname.slice('/issue-report/'.length));
          return send(res, 200, mockIssueReport(requestId));
        }
        if (pathname.startsWith('/debug-bundle/')) {
          const requestId = decodeURIComponent(pathname.slice('/debug-bundle/'.length));
          return send(res, 200, mockDebugBundle(requestId));
        }
        if (url.startsWith('/skills')) return send(res, 200, SKILLS_PAYLOAD);
        if (url.startsWith('/skill-paths')) {
          if (req.method === 'POST' || req.method === 'DELETE') return send(res, 200, { ok: true });
          return send(res, 200, SKILL_PATHS_PAYLOAD);
        }
        if (url.startsWith('/integrations/test') && req.method === 'POST') {
          const payload = await readJsonBody(req);
          const kind = String(payload.kind ?? '').trim().toLowerCase();
          const config = stringRecord(payload.config);
          if (kind !== 'wecom') {
            return send(res, 400, {
              error: 'unsupported_integration_test',
              message: `test send is not supported for integration kind '${kind}'`,
            });
          }
          const configuredUrl = configuredWebhookUrl(config);
          if (configuredUrl && !wecomWebhookUrlLooksValid(configuredUrl)) {
            return send(res, 400, {
              error: 'invalid_integration_test_config',
              message: `webhook_url must be a valid WeCom robot webhook URL like ${WECOM_WEBHOOK_URL_HINT}`,
            });
          }
          const webhookUrl = concreteWebhookUrlFromConfig(config) ?? devWecomWebhookUrl;
          if (!webhookUrl) {
            return send(res, 400, {
              error: 'invalid_integration_test_config',
              message: 'wecom webhook_url is required before sending a test message',
            });
          }
          try {
            const result = await sendWecomTestMessage(webhookUrl);
            if (!result.ok) {
              return send(res, 502, {
                error: 'wecom_test_failed',
                message: `WeCom test message was rejected: ${result.message}`,
                kind: 'wecom',
                http_status: result.status,
                wecom: result.body,
                webhook_url: maskWebhookUrl(webhookUrl),
              });
            }
            return send(res, 200, {
              kind: 'wecom',
              status: 'sent',
              message: result.message,
              sent_at_ms: Date.now(),
              webhook_url: maskWebhookUrl(webhookUrl),
              wecom: result.body,
            });
          } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            const timedOut = error instanceof WecomTestTimeoutError;
            return send(res, timedOut ? 504 : 502, {
              error: 'wecom_test_failed',
              message: `failed to send WeCom test message: ${message}`,
              kind: 'wecom',
              webhook_url: maskWebhookUrl(webhookUrl),
            });
          }
        }
        if (url.startsWith('/integrations')) {
          if (req.method === 'PUT') {
            const payload = await readJsonBody(req);
            const kind = String(payload.kind ?? 'sentry');
            const config = payload.config && typeof payload.config === 'object' && !Array.isArray(payload.config)
              ? payload.config as Record<string, unknown>
              : {};
            const current = INTEGRATIONS_PAYLOAD.integrations.find((entry) => entry.kind === kind)
              ?? INTEGRATIONS_PAYLOAD.integrations[0];
            if (kind === 'wecom') {
              const configuredUrl = configuredWebhookUrl(config);
              if (configuredUrl && !wecomWebhookUrlLooksValid(configuredUrl)) {
                return send(res, 400, {
                  error: 'invalid_integration_config',
                  message: `webhook_url must be a valid WeCom robot webhook URL like ${WECOM_WEBHOOK_URL_HINT}`,
                });
              }
              devWecomWebhookUrl = concreteWebhookUrlFromConfig(config) ?? devWecomWebhookUrl;
            }
            const responseConfig = {
              ...current.config,
              ...config,
            };
            if (kind === 'wecom' && devWecomWebhookUrl) {
              responseConfig.webhook_url = maskWebhookUrl(devWecomWebhookUrl);
            }
            return send(res, 200, {
              ...current,
              status: 'pending_restart',
              config: responseConfig,
            });
          }
          return send(res, 200, INTEGRATIONS_PAYLOAD);
        }
        if (url.startsWith('/marketplace/catalog')) {
          return send(res, 200, { entries: MARKETPLACE_CATALOG });
        }
        if (url.startsWith('/marketplace/installed')) {
          return send(res, 200, { packages: marketplaceInstalled });
        }
        if (url.startsWith('/marketplace/sources')) {
          if (req.method === 'POST') {
            const payload = await readJsonBody(req);
            const rawSource = String(payload.source ?? '').trim();
            const source = rawSource || 'dcc-mcp/marketplace';
            const entry = {
              name: source.split('/').pop() || source,
              url: source.startsWith('http') ? source : `https://github.com/${source}`,
              origin: 'explicit',
            };
            marketplaceSources = [...marketplaceSources.filter((item) => item.name !== entry.name), entry];
          }
          return send(res, 200, { sources: marketplaceSources });
        }
        if (url.startsWith('/marketplace/outdated')) {
          return send(res, 200, {
            dcc: null,
            count: marketplaceOutdated.length,
            packages: marketplaceOutdated,
          });
        }
        if (url.startsWith('/marketplace/install') && req.method === 'POST') {
          const payload = await readJsonBody(req);
          const name = String(payload.name ?? 'maya-modeling');
          const dcc = String(payload.dcc ?? 'maya');
          const force = Boolean(payload.force);
          const installed = marketplaceInstalled.some((item) => item.name === name && item.dcc === dcc);
          if (!installed || force) {
            marketplaceInstalled = [
              ...marketplaceInstalled.filter((item) => !(item.name === name && item.dcc === dcc)),
              marketplacePackage(name, dcc),
            ];
          }
          return send(res, 200, {
            installed: true,
            name,
            dcc,
            version: marketplacePackage(name, dcc).version,
            path: marketplacePackage(name, dcc).path,
            skill_search_path: `C:/Users/demo/.dcc-mcp/${dcc}/skills`,
            install_type: 'git',
            reload_required: true,
          });
        }
        if (url.startsWith('/marketplace/uninstall') && req.method === 'POST') {
          const payload = await readJsonBody(req);
          const name = String(payload.name ?? '');
          const dcc = String(payload.dcc ?? '');
          marketplaceInstalled = marketplaceInstalled.filter((item) => !(item.name === name && item.dcc === dcc));
          marketplaceOutdated = marketplaceOutdated.filter((item) => !(item.name === name && item.dcc === dcc));
          return send(res, 200, {
            uninstalled: true,
            name,
            dcc,
            path: `C:/Users/demo/.dcc-mcp/${dcc}/skills/${name}`,
            removed_state: true,
            removed_files: true,
            reload_required: true,
          });
        }
        if (url.startsWith('/marketplace/update') && req.method === 'POST') {
          const payload = await readJsonBody(req);
          const name = String(payload.name ?? 'cross-dcc-utils');
          const dcc = String(payload.dcc ?? 'maya');
          const updated = marketplaceOutdated.filter((item) => item.name === name && item.dcc === dcc);
          marketplaceOutdated = marketplaceOutdated.filter((item) => !(item.name === name && item.dcc === dcc));
          marketplaceInstalled = [
            ...marketplaceInstalled.filter((item) => !(item.name === name && item.dcc === dcc)),
            marketplacePackage(name, dcc),
          ];
          return send(res, 200, {
            updated: updated.length || 1,
            results: [{
              updated: true,
              name,
              dcc,
              previous_version: updated[0]?.installed_version ?? null,
              new_version: updated[0]?.latest_version ?? marketplacePackage(name, dcc).version,
              path: marketplacePackage(name, dcc).path,
              install_type: 'git',
              source_name: 'official',
              source_url: 'https://github.com/dcc-mcp/marketplace',
              reload_required: true,
            }],
          });
        }
        if (url.startsWith('/skill-detail')) {
          return send(res, 200, {
            skill: {
              name: 'maya-modeling',
              description: 'Modeling primitives for Maya.',
              dcc_type: 'maya',
              state: 'loaded',
              markdown: '---\nname: maya-modeling\n---\n\n# Modeling\n\nSphere, cube, extrude, bevel.',
              tools: [
                { name: 'create_sphere', summary: 'Create a UV sphere primitive', annotations: { readonly: true } },
                { name: 'extrude_edges', summary: 'Extrude selected edges', annotations: { destructive: true } },
              ],
            },
            instances: [],
          });
        }
        return next();
      };

      const handleV1DebugMock = async (req: IncomingMessage, res: ServerResponse, next: () => void) => {
        const url = req.url ?? '';
        const requestUrl = new URL(url, 'http://admin.local');
        const pathname = requestUrl.pathname;
        if (pathname.startsWith('/agent-traces/') && pathname.length > '/agent-traces/'.length) {
          const lookupId = decodeURIComponent(pathname.slice('/agent-traces/'.length));
          return send(res, 200, mockAgentTracePacket(lookupId));
        }
        if (pathname.startsWith('/issue-reports/') && pathname.length > '/issue-reports/'.length) {
          const requestId = decodeURIComponent(pathname.slice('/issue-reports/'.length));
          return send(res, 200, mockIssueReport(requestId));
        }
        if (pathname.startsWith('/bundles/') && pathname.length > '/bundles/'.length) {
          const requestId = decodeURIComponent(pathname.slice('/bundles/'.length));
          return send(res, 200, mockDebugBundle(requestId));
        }
        return handleAdminApiMock(req, res, next);
      };

      const handleV1OpenApiMock = async (_req: IncomingMessage, res: ServerResponse) => {
        return send(res, 200, OPENAPI_PAYLOAD);
      };

      server.middlewares.use('/admin/api', handleAdminApiMock);
      server.middlewares.use('/api', handleAdminApiMock);
      server.middlewares.use('/v1/debug', handleV1DebugMock);
      server.middlewares.use('/v1/openapi.json', handleV1OpenApiMock);
    },
  };
}
