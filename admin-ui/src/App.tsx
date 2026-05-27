import { type ReactNode, useCallback, useEffect, useMemo, useState } from 'react';
import mayaIcon from './assets/icons/autodesk.svg';
import blenderIcon from './assets/icons/blender.svg';
import claudeIcon from './assets/icons/claude.svg';
import clineIcon from './assets/icons/cline.svg';
import codebuddyIcon from './assets/icons/codebuddy.svg';
import cursorIcon from './assets/icons/cursor.svg';
import gimpIcon from './assets/icons/gimp.svg';
import inkscapeIcon from './assets/icons/inkscape.svg';
import kritaIcon from './assets/icons/krita.svg';
import openaiIcon from './assets/icons/openai.svg';
import unityIcon from './assets/icons/unity.svg';
import unrealIcon from './assets/icons/unrealengine.svg';
import substancePainterIcon from './assets/icons/photoshop.svg';
import puzzleIcon from './assets/icons/puzzle.svg';
import vscodeIcon from './assets/icons/vscode.svg';
import { LanguageSelector } from './components/LanguageSelector';
import { LogsPanel } from './components/LogsPanel';
import { SkillMarkdownPreview } from './components/SkillMarkdownPreview';
import { createTranslator, detectBrowserLocale, type MessageKey, type SupportedLocale } from './i18n';
import { readLocaleOverride, storeLocaleOverride } from './locale';
import { filterLogs, isProblemLog, normalizeLogRow, summarizeLogSeverity, type LogRow, type LogSeverityFilter } from './logs';
import { formatTime, timestampTitle } from './time';

type Translator = ReturnType<typeof createTranslator>;

type Panel = 'setup' | 'debug' | 'activity' | 'health' | 'instances' | 'tools' | 'workflows' | 'tasks' | 'openapi' | 'calls' | 'traces' | 'traffic' | 'stats' | 'governance' | 'logs' | 'skill-paths';

type SignalTone = 'ok' | 'warn' | 'err';
type LatencySeverity = 'slow' | 'critical';

const SLOW_LATENCY_MS = 1_000;
const CRITICAL_LATENCY_MS = 5_000;

type DebugSignal = {
  key: string;
  label: string;
  value: string;
  detail: string;
  tone: SignalTone;
  panel: Panel;
  traceId?: string;
};

type FailureSignal = {
  request_id: string;
  status: string;
  tool: string;
  detail: string;
  ms: number | null;
};

type HealthPayload = {
  status: string;
  instances_ready: number;
  instances_total: number;
  uptime_secs: number;
  version: string;
  rss_bytes?: number | null;
  response_format?: {
    default?: string | null;
    legacy_mime?: string | null;
    compact_mime?: string | null;
    token_estimator?: string | null;
  };
  gateway?: {
    current?: GatewaySentinel | null;
    candidates?: GatewaySentinel[];
  };
  limits?: {
    body_max_bytes: number;
    rate_limit_per_minute_per_ip: number;
    xff_trusted_depth: number;
    read_retry_max: number;
    circuit_failure_threshold: number;
    circuit_open_secs: number;
  };
  circuits?: { tracked_backends: number; circuits_open: number };
};

type GatewaySentinel = {
  name: string;
  role: string;
  pid?: number | null;
  host: string;
  port: number;
  instance_id: string;
  version?: string | null;
  adapter_version?: string | null;
  adapter_dcc?: string | null;
};

type ToolRow = {
  slug: string;
  dcc_type: string;
  summary: string;
  skill_name?: string | null;
  name?: string;
  instance_id?: string;
  instance_prefix?: string;
};

type AttributionTrust = {
  actor_id?: string | null;
  actor_name?: string | null;
  actor_email_hash?: string | null;
  agent_id?: string | null;
  agent_name?: string | null;
  agent_kind?: string | null;
  agent_version?: string | null;
  model?: string | null;
  model_provider?: string | null;
  model_version?: string | null;
  client_platform?: string | null;
  client_os?: string | null;
  client_host?: string | null;
  auth_subject?: string | null;
  source_ip?: string | null;
  forwarded_for?: string | null;
};

type CallRow = {
  timestamp: string;
  request_id: string;
  tool: string;
  dcc_type: string;
  status: string;
  success: boolean;
  error: string | null;
  duration_ms: number | null;
  instance_id?: string | null;
  transport?: string | null;
  agent_id?: string | null;
  agent_name?: string | null;
  agent_model?: string | null;
  actor?: string | null;
  actor_id?: string | null;
  actor_name?: string | null;
  actor_email_hash?: string | null;
  client_platform?: string | null;
  client_os?: string | null;
  client_host?: string | null;
  auth_subject?: string | null;
  source_ip?: string | null;
  attribution_trust?: AttributionTrust | null;
  parent_request_id?: string | null;
  token_accounting?: TokenAccounting | null;
  response_format?: string | null;
  token_estimator?: string | null;
  original_bytes?: number | null;
  returned_bytes?: number | null;
  original_tokens?: number | null;
  returned_tokens?: number | null;
  saved_tokens?: number | null;
  savings_pct?: number | string | null;
  links?: AdminLinks;
};

type TraceRow = {
  timestamp: string;
  request_id: string;
  tool: string;
  status: string;
  success: boolean;
  total_ms: number | null;
  instance_id?: string | null;
  dcc_type?: string | null;
  transport?: string | null;
  agent_id?: string | null;
  agent_name?: string | null;
  agent_model?: string | null;
  actor?: string | null;
  actor_id?: string | null;
  actor_name?: string | null;
  actor_email_hash?: string | null;
  client_platform?: string | null;
  client_os?: string | null;
  client_host?: string | null;
  auth_subject?: string | null;
  source_ip?: string | null;
  attribution_trust?: AttributionTrust | null;
  span_count?: number | null;
  input_bytes?: number | null;
  output_bytes?: number | null;
  slowest_span_name?: string | null;
  slowest_span_ms?: number | null;
  token_accounting?: TokenAccounting | null;
  response_format?: string | null;
  token_estimator?: string | null;
  original_bytes?: number | null;
  returned_bytes?: number | null;
  original_tokens?: number | null;
  returned_tokens?: number | null;
  saved_tokens?: number | null;
  savings_pct?: number | string | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  total_tokens?: number | null;
  payload_token_estimator?: string | null;
  links?: AdminLinks;
};

type TokenAccounting = {
  response_format?: string | null;
  token_estimator?: string | null;
  original_bytes?: number | null;
  returned_bytes?: number | null;
  original_tokens?: number | null;
  returned_tokens?: number | null;
  saved_tokens?: number | null;
  savings_pct?: number | string | null;
};

type TokenCarrier = {
  token_accounting?: TokenAccounting | null;
  response_format?: string | null;
  token_estimator?: string | null;
  original_bytes?: number | null;
  returned_bytes?: number | null;
  original_tokens?: number | null;
  returned_tokens?: number | null;
  saved_tokens?: number | null;
  savings_pct?: number | string | null;
};

type AdminLinks = {
  admin_trace_url?: string;
  trace_api_url?: string;
  agent_trace_packet_url?: string;
  debug_bundle_url?: string;
  issue_report_url?: string;
  openapi_inspector_url?: string;
  openapi_spec_url?: string;
  openapi_docs_url?: string;
  stats_url?: string;
  admin_traces_url?: string;
  admin_workflows_url?: string;
  admin_tasks_url?: string;
  admin_calls_url?: string;
};

type TrafficLinks = {
  admin_traffic_url?: string;
  traffic_api_url?: string;
  traffic_export_jsonl_url?: string;
};

type TrafficFrameEnvelope = {
  schema_version?: number;
  name?: string;
  id?: string;
  timestamp_ns?: number;
  source?: Record<string, unknown>;
  correlation?: {
    request_id?: string;
    trace_id?: string;
    session_id?: string;
    [key: string]: unknown;
  };
  attributes?: TrafficFrameAttributes;
};

type TrafficFrameAttributes = {
  capture_id?: string;
  session_id?: string | null;
  direction?: string;
  leg?: string;
  transport?: string;
  http?: {
    method?: string;
    url?: string;
    status?: number | null;
    headers?: Record<string, string>;
  };
  mcp?: {
    kind?: string;
    method?: string;
    id?: unknown;
  };
  body?: {
    encoding?: string;
    data?: unknown;
    size_bytes?: number;
    redacted_paths?: string[];
  };
};

type TrafficCaptureStatus = {
  state?: 'captured' | 'capture_disabled' | 'capture_unavailable' | 'capture_filtered' | 'no_traffic' | string;
  message?: string;
  capture_enabled?: boolean;
  live_sink_enabled?: boolean;
  sink_count?: number;
  subscriber_enabled?: boolean;
  retained_frames?: number;
  recent_decision_count?: number;
  captured_decision_count?: number;
  skipped_decision_count?: number;
  skip_reasons?: string[];
  redacted_path_count?: number;
  redacted_paths?: string[];
  safe_to_share?: boolean;
  payload_policy?: string;
  retention?: {
    admin_live_configured?: boolean;
    ring_buffer_capacity?: number | null;
  };
};

type TrafficPayload = {
  schema_version?: string;
  total?: number;
  frames?: TrafficFrameEnvelope[];
  capture_status?: TrafficCaptureStatus;
  links?: TrafficLinks;
};

type AgentContext = {
  actor_id?: string | null;
  actor_name?: string | null;
  actor_email_hash?: string | null;
  agent_id?: string | null;
  agent_name?: string | null;
  agent_kind?: string | null;
  agent_version?: string | null;
  model_provider?: string | null;
  model_version?: string | null;
  model?: string | null;
  reasoning_effort?: string | null;
  session_id?: string | null;
  turn_id?: string | null;
  task?: string | null;
  client_platform?: string | null;
  client_os?: string | null;
  client_host?: string | null;
  auth_subject?: string | null;
  source_ip?: string | null;
  forwarded_for?: string[];
  trust?: AttributionTrust;
  user_intent_summary?: string | null;
  agent_reply_summary?: string | null;
  user_input_hash?: string | null;
  agent_reply_hash?: string | null;
  user_input_chars?: number | null;
  agent_reply_chars?: number | null;
  reasoning_summary?: string | null;
  plan?: string[];
  observations?: string[];
  tags?: string[];
  parent_request_id?: string | null;
  trace_id?: string | null;
  turn_index?: number | null;
  metadata?: unknown;
};

type TracePayload = {
  content: string;
  mime_type: string;
  truncated: boolean;
  original_size: number;
  estimated_tokens?: number | null;
};

type TraceSpan = {
  name: string;
  started_ns: number;
  duration_ns: number;
  ok: boolean;
  attributes?: Record<string, unknown>;
};

type TraceDetailPayload = {
  request_id: string;
  method: string;
  tool_slug?: string | null;
  instance_id?: string | null;
  session_id?: string | null;
  dcc_type?: string | null;
  transport?: string | null;
  agent_context?: AgentContext | null;
  started_at?: number | string;
  total_ms: number;
  ok: boolean;
  spans: TraceSpan[];
  input?: TracePayload | null;
  output?: TracePayload | null;
  token_accounting?: TokenAccounting | null;
  response_format?: string | null;
  token_estimator?: string | null;
  original_bytes?: number | null;
  returned_bytes?: number | null;
  original_tokens?: number | null;
  returned_tokens?: number | null;
  saved_tokens?: number | null;
  savings_pct?: number | string | null;
  estimated_tokens?: number | null;
  estimated_total_tokens?: number | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  total_tokens?: number | null;
  payload_token_estimator?: string | null;
  links?: AdminLinks;
};

type OpenApiSpec = {
  openapi?: string;
  info?: {
    title?: string;
    version?: string;
    description?: string;
  };
  servers?: { url?: string; description?: string }[];
  tags?: { name?: string; description?: string }[];
  paths?: Record<string, Record<string, OpenApiOperationObject> | unknown>;
  components?: unknown;
};

type OpenApiOperationObject = {
  operationId?: string;
  summary?: string;
  description?: string;
  tags?: string[];
  parameters?: unknown[];
  requestBody?: unknown;
  responses?: Record<string, unknown>;
};

type OpenApiOperationRow = {
  key: string;
  method: string;
  path: string;
  operationId: string;
  summary: string;
  tags: string[];
  responseCodes: string[];
  hasRequestBody: boolean;
  parameterCount: number;
};

type OpenApiSource = {
  label: string;
  specUrl: string;
  docsUrl: string;
  inspectorUrl: string;
  kind: 'gateway' | 'instance';
};

type ActivityEvent = {
  event_id: string;
  timestamp: string;
  kind: string;
  severity: string;
  status: string;
  message: string;
  tool?: string | null;
  duration_ms?: number | null;
  correlation?: {
    request_id?: string;
    session_id?: string;
    instance_id?: string;
    dcc_type?: string;
    workflow_id?: string;
    job_id?: string;
    agent_id?: string;
    actor_id?: string;
    actor_name?: string;
    client_platform?: string;
    source_ip?: string;
    parent_request_id?: string;
  };
};

type TaskRow = {
  task_id: string;
  task_type: string;
  status: string;
  title: string;
  goal?: string | null;
  summary?: string | null;
  final_result?: string | null;
  failure_reason?: string | null;
  started_at: string;
  finished_at?: string | null;
  duration_ms?: number | null;
  app_types?: string[];
  artifacts?: TaskArtifact[];
  validation_checks?: TaskValidation[];
  related?: TaskRelated;
  correlation?: ActivityEvent['correlation'];
  links?: AdminLinks & { primary_request?: AdminLinks };
};

type TaskRelated = {
  workflow_ids?: string[];
  request_ids?: string[];
  trace_ids?: string[];
  session_ids?: string[];
};

type TaskArtifact = {
  name: string;
  kind: string;
  request_id?: string | null;
};

type TaskValidation = {
  title: string;
  status: string;
  request_id?: string | null;
};

type WorkflowAgent = {
  agent_id?: string | null;
  agent_name?: string | null;
  agent_kind?: string | null;
  model_provider?: string | null;
  model_version?: string | null;
  model?: string | null;
  reasoning_effort?: string | null;
  session_id?: string | null;
  turn_id?: string | null;
  task?: string | null;
  user_intent_summary?: string | null;
  agent_reply_summary?: string | null;
  user_input_hash?: string | null;
  agent_reply_hash?: string | null;
  user_input_chars?: number | null;
  agent_reply_chars?: number | null;
  turn_index?: number | null;
  tags?: string[];
};

type WorkflowSearchSignal = {
  search_id: string;
  selected_rank?: number | null;
  selected_score?: number | null;
  match_reasons?: string[];
  zero_results?: boolean | null;
  result_count?: number | null;
  first_success_ms?: number | null;
};

type WorkflowStep = {
  step_id: string;
  kind: string;
  title: string;
  timestamp: string;
  status: string;
  success?: boolean | null;
  request_id?: string | null;
  trace_id?: string | null;
  parent_request_id?: string | null;
  session_id?: string | null;
  dcc_type?: string | null;
  instance_id?: string | null;
  tool?: string | null;
  transport?: string | null;
  duration_ms?: number | null;
  search?: WorkflowSearchSignal | null;
  links?: AdminLinks;
};

type WorkflowRow = {
  workflow_id: string;
  group_kind: string;
  title: string;
  status: string;
  started_at: string;
  finished_at: string;
  duration_ms?: number | null;
  step_count: number;
  failed_steps: number;
  agent?: WorkflowAgent | null;
  correlation: {
    session_id?: string | null;
    trace_id?: string | null;
    agent_id?: string | null;
    turn_id?: string | null;
    request_ids?: string[];
    trace_ids?: string[];
    session_ids?: string[];
  };
  discovery: {
    search_count: number;
    zero_result_count: number;
    selected_count: number;
    best_selected_rank?: number | null;
    time_to_first_success_ms?: number | null;
    search_ids?: string[];
  };
  steps: WorkflowStep[];
  links?: AdminLinks;
};

type WorkflowGraphStage = 'intent' | 'discovery' | 'skillLoad' | 'toolCalls' | 'fallbacks' | 'artifacts' | 'validation' | 'report';

type WorkflowGraphNode = {
  node_id: string;
  node_kind: 'intent' | 'step' | 'report';
  stage: WorkflowGraphStage;
  title: string;
  status: string;
  timestamp?: string | null;
  duration_ms?: number | null;
  step?: WorkflowStep;
  escape_hatch?: boolean;
};

type LatencyBlock = {
  min_ms?: number;
  max_ms?: number;
  mean_ms?: number;
  p50_ms?: number;
  p95_ms?: number;
  p99_ms?: number;
};

type TopEntry = { name: string; count: number };

type AttributionFacet = {
  name: string;
  count: number;
  failed?: number;
  failure_rate?: number;
  mean_latency_ms?: number;
  p95_latency_ms?: number;
};

type TokenBreakdownEntry = {
  name: string;
  calls: number;
  returned_tokens: number;
  saved_tokens: number;
  savings_pct: number;
};

type TokenUsageStats = {
  total_original_bytes?: number;
  total_returned_bytes?: number;
  total_original_tokens?: number;
  total_returned_tokens?: number;
  total_saved_tokens?: number;
  average_savings_pct?: number;
  by_tool?: TokenBreakdownEntry[];
  by_instance?: TokenBreakdownEntry[];
  by_agent?: TokenBreakdownEntry[];
  by_transport?: TokenBreakdownEntry[];
  by_response_format?: TokenBreakdownEntry[];
};

type PayloadTokenUsageStats = {
  token_estimator?: string;
  total_input_tokens?: number;
  total_output_tokens?: number;
  total_tokens?: number;
  calls_with_input_tokens?: number;
  calls_with_output_tokens?: number;
  calls_with_any_payload_tokens?: number;
  calls_missing_payload_tokens?: number;
  avg_input_tokens_per_call?: number;
  avg_output_tokens_per_call?: number;
  avg_total_tokens_per_call?: number;
  avg_total_tokens_per_recorded_call?: number;
};

type StatsPayload = {
  range: string;
  total_calls: number;
  successful_calls?: number;
  failed_calls?: number;
  success_rate: number;
  total_tokens?: number | null;
  total_input_tokens?: number | null;
  total_output_tokens?: number | null;
  avg_tokens_per_call?: number | null;
  avg_input_tokens_per_call?: number | null;
  avg_output_tokens_per_call?: number | null;
  avg_total_tokens_per_call?: number | null;
  payload_token_estimator?: string | null;
  p50_ms?: number | null;
  p95_ms?: number | null;
  latency_ms?: LatencyBlock;
  top_app_types?: TopEntry[];
  top_tools?: TopEntry[];
  top_instances?: TopEntry[];
  top_agents?: TopEntry[];
  top_actors?: AttributionFacet[];
  top_client_platforms?: AttributionFacet[];
  top_source_ips?: AttributionFacet[];
  payload_token_usage?: PayloadTokenUsageStats;
  token_usage?: TokenUsageStats;
  hourly_distribution?: number[];
  governance?: GovernanceStats;
  error?: string;
};

type GovernanceStats = {
  recent_allowed?: number;
  recent_policy_denied?: number;
  recent_throttled?: number;
  captured_frames?: number;
  skipped_capture_frames?: number;
  redacted_path_count?: number;
  redacted_paths?: string[];
};

type GovernancePolicyPayload = {
  read_only?: boolean;
  unrestricted?: boolean;
  allowlists_active?: Record<string, boolean>;
  allowed_dcc_types?: string[];
  allowed_skill_names?: string[];
  allowed_skill_families?: string[];
  allowed_tool_slugs?: string[];
  allowed_tool_slug_prefixes?: string[];
};

type GovernanceTrafficPayload = {
  enabled?: boolean;
  mode?: string;
  sink_count?: number;
  subscriber_enabled?: boolean;
  sinks?: { kind?: string; path?: string | null }[];
  redaction?: { rule_count?: number; paths?: string[] };
  filter?: {
    include?: { path?: string; pattern?: string }[];
    exclude?: { path?: string; pattern?: string }[];
  };
  production_profile?: boolean;
  force_capture?: boolean;
  production_guardrail?: string;
  recent_decisions?: GovernanceCaptureDecision[];
};

type GovernanceCaptureDecision = {
  timestamp?: string;
  request_id?: string | null;
  trace_id?: string | null;
  session_id?: string | null;
  direction?: string;
  leg?: string;
  transport?: string;
  http_url?: string | null;
  mcp_method?: string | null;
  outcome?: string;
  reason?: string | null;
  redacted_paths?: string[];
  body_size_bytes?: number;
};

type GovernanceMiddlewareControl = {
  kind: string;
  mode: string;
  summary: string;
  config?: Record<string, unknown>;
};

type GovernancePayload = {
  schema_version?: string;
  generated_at?: string;
  mode?: { admin_mutations?: string; reason?: string };
  policy?: GovernancePolicyPayload;
  traffic_capture?: GovernanceTrafficPayload;
  middleware?: {
    before_count?: number;
    after_count?: number;
    controls?: GovernanceMiddlewareControl[];
  };
  stats?: GovernanceStats;
  recent_decisions?: GovernanceDecisionRow[];
};

type GovernanceDecisionRow = {
  timestamp?: string;
  request_id?: string | null;
  trace_id?: string | null;
  session_id?: string | null;
  transport?: string | null;
  agent_id?: string | null;
  agent_name?: string | null;
  agent_model?: string | null;
  actor_id?: string | null;
  actor_name?: string | null;
  client_platform?: string | null;
  source_ip?: string | null;
  parent_request_id?: string | null;
  tool?: string | null;
  dcc_type?: string | null;
  outcome?: string;
  success?: boolean | null;
  reason?: string | null;
  duration_ms?: number | null;
  policy?: { read_only?: boolean; denied?: boolean; reason?: string | null };
  traffic_capture?: { frame_count?: number; captured?: number; skipped?: number; reasons?: string[] };
  privacy?: { redaction_middleware_active?: boolean; redacted_paths?: string[] };
  pressure?: { quota_active?: boolean; throttled?: boolean };
};

type InstanceRow = {
  instance_id: string;
  display_name: string | null;
  dcc_type: string;
  status: string;
  stale: boolean;
  pid: number | null;
  host?: string;
  port?: number;
  uptime_secs: number | null;
  version: string | null;
  adapter_version: string | null;
  cpu_percent: number | null;
  memory_bytes: number | null;
  mcp_url: string;
  scene?: string | null;
  failure_reason?: string | null;
  failure_stage?: string | null;
};

type InstanceSummary = {
  live: number;
  stale: number;
  unhealthy: number;
};

type SkillPathRow = {
  path: string;
  display_path?: string;
  path_alias?: string;
  path_hash?: string;
  path_redacted?: boolean;
  status?: string;
  exists?: boolean;
  source: string;
  id?: number;
};

type SkillAdoptionMetrics = {
  search_hits: number;
  best_rank: number | null;
  average_rank: number | null;
  selected_count: number;
  call_count: number;
  failure_count: number;
  load_error_count: number;
  last_searched: string | null;
  last_used: string | null;
  fallback_displaced_by_scripting: number;
  searched: boolean;
  used: boolean;
  low_adoption: boolean;
};

type SkillRow = {
  name: string;
  dcc_type: string;
  loaded: boolean;
  action_count: number;
  instance_count: number;
  instances: string[];
  instance_ids: string[];
  instance_details: SkillInstanceRef[];
  tools: string[];
  summary?: string | null;
  adoption: SkillAdoptionMetrics;
  package?: string | null;
  version?: string | null;
};

type SkillInstanceRef = {
  id: string;
  prefix: string;
  dcc_type?: string;
};

type SkillPayload = {
  skills?: unknown[];
  total?: number;
  loaded?: number;
  unloaded?: number;
  action_count?: number;
  health?: {
    searched_skills?: number;
    used_skills?: number;
    low_adoption_skills?: number;
    load_error_count?: number;
    missing_path_count?: number;
  };
  error?: string;
};

type SkillDetailInstance = {
  name?: string;
  description?: string;
  dcc?: string;
  dcc_type?: string;
  state?: string;
  skill_path?: string;
  skill_md_path?: string | null;
  markdown?: string | null;
  instance_id?: string;
  instance_short?: string;
  tools?: unknown[];
  error?: string;
  message?: string;
  raw?: unknown;
};

type SkillDetailPayload = {
  skill?: SkillDetailInstance | null;
  instances?: SkillDetailInstance[];
  error?: string | null;
};

type SetupUrlMode = 'local' | 'lan' | 'direct';
type ClientPlatform = 'windows' | 'macos' | 'linux';

type IdeTarget = {
  id: string;
  label: string;
  configPath: string | Record<ClientPlatform, string>;
  icon: string;
  build: (url: string) => string;
};

function stringList(value: unknown): string[] {
  return Array.isArray(value) ? value.map((item) => String(item)) : [];
}

function recordOrNull(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' ? value as Record<string, unknown> : null;
}

function normalizeSkillInstanceRef(raw: unknown, fallbackPrefix = '', fallbackDcc = ''): SkillInstanceRef {
  const o = recordOrNull(raw);
  const id = String(o?.id ?? o?.instance_id ?? fallbackPrefix);
  const prefix = String(o?.prefix ?? o?.instance_short ?? fallbackPrefix ?? id);
  return {
    id,
    prefix,
    dcc_type: String(o?.dcc_type ?? fallbackDcc),
  };
}

function defaultSkillAdoption(): SkillAdoptionMetrics {
  return {
    search_hits: 0,
    best_rank: null,
    average_rank: null,
    selected_count: 0,
    call_count: 0,
    failure_count: 0,
    load_error_count: 0,
    last_searched: null,
    last_used: null,
    fallback_displaced_by_scripting: 0,
    searched: false,
    used: false,
    low_adoption: false,
  };
}

function normalizeSkillAdoption(raw: unknown): SkillAdoptionMetrics {
  const o = recordOrNull(raw);
  const fallback = defaultSkillAdoption();
  if (!o) {
    return fallback;
  }
  const bestRank = o.best_rank == null ? null : Number(o.best_rank);
  const averageRank = o.average_rank == null ? null : Number(o.average_rank);
  const searchHits = Number(o.search_hits ?? 0);
  const callCount = Number(o.call_count ?? 0);
  return {
    search_hits: searchHits,
    best_rank: Number.isFinite(bestRank ?? NaN) ? bestRank : null,
    average_rank: Number.isFinite(averageRank ?? NaN) ? averageRank : null,
    selected_count: Number(o.selected_count ?? 0),
    call_count: callCount,
    failure_count: Number(o.failure_count ?? 0),
    load_error_count: Number(o.load_error_count ?? 0),
    last_searched: o.last_searched == null ? null : String(o.last_searched),
    last_used: o.last_used == null ? null : String(o.last_used),
    fallback_displaced_by_scripting: Number(o.fallback_displaced_by_scripting ?? 0),
    searched: o.searched === true || searchHits > 0,
    used: o.used === true || callCount > 0,
    low_adoption: o.low_adoption === true,
  };
}

function safeSkillPathDisplay(source: string, id?: number): string {
  const safeSource = source.replace(/[^\w:.-]+/g, '_').slice(0, 64) || 'skill_path';
  return `${safeSource} #${id ?? 1}`;
}

function normalizeSkillPathRow(raw: unknown): SkillPathRow {
  const o = recordOrNull(raw);
  if (!o) {
    return { path: '', source: '' };
  }
  const source = String(o.source ?? '');
  const id = o.id == null ? undefined : Number(o.id);
  const rawPath = String(o.path ?? '');
  const safeId = typeof id === 'number' && Number.isFinite(id) ? id : undefined;
  const displayPath = o.display_path == null
    ? safeSkillPathDisplay(source, safeId)
    : String(o.display_path);
  return {
    path: rawPath,
    display_path: displayPath,
    path_alias: o.path_alias == null ? undefined : String(o.path_alias),
    path_hash: o.path_hash == null ? undefined : String(o.path_hash),
    path_redacted: o.path_redacted !== false,
    status: o.status == null ? undefined : String(o.status),
    exists: o.exists == null ? undefined : o.exists === true,
    source,
    id: safeId,
  };
}

function normalizeSkillRow(raw: unknown): SkillRow {
  if (!raw || typeof raw !== 'object') {
    return {
      name: '',
      dcc_type: '',
      loaded: false,
      action_count: 0,
      instance_count: 0,
      instances: [],
      instance_ids: [],
      instance_details: [],
      tools: [],
      adoption: defaultSkillAdoption(),
    };
  }
  const o = raw as Record<string, unknown>;
  const instances = stringList(o.instances);
  const instanceIds = stringList(o.instance_ids);
  const dccType = String(o.dcc_type ?? o.dcc ?? '');
  const explicitDetails = Array.isArray(o.instance_details)
    ? o.instance_details.map((item, index) => normalizeSkillInstanceRef(item, instances[index], dccType))
    : [];
  const instanceDetails = explicitDetails.length > 0
    ? explicitDetails
    : instances.map((prefix, index) => ({ id: instanceIds[index] ?? prefix, prefix, dcc_type: dccType }));
  const tools = stringList(o.tools);
  return {
    name: String(o.name ?? ''),
    dcc_type: dccType,
    loaded: o.loaded === true,
    action_count: Number(o.action_count ?? tools.length ?? 0),
    instance_count: Number(o.instance_count ?? instances.length ?? 0),
    instances,
    instance_ids: instanceIds,
    instance_details: instanceDetails,
    tools,
    summary: o.summary == null ? null : String(o.summary),
    adoption: normalizeSkillAdoption(o.adoption),
    package: o.package == null ? null : String(o.package),
    version: o.version == null ? null : String(o.version),
  };
}

function normalizeSkillDetailInstance(raw: unknown): SkillDetailInstance {
  const o = recordOrNull(raw);
  if (!o) {
    return { raw };
  }
  return {
    name: o.name == null ? undefined : String(o.name),
    description: o.description == null ? undefined : String(o.description),
    dcc: o.dcc == null ? undefined : String(o.dcc),
    dcc_type: o.dcc_type == null ? undefined : String(o.dcc_type),
    state: o.state == null ? undefined : String(o.state),
    skill_path: o.skill_path == null ? undefined : String(o.skill_path),
    skill_md_path: o.skill_md_path == null ? null : String(o.skill_md_path),
    markdown: o.markdown == null ? null : String(o.markdown),
    instance_id: o.instance_id == null ? undefined : String(o.instance_id),
    instance_short: o.instance_short == null ? undefined : String(o.instance_short),
    tools: Array.isArray(o.tools) ? o.tools : undefined,
    error: o.error == null ? undefined : String(o.error),
    message: o.message == null ? undefined : String(o.message),
    raw,
  };
}

function normalizeSkillDetailPayload(raw: unknown): SkillDetailPayload {
  const o = recordOrNull(raw);
  if (!o) {
    return { skill: null, instances: [], error: 'Invalid skill detail payload' };
  }
  const instances = Array.isArray(o.instances)
    ? o.instances.map(normalizeSkillDetailInstance)
    : [];
  const skill = o.skill == null || o.skill === false
    ? instances[0] ?? null
    : normalizeSkillDetailInstance(o.skill);
  return {
    skill,
    instances,
    error: o.error == null ? null : String(o.error),
  };
}

/// DCC-type → icon URL (local SVGs, bundled by Vite + vite-plugin-singlefile).
/// Unknown/missing types fall back to a generic puzzle-piece icon.
const DCC_ICON_MAP: Record<string, string> = {
  maya: mayaIcon,
  blender: blenderIcon,
  gimp: gimpIcon,
  inkscape: inkscapeIcon,
  krita: kritaIcon,
  unity: unityIcon,
  unreal: unrealIcon,
  substance_painter: substancePainterIcon,
};
const DCC_ICON_FALLBACK = puzzleIcon;

/// Resolve icon URL for a dcc_type, supporting prefix matching
/// (e.g. "autodesk_maya" → maya icon).
function resolveDccIcon(dccType: string): string {
  const key = dccType.toLowerCase();
  if (DCC_ICON_MAP[key]) return DCC_ICON_MAP[key];
  // Prefix match: "autodesk_maya" → "maya"
  for (const [k, url] of Object.entries(DCC_ICON_MAP)) {
    if (key.includes(k)) return url;
  }
  return DCC_ICON_FALLBACK;
}

/// Resolve JSON API base from the current admin URL so custom `--admin-path`
/// (e.g. `/gw-admin`) works. A fixed `/admin/api` prefix 404s on non-default mounts.
function adminApiBase(): string {
  const { origin, pathname } = window.location;
  let basePath = pathname.replace(/\/+$/, '');
  if (basePath.endsWith('/index.html')) {
    basePath = basePath.slice(0, -'/index.html'.length);
  }
  if (!basePath || basePath === '/') {
    basePath = '/admin';
  }
  const prefix = basePath.endsWith('/') ? basePath : `${basePath}/`;
  return `${origin}${prefix}api`;
}

const API_BASE = adminApiBase();
/** Abort hung admin fetches so the UI does not wait indefinitely on a wedged gateway. */
const ADMIN_FETCH_TIMEOUT_MS = 25_000;
const DEFAULT_LOCAL_GATEWAY_PORT = '9765';
const OPENAPI_METHODS = new Set(['get', 'put', 'post', 'delete', 'patch', 'options', 'head', 'trace']);
const IDE_SERVER_NAME = 'dcc-mcp-gateway';
const buildMcpServersConfig = (url: string) => JSON.stringify({
  mcpServers: {
    [IDE_SERVER_NAME]: { url },
  },
}, null, 2);
const tomlString = (value: string) => JSON.stringify(value);
const buildCodexConfig = (url: string) => [
  `[mcp_servers.${IDE_SERVER_NAME}]`,
  `url = ${tomlString(url)}`,
].join('\n');
const IDE_TARGETS: IdeTarget[] = [
  {
    id: 'claude',
    label: 'Claude Desktop',
    configPath: {
      windows: '%APPDATA%\\Claude\\claude_desktop_config.json',
      macos: '~/Library/Application Support/Claude/claude_desktop_config.json',
      linux: '~/.config/Claude/claude_desktop_config.json',
    },
    icon: claudeIcon,
    build: buildMcpServersConfig,
  },
  {
    id: 'cursor',
    label: 'Cursor',
    configPath: {
      windows: '%USERPROFILE%\\.cursor\\mcp.json',
      macos: '~/.cursor/mcp.json',
      linux: '~/.cursor/mcp.json',
    },
    icon: cursorIcon,
    build: buildMcpServersConfig,
  },
  {
    id: 'codebuddy',
    label: 'CodeBuddy',
    configPath: 'App settings -> Custom MCP Servers',
    icon: codebuddyIcon,
    build: buildMcpServersConfig,
  },
  {
    id: 'vscode',
    label: 'VS Code',
    configPath: {
      windows: '%APPDATA%\\Code\\User\\mcp.json',
      macos: '~/Library/Application Support/Code/User/mcp.json',
      linux: '~/.config/Code/User/mcp.json',
    },
    icon: vscodeIcon,
    build: buildMcpServersConfig,
  },
  {
    id: 'cline',
    label: 'Cline',
    configPath: 'App settings panel -> MCP servers',
    icon: clineIcon,
    build: buildMcpServersConfig,
  },
  {
    id: 'codex',
    label: 'Codex / OpenAI',
    configPath: {
      windows: '%USERPROFILE%\\.codex\\config.toml',
      macos: '~/.codex/config.toml',
      linux: '~/.codex/config.toml',
    },
    icon: openaiIcon,
    build: buildCodexConfig,
  },
];
type PanelDefinition = { id: Panel; labelKey: MessageKey; groupKey: MessageKey };

const PANELS: PanelDefinition[] = [
  { id: 'setup', labelKey: 'navigation.panel.setup', groupKey: 'navigation.group.onboarding' },
  { id: 'debug', labelKey: 'navigation.panel.debug', groupKey: 'navigation.group.operations' },
  { id: 'instances', labelKey: 'navigation.panel.instances', groupKey: 'navigation.group.operations' },
  { id: 'activity', labelKey: 'navigation.panel.activity', groupKey: 'navigation.group.operations' },
  { id: 'health', labelKey: 'navigation.panel.health', groupKey: 'navigation.group.operations' },
  { id: 'workflows', labelKey: 'navigation.panel.workflows', groupKey: 'navigation.group.workspace' },
  { id: 'tasks', labelKey: 'navigation.panel.tasks', groupKey: 'navigation.group.workspace' },
  { id: 'tools', labelKey: 'navigation.panel.tools', groupKey: 'navigation.group.workspace' },
  { id: 'openapi', labelKey: 'navigation.panel.openapi', groupKey: 'navigation.group.contracts' },
  { id: 'stats', labelKey: 'navigation.panel.stats', groupKey: 'navigation.group.observability' },
  { id: 'governance', labelKey: 'navigation.panel.governance', groupKey: 'navigation.group.observability' },
  { id: 'traffic', labelKey: 'navigation.panel.traffic', groupKey: 'navigation.group.observability' },
  { id: 'traces', labelKey: 'navigation.panel.traces', groupKey: 'navigation.group.observability' },
  { id: 'calls', labelKey: 'navigation.panel.calls', groupKey: 'navigation.group.observability' },
  { id: 'logs', labelKey: 'navigation.panel.logs', groupKey: 'navigation.group.observability' },
  { id: 'skill-paths', labelKey: 'navigation.panel.skillPaths', groupKey: 'navigation.group.configuration' },
];

const PANEL_ID_SET = new Set<Panel>(PANELS.map((p) => p.id));

const STATS_RANGE_IDS = new Set(['1h', '24h', '7d', 'all']);

function gatewayDocsHref(): string {
  return `${window.location.origin}/docs`;
}

function projectDocsHref(): string {
  return 'https://github.com/loonghao/dcc-mcp-core/tree/main/docs';
}

function gatewayOpenApiHref(): string {
  return `${window.location.origin}/v1/openapi.json`;
}

function gatewayOpenApiSource(): OpenApiSource {
  return {
    label: 'Gateway REST API',
    specUrl: gatewayOpenApiHref(),
    docsUrl: gatewayDocsHref(),
    inspectorUrl: fullHrefForAdmin('openapi'),
    kind: 'gateway',
  };
}

function isPanelId(value: string | null | undefined): value is Panel {
  return value != null && value !== '' && PANEL_ID_SET.has(value as Panel);
}

/** Admin HTML path without `/api` (honours custom `--admin-path`). */
function adminShellPath(): string {
  const { pathname } = window.location;
  let base = pathname.replace(/\/+$/, '');
  if (base.endsWith('/index.html')) {
    base = base.slice(0, -'/index.html'.length);
  }
  if (!base || base === '/') {
    base = '/admin';
  }
  return base.startsWith('/') ? base : `/${base}`;
}

/** Shareable relative URL: `/admin?panel=stats&range=7d`. */
function hrefForAdmin(panel: Panel, extra?: Record<string, string | undefined>): string {
  const u = new URL(`${window.location.origin}${adminShellPath()}`);
  u.searchParams.set('panel', panel);
  if (extra) {
    for (const [k, v] of Object.entries(extra)) {
      if (v != null && v !== '') {
        u.searchParams.set(k, v);
      }
    }
  }
  return `${u.pathname}${u.search}`;
}

function fullHrefForAdmin(panel: Panel, extra?: Record<string, string | undefined>): string {
  return new URL(hrefForAdmin(panel, extra), window.location.origin).toString();
}

function openApiInspectorHref(specUrl: string, docsUrl: string, label: string): string {
  return hrefForAdmin('openapi', { spec: specUrl, docs: docsUrl, label });
}

function readOpenApiSourceFromUrl(): OpenApiSource {
  const u = new URL(window.location.href);
  const spec = u.searchParams.get('spec')?.trim();
  if (!spec) {
    return gatewayOpenApiSource();
  }
  const docs = u.searchParams.get('docs')?.trim();
  const label = u.searchParams.get('label')?.trim();
  const specUrl = new URL(spec, window.location.origin).toString();
  return {
    label: label || 'Instance REST API',
    specUrl,
    docsUrl: docs ? new URL(docs, window.location.origin).toString() : specUrl.replace(/\/v1\/openapi\.json(?:[?#].*)?$/, '/docs'),
    inspectorUrl: fullHrefForAdmin('openapi', { spec: specUrl, docs: docs ?? undefined, label: label ?? undefined }),
    kind: 'instance',
  };
}

function traceLinks(requestId: string, provided?: AdminLinks): AdminLinks {
  const encoded = encodeURIComponent(requestId);
  return {
    admin_trace_url: provided?.admin_trace_url ?? fullHrefForAdmin('traces', { trace: requestId }),
    trace_api_url: provided?.trace_api_url ?? `${API_BASE}/traces/${encoded}`,
    agent_trace_packet_url: provided?.agent_trace_packet_url ?? `${window.location.origin}/v1/debug/agent-traces/${encoded}`,
    debug_bundle_url: provided?.debug_bundle_url ?? `${API_BASE}/debug-bundle/${encoded}`,
    issue_report_url: provided?.issue_report_url ?? `${API_BASE}/issue-report/${encoded}`,
    openapi_inspector_url: provided?.openapi_inspector_url ?? fullHrefForAdmin('openapi'),
    openapi_spec_url: provided?.openapi_spec_url ?? gatewayOpenApiHref(),
    openapi_docs_url: provided?.openapi_docs_url ?? gatewayDocsHref(),
    stats_url: provided?.stats_url ?? fullHrefForAdmin('stats', { range: readStatsRangeFromUrl() }),
    admin_traces_url: provided?.admin_traces_url ?? fullHrefForAdmin('traces'),
  };
}

function adminPathBases(): { adminBase: string; apiBase: string } {
  try {
    const apiBase = new URL(API_BASE).pathname.replace(/\/+$/, '') || '/admin/api';
    const adminBase = apiBase.endsWith('/api') ? apiBase.slice(0, -'/api'.length) || '/admin' : '/admin';
    return { adminBase, apiBase };
  } catch {
    return { adminBase: '/admin', apiBase: '/admin/api' };
  }
}

function publicSafeIssuePaths(requestId: string): Record<string, string> {
  const encoded = encodeURIComponent(requestId);
  const { adminBase, apiBase } = adminPathBases();
  return {
    admin_trace_path: `${adminBase}?panel=traces&trace=${encoded}`,
    trace_api_path: `${apiBase}/traces/${encoded}`,
    safe_issue_report_path: `${apiBase}/issue-report/${encoded}`,
    raw_issue_report_path: `${apiBase}/issue-report/${encoded}?mode=raw`,
    stable_safe_issue_report_path: `/v1/debug/issue-reports/${encoded}`,
    stable_raw_issue_report_path: `/v1/debug/issue-reports/${encoded}?mode=raw`,
    openapi_spec_path: '/v1/openapi.json',
    docs_path: '/docs',
  };
}

function publicToolFamily(tool: string | null | undefined, method: string): string {
  const raw = tool || method || 'unknown';
  const lastSegment = raw.split('.').filter(Boolean).pop() || raw;
  const family = lastSegment.includes('__') ? lastSegment.split('__').pop() || lastSegment : lastSegment;
  return family.replace(/[^A-Za-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'unknown';
}

function readPanelFromUrl(): Panel {
  const u = new URL(window.location.href);
  const raw = u.searchParams.get('panel');
  return isPanelId(raw) ? raw : 'setup';
}

function readStatsRangeFromUrl(): string {
  const u = new URL(window.location.href);
  const r = u.searchParams.get('range');
  return r && STATS_RANGE_IDS.has(r) ? r : '24h';
}

function readTraceIdFromUrl(): string | null {
  const u = new URL(window.location.href);
  const t = u.searchParams.get('trace');
  return t != null && t.trim() !== '' ? t.trim() : null;
}

function haystack(...parts: (string | number | null | undefined)[]): string {
  return parts
    .filter((p) => p != null && p !== '')
    .map((p) => String(p))
    .join(' ')
    .toLowerCase();
}

function matchesListFilter(query: string, hay: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) {
    return true;
  }
  return hay.includes(q);
}

function isLoopbackHost(hostname: string): boolean {
  const h = hostname.toLowerCase();
  return h === 'localhost' || h === '127.0.0.1' || h === '::1' || h === '[::1]';
}

function backendAccessUrls(mcpUrl: string): { origin: string; mcp: string; docs: string; openapi: string } {
  const u = new URL(mcpUrl);
  if (isLoopbackHost(u.hostname)) {
    u.hostname = window.location.hostname;
  }
  const origin = u.origin;
  return { origin, mcp: u.toString(), docs: `${origin}/docs`, openapi: `${origin}/v1/openapi.json` };
}

function urlHost(host: string): string {
  const trimmed = host.trim();
  if (trimmed === '0.0.0.0' || trimmed === '::') {
    return window.location.hostname;
  }
  if (trimmed.includes(':') && !trimmed.startsWith('[') && !trimmed.endsWith(']')) {
    return `[${trimmed}]`;
  }
  return trimmed;
}

function gatewaySentinelMcpUrl(sentinel: GatewaySentinel | null | undefined): string | null {
  if (!sentinel || !sentinel.host || !Number.isFinite(sentinel.port) || sentinel.port <= 0) {
    return null;
  }
  try {
    return new URL('/mcp', `http://${urlHost(sentinel.host)}:${sentinel.port}`).toString();
  } catch {
    return null;
  }
}

function configuredDevGatewayMcpUrl(): string | null {
  if (!import.meta.env.DEV || !isLoopbackHost(window.location.hostname)) {
    return null;
  }
  const configured = String(import.meta.env.VITE_DCC_MCP_GATEWAY_URL ?? '').trim();
  try {
    if (configured) {
      return new URL('/mcp', configured).toString();
    }
    return new URL('/mcp', `${window.location.protocol}//${window.location.hostname}:${DEFAULT_LOCAL_GATEWAY_PORT}`).toString();
  } catch {
    return null;
  }
}

function gatewayMcpUrl(health: HealthPayload | null): string {
  return gatewaySentinelMcpUrl(health?.gateway?.current)
    ?? configuredDevGatewayMcpUrl()
    ?? new URL('/mcp', window.location.origin).toString();
}

function gatewayMcpUrlFromPage(): string {
  return new URL('/mcp', window.location.origin).toString();
}

function lanGatewayMcpUrl(): string | null {
  if (isLoopbackHost(window.location.hostname)) {
    return null;
  }
  return gatewayMcpUrlFromPage();
}

function instanceSetupLabel(instance: InstanceRow): string {
  return `${instance.display_name || instance.dcc_type} (${instance.instance_id.slice(0, 8)})`;
}

function detectClientPlatform(): ClientPlatform {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const primaryPlatform = `${nav.userAgentData?.platform ?? navigator.platform ?? ''}`.toLowerCase();
  if (primaryPlatform.includes('win')) {
    return 'windows';
  }
  if (primaryPlatform.includes('mac')) {
    return 'macos';
  }
  if (primaryPlatform.includes('linux') || primaryPlatform.includes('x11')) {
    return 'linux';
  }

  const userAgent = `${navigator.userAgent ?? ''}`.toLowerCase();
  if (userAgent.includes('win')) {
    return 'windows';
  }
  if (userAgent.includes('mac')) {
    return 'macos';
  }
  return 'linux';
}

function configPathForTarget(target: IdeTarget, platform: ClientPlatform): string {
  if (typeof target.configPath === 'string') {
    return target.configPath;
  }
  return target.configPath[platform] ?? target.configPath.linux;
}

function ideConfigText(target: IdeTarget, url: string): string {
  return target.build(url);
}

function configPathFileUrl(path: string): string | null {
  if (path.startsWith('%') || path.startsWith('~') || path.includes('->')) {
    return null;
  }
  const normalized = path.replace(/\\/g, '/');
  return normalized.match(/^[A-Za-z]:\//) ? `file:///${normalized}` : null;
}

function instanceOpenApiSource(instance: InstanceRow): OpenApiSource {
  const urls = backendAccessUrls(instance.mcp_url);
  const label = `${instance.display_name || instance.dcc_type} ${instance.instance_id.slice(0, 8)}`;
  return {
    label,
    specUrl: urls.openapi,
    docsUrl: urls.docs,
    inspectorUrl: new URL(openApiInspectorHref(urls.openapi, urls.docs, label), window.location.origin).toString(),
    kind: 'instance',
  };
}

function BackendAccessUrl({ mcpUrl }: { mcpUrl: string }) {
  try {
    const urls = backendAccessUrls(mcpUrl);
    return (
      <a className="mono-path" href={urls.origin} target="_blank" rel="noopener noreferrer">
        {urls.origin}
      </a>
    );
  } catch {
    return <span className="mono-path">{mcpUrl}</span>;
  }
}

/** Open host root, MCP endpoint, and `/docs` on the DCC HTTP server (same origin as MCP). */
function McpBackendLinks({ mcpUrl }: { mcpUrl: string }) {
  try {
    const urls = backendAccessUrls(mcpUrl);
    return (
      <span className="mcp-backend-links">
        <a href={urls.origin} target="_blank" rel="noopener noreferrer">host</a>
        {' · '}
        <a href={urls.mcp} target="_blank" rel="noopener noreferrer">MCP</a>
        {' · '}
        <a href={urls.docs} target="_blank" rel="noopener noreferrer">docs</a>
      </span>
    );
  } catch {
    return <span className="mono-path">{mcpUrl}</span>;
  }
}

async function apiJson<T>(path: string): Promise<T> {
  const ctrl = new AbortController();
  const tid = window.setTimeout(() => ctrl.abort(), ADMIN_FETCH_TIMEOUT_MS);
  try {
    const response = await fetch(`${API_BASE}${path}`, { signal: ctrl.signal });
    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }
    return (await response.json()) as T;
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') {
      throw new Error(`Request timed out after ${ADMIN_FETCH_TIMEOUT_MS / 1000}s`);
    }
    throw err;
  } finally {
    clearTimeout(tid);
  }
}

function BackendOpenApiLinks({ instance }: { instance: InstanceRow }) {
  try {
    const source = instanceOpenApiSource(instance);
    return (
      <span className="mcp-backend-links openapi-backend-links">
        <a href={source.inspectorUrl}>Inspector</a>
        {' · '}
        <a href={source.specUrl} target="_blank" rel="noopener noreferrer">spec</a>
        {' · '}
        <a href={source.docsUrl} target="_blank" rel="noopener noreferrer">docs</a>
      </span>
    );
  } catch {
    return <span className="mono-path">{instance.mcp_url}</span>;
  }
}

async function issueReportJsonText(requestId: string): Promise<string> {
  const payload = await apiJson<unknown>(`/issue-report/${encodeURIComponent(requestId)}`);
  return JSON.stringify(payload, null, 2);
}

function issueReportFilename(requestId: string): string {
  const safe = requestId.replace(/[^A-Za-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'request';
  return `dcc-mcp-issue-report-${safe}.json`;
}

function openApiSpecFilename(label: string): string {
  const safe = label.replace(/[^A-Za-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'gateway';
  return `dcc-mcp-openapi-${safe}.json`;
}

function downloadJsonText(filename: string, text: string): void {
  const blob = new Blob([text], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

async function fetchOpenApiSpecText(specUrl: string): Promise<{ spec: OpenApiSpec; raw: string }> {
  const ctrl = new AbortController();
  const tid = window.setTimeout(() => ctrl.abort(), ADMIN_FETCH_TIMEOUT_MS);
  try {
    const response = await fetch(specUrl, { signal: ctrl.signal });
    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }
    const text = await response.text();
    const spec = JSON.parse(text) as OpenApiSpec;
    return { spec, raw: JSON.stringify(spec, null, 2) };
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') {
      throw new Error(`Request timed out after ${ADMIN_FETCH_TIMEOUT_MS / 1000}s`);
    }
    throw err;
  } finally {
    clearTimeout(tid);
  }
}

function flattenOpenApiOperations(spec: OpenApiSpec | null): OpenApiOperationRow[] {
  const rows: OpenApiOperationRow[] = [];
  const paths = spec?.paths ?? {};
  for (const [path, rawPathItem] of Object.entries(paths)) {
    if (!rawPathItem || typeof rawPathItem !== 'object' || Array.isArray(rawPathItem)) {
      continue;
    }
    for (const [method, rawOperation] of Object.entries(rawPathItem as Record<string, unknown>)) {
      const methodKey = method.toLowerCase();
      if (!OPENAPI_METHODS.has(methodKey) || !rawOperation || typeof rawOperation !== 'object' || Array.isArray(rawOperation)) {
        continue;
      }
      const operation = rawOperation as OpenApiOperationObject;
      const responseCodes = Object.keys(operation.responses ?? {});
      const tags = Array.isArray(operation.tags) ? operation.tags.filter((tag): tag is string => typeof tag === 'string') : [];
      const operationId = operation.operationId ?? `${methodKey}_${path.replace(/[^A-Za-z0-9]+/g, '_').replace(/^_+|_+$/g, '')}`;
      rows.push({
        key: `${methodKey.toUpperCase()} ${path}`,
        method: methodKey.toUpperCase(),
        path,
        operationId,
        summary: operation.summary ?? operation.description ?? '',
        tags,
        responseCodes,
        hasRequestBody: operation.requestBody != null,
        parameterCount: Array.isArray(operation.parameters) ? operation.parameters.length : 0,
      });
    }
  }
  return rows.sort((a, b) => a.path.localeCompare(b.path) || a.method.localeCompare(b.method));
}

function formatUptime(value: number | null | undefined): string {
  if (value == null) {
    return '-';
  }
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const seconds = value % 60;
  return `${hours}h ${minutes}m ${seconds}s`;
}

function formatBytes(value: number | null | undefined): string {
  if (value == null) {
    return '-';
  }
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let index = 0;
  let size = value;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size.toFixed(1)} ${units[index]}`;
}

function totalTraceTokens(row: TraceRow): number | null {
  if (row.total_tokens != null) {
    return row.total_tokens;
  }
  if (row.input_tokens == null && row.output_tokens == null) {
    return null;
  }
  return (row.input_tokens ?? 0) + (row.output_tokens ?? 0);
}

function detailTraceTokens(trace: TraceDetailPayload): {
  inputTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  estimatedTokens: number | null;
  estimator: string | null;
} {
  const inputTokens = trace.input_tokens ?? trace.input?.estimated_tokens ?? null;
  const outputTokens = trace.output_tokens ?? trace.output?.estimated_tokens ?? null;
  const totalTokens = (() => {
    if (trace.total_tokens != null) {
      return trace.total_tokens;
    }
    if (inputTokens == null && outputTokens == null) {
      return null;
    }
    return (inputTokens ?? 0) + (outputTokens ?? 0);
  })();
  const estimatedTokens = trace.estimated_total_tokens ?? trace.estimated_tokens ?? totalTokens;
  const estimator = trace.payload_token_estimator ?? trace.token_accounting?.token_estimator ?? trace.token_estimator ?? null;
  return { inputTokens, outputTokens, totalTokens, estimatedTokens, estimator };
}

function statusClass(value: string): string {
  const status = value.toLowerCase();
  if (status.includes('fail') || status.includes('error') || status.includes('err') || status.includes('rejected') || status.includes('cancel')) {
    return 'badge badge-err';
  }
  if (status.includes('ok') || status.includes('success') || status.includes('complete') || status.includes('completed') || status.includes('done') || status.includes('ready') || status.includes('available')) {
    return 'badge badge-ok';
  }
  if (status.includes('stale') || status.includes('booting') || status.includes('warn') || status.includes('zero') || status.includes('pending') || status.includes('running') || status.includes('busy') || status.includes('queued')) {
    return 'badge badge-warn';
  }
  return 'badge badge-muted';
}

function isOkStatus(value: string | null | undefined): boolean {
  return statusClass(value ?? '').includes('badge-ok');
}

function isErrStatus(value: string | null | undefined): boolean {
  return statusClass(value ?? '').includes('badge-err');
}

function isWarnStatus(value: string | null | undefined): boolean {
  return statusClass(value ?? '').includes('badge-warn');
}

function StatusBadge({ value }: { value: string }) {
  return <span className={statusClass(value)}>{value}</span>;
}

function TimeValue({ value, className }: { value: string | null | undefined; className?: string }) {
  const title = timestampTitle(value);
  const text = formatTime(value);
  if (!title) {
    return <span className={className}>{text}</span>;
  }
  return <time className={className} dateTime={title} title={title}>{text}</time>;
}

function StatusLine({ text, error }: { text: string; error?: string }) {
  return <div className="status-bar">{error ? `Error: ${error}` : text}</div>;
}

function HealthCard({ tone, label, value }: { tone?: 'ok' | 'warn'; label: string; value: string | number }) {
  return (
    <div className={`health-card ${tone ?? ''}`}>
      <div className="label">{label}</div>
      <div className="value">{value}</div>
    </div>
  );
}

function MetricTile({ tone, label, value, detail }: { tone?: 'ok' | 'warn' | 'err'; label: string; value: string | number; detail?: string }) {
  return (
    <div className={`metric-tile ${tone ?? ''}`}>
      <div className="metric-label">{label}</div>
      <div className="metric-value">{value}</div>
      {detail ? <div className="metric-detail">{detail}</div> : null}
    </div>
  );
}

function PanelHeader({ title, meta, action }: { title: string; meta?: string; action?: ReactNode }) {
  return (
    <div className="panel-header">
      <div>
        <h2>{title}</h2>
        {meta ? <p className="panel-meta">{meta}</p> : null}
      </div>
      {action ? <div className="panel-actions">{action}</div> : null}
    </div>
  );
}

function NavIcon({ panel }: { panel: Panel }) {
  const icons: Record<Panel, string[]> = {
    setup: ['M5 12h10', 'M13 8l4 4-4 4', 'M4 5h16v14H4z'],
    debug: ['M6 6h12v12H6z', 'M9 9h6v6H9z'],
    activity: ['M4 14h4l2-5 4 9 2-4h4'],
    health: ['M12 4l7 4v5c0 4-3 7-7 8-4-1-7-4-7-8V8z'],
    instances: ['M6 7h12v10H6z', 'M9 4h6', 'M9 20h6'],
    tools: ['M5 19l5-5', 'M14 5l5 5', 'M12 7l5 5-5 5-5-5z'],
    workflows: ['M5 6h4v4H5z', 'M15 6h4v4h-4z', 'M5 15h4v4H5z', 'M9 8h6', 'M7 10v5'],
    tasks: ['M6 7h12', 'M6 12h12', 'M6 17h12', 'M4 7h.01', 'M4 12h.01', 'M4 17h.01'],
    calls: ['M7 7h10v10H7z', 'M10 10h4v4h-4z'],
    traces: ['M5 7h4v4H5z', 'M15 13h4v4h-4z', 'M9 9l6 6'],
    traffic: ['M4 8h4l2 8 4-12 2 8h4', 'M4 16h4', 'M16 16h4'],
    stats: ['M5 18V9', 'M12 18V5', 'M19 18v-6', 'M4 18h16'],
    governance: ['M12 4l7 4v5c0 4-3 7-7 8-4-1-7-4-7-8V8z', 'M9 12l2 2 4-5'],
    logs: ['M7 5h8l3 3v11H7z', 'M15 5v4h4', 'M10 13h6', 'M10 16h5'],
    openapi: ['M5 5h14v14H5z', 'M8 9h8', 'M8 13h5', 'M8 17h8'],
    'skill-paths': ['M5 12h14', 'M12 5v14', 'M7 7l10 10', 'M17 7L7 17'],
  };
  return (
    <svg className="nav-icon" viewBox="0 0 24 24" aria-hidden="true">
      {icons[panel].map((d) => <path key={d} d={d} />)}
    </svg>
  );
}

function IdeIcon({ target }: { target: IdeTarget }) {
  return (
    <img className={`ide-icon ide-icon-${target.id}`} src={target.icon} alt="" aria-hidden="true" />
  );
}

function DocsIcon() {
  return (
    <svg className="nav-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M6 4h9l3 3v13H6z" />
      <path d="M15 4v4h4" />
      <path d="M9 13h6" />
      <path d="M9 16h5" />
    </svg>
  );
}

function EmptyRow({ columns, children }: { columns: number; children: string }) {
  return (
    <tr>
      <td colSpan={columns} className="empty">{children}</td>
    </tr>
  );
}

type SkillDetailToolSummary = {
  name: string;
  summary?: string;
  annotations: string[];
};

function skillDetailTools(detail: SkillDetailInstance | null | undefined): SkillDetailToolSummary[] {
  if (!Array.isArray(detail?.tools)) {
    return [];
  }
  return detail.tools
    .map((tool) => {
      if (typeof tool === 'string') {
        return { name: tool, annotations: [] };
      }
      const o = recordOrNull(tool);
      const name = String(o?.name ?? o?.tool_slug ?? o?.id ?? '');
      const annotations = recordOrNull(o?.annotations) ?? recordOrNull(o?.tool_annotations);
      const labels = [
        annotations?.readOnlyHint === true || annotations?.read_only === true ? 'read-only' : '',
        annotations?.destructiveHint === true || annotations?.destructive === true ? 'destructive' : '',
        annotations?.idempotentHint === true || annotations?.idempotent === true ? 'idempotent' : '',
        o?.thread_affinity != null ? `thread:${String(o.thread_affinity)}` : '',
        o?.affinity != null ? `affinity:${String(o.affinity)}` : '',
      ].filter(Boolean);
      return {
        name,
        summary: o?.description == null && o?.summary == null ? undefined : String(o.description ?? o.summary),
        annotations: labels,
      };
    })
    .filter((tool) => tool.name);
}

function SkillDetailPanel({
  skill,
  detail,
  busy,
  onReload,
  onClose,
  t,
}: {
  skill: SkillRow;
  detail: SkillDetailPayload | null;
  busy: boolean;
  onReload: () => void;
  onClose: () => void;
  t: Translator;
}) {
  const selected = detail?.skill ?? detail?.instances?.[0] ?? null;
  const tools = skillDetailTools(selected);
  const dccLabel = selected?.dcc_type ?? selected?.dcc ?? skill.dcc_type;
  const instanceCount = detail?.instances?.length || skill.instance_count || (selected?.instance_id ? 1 : 0);
  return (
    <section className="skill-detail-panel" aria-live="polite">
      <div className="skill-detail-heading">
        <div>
          <h3>{selected?.name ?? skill.name}</h3>
          <div className="skill-detail-meta">
            <span className="source-pill">{dccLabel || t('common.status.unknown')}</span>
            <span className={`badge ${skill.loaded ? 'badge-ok' : 'badge-muted'}`}>{selected?.state ?? (skill.loaded ? t('skillPaths.state.loaded') : t('skillPaths.state.unloaded'))}</span>
            {selected?.instance_short ? <span className="mono-path">{t('skillPaths.label.instance', { id: selected.instance_short })}</span> : null}
          </div>
        </div>
        <div className="table-actions">
          <button className="refresh-btn" type="button" disabled={busy} onClick={onReload}>
            {busy ? t('common.status.loading') : t('common.action.reload')}
          </button>
          <button className="linkish" type="button" onClick={onClose}>{t('common.action.close')}</button>
        </div>
      </div>
      {selected?.description ? <p className="skill-detail-description">{selected.description}</p> : null}
      {selected?.skill_md_path ? <div className="mono-path skill-detail-path">{selected.skill_md_path}</div> : null}
      <div className="skill-detail-summary-grid">
        <span><strong>{t('skillPaths.table.state')}</strong>{selected?.state ?? (skill.loaded ? t('skillPaths.state.loaded') : t('skillPaths.state.unloaded'))}</span>
        <span><strong>{t('skillPaths.metric.actions')}</strong>{tools.length || skill.action_count}</span>
        <span><strong>{t('skillPaths.table.instances')}</strong>{instanceCount}</span>
        <span><strong>DCC</strong>{dccLabel || t('common.status.unknown')}</span>
      </div>
      {detail?.error || selected?.error ? <p className="empty skill-detail-error">{detail?.error ?? selected?.error}</p> : null}
      {selected?.message ? <p className="empty">{selected.message}</p> : null}
      {tools.length > 0 ? (
        <div className="skill-tool-list">
          {tools.map((tool) => (
            <div className="skill-tool-row" key={tool.name}>
              <code title={tool.name}>{tool.name}</code>
              {tool.summary ? <span>{tool.summary}</span> : null}
              {tool.annotations.length > 0 ? (
                <div className="skill-tool-annotations">
                  {tool.annotations.map((label) => <span className="source-pill" key={`${tool.name}-${label}`}>{label}</span>)}
                </div>
              ) : null}
            </div>
          ))}
        </div>
      ) : null}
      <SkillMarkdownPreview
        markdown={selected?.markdown}
        frontmatterLabel={t('skillPaths.label.frontmatter')}
        noMarkdownLabel={t('skillPaths.detail.noMarkdown')}
        noBodyLabel={t('skillPaths.detail.noBody')}
        copyLabel={t('common.action.copy')}
        copiedLabel={t('skillPaths.action.copiedCode')}
      />
      {detail?.instances && detail.instances.length > 1 ? (
        <div className="skill-detail-instances">
          {detail.instances.map((instance) => (
            <span className="source-pill" key={`${instance.instance_id ?? instance.instance_short ?? instance.name}`}>
              {instance.dcc_type ?? instance.dcc ?? skill.dcc_type}:{instance.instance_short ?? compactId(instance.instance_id)}
            </span>
          ))}
        </div>
      ) : null}
    </section>
  );
}

function appTypeLabel(value: string | null | undefined): string {
  const app = (value ?? 'unknown').trim() || 'unknown';
  return `app-type: ${app}`;
}

function compactInstanceId(value: string | null | undefined): string {
  if (typeof value !== 'string' || value.length === 0) {
    return 'unrouted';
  }
  return value.length > 8 ? value.slice(0, 8) : value;
}

function toolInstanceLabel(tool: ToolRow): string {
  return tool.instance_prefix ?? compactInstanceId(tool.instance_id);
}

function toolGroupLabel(tool: ToolRow): string {
  return appTypeLabel(tool.dcc_type);
}

function instanceGroupLabel(instance: InstanceRow): string {
  return appTypeLabel(instance.dcc_type);
}

function callGroupLabel(call: CallRow): string {
  return appTypeLabel(call.dcc_type);
}

function traceGroupLabel(trace: TraceRow): string {
  return appTypeLabel(trace.dcc_type);
}

function maxTopCount(items: TopEntry[]): number {
  if (!items.length) {
    return 1;
  }
  return Math.max(1, ...items.map((i) => i.count));
}

function latencyTone(value: number | null | undefined): 'ok' | 'warn' | undefined {
  if (value == null) {
    return undefined;
  }
  return latencySeverity(value) ? 'warn' : 'ok';
}

function latencySeverity(value: number | null | undefined): LatencySeverity | null {
  if (value == null) {
    return null;
  }
  if (value >= CRITICAL_LATENCY_MS) {
    return 'critical';
  }
  if (value >= SLOW_LATENCY_MS) {
    return 'slow';
  }
  return null;
}

function isSlowLatency(value: number | null | undefined): boolean {
  return latencySeverity(value) != null;
}

function latencyClass(value: number | null | undefined): string {
  const severity = latencySeverity(value);
  return severity ? `latency-${severity}` : '';
}

function latencyBadgeKey(severity: LatencySeverity): MessageKey {
  return severity === 'critical' ? 'common.badge.tail' : 'common.badge.slow';
}

function LatencyBadge({ value, t }: { value: number | null | undefined; t: Translator }) {
  const severity = latencySeverity(value);
  if (!severity) {
    return null;
  }
  return <span className={`badge badge-latency badge-latency-${severity}`}>{t(latencyBadgeKey(severity))}</span>;
}

function LatencyValue({ value, t }: { value: number | null | undefined; t: Translator }) {
  return (
    <span className="latency-value">
      <span>{formatDurationMs(value)}</span>
      <LatencyBadge value={value} t={t} />
    </span>
  );
}

function errorRateTone(stats: StatsPayload | null): 'ok' | 'warn' | undefined {
  if (!stats || stats.total_calls === 0) {
    return undefined;
  }
  return stats.success_rate < 95 ? 'warn' : 'ok';
}

function traceLatency(trace: TraceRow): number {
  return trace.total_ms ?? -1;
}

function spanDurationMs(span: TraceSpan): number {
  return Math.round((span.duration_ns ?? 0) / 1_000_000);
}

function agentLabel(row: { agent_name?: string | null; agent_id?: string | null; agent_model?: string | null }): string {
  return row.agent_name || row.agent_id || row.agent_model || '-';
}

function actorLabel(row: {
  actor?: string | null;
  actor_name?: string | null;
  actor_id?: string | null;
  auth_subject?: string | null;
  actor_email_hash?: string | null;
}): string {
  return row.actor || row.actor_name || row.actor_id || row.auth_subject || row.actor_email_hash || '-';
}

function platformLabel(row: { client_platform?: string | null; client_os?: string | null; client_host?: string | null }): string {
  return [row.client_platform, row.client_os, row.client_host].filter(Boolean).join(' / ') || '-';
}

function sourceIpLabel(row: { source_ip?: string | null }): string {
  return row.source_ip || '-';
}

function trustFor(row: { attribution_trust?: AttributionTrust | null; trust?: AttributionTrust | null }, field: keyof AttributionTrust): string | null {
  return row.attribution_trust?.[field] ?? row.trust?.[field] ?? null;
}

function firstTrust(row: { attribution_trust?: AttributionTrust | null; trust?: AttributionTrust | null }, fields: (keyof AttributionTrust)[]): string | null {
  for (const field of fields) {
    const value = trustFor(row, field);
    if (value) {
      return value;
    }
  }
  return null;
}

function trustChip(source: string | null | undefined): ReactNode {
  return source ? <span className="trust-chip" title={`trust: ${source}`}>{source}</span> : null;
}

function safeCallerContext(agent: AgentContext | null | undefined): Record<string, unknown> | null {
  if (!agent) {
    return null;
  }
  return {
    actor_id: agent.actor_id ?? null,
    actor_name: agent.actor_name ?? null,
    actor_email_hash: agent.actor_email_hash ?? null,
    agent_id: agent.agent_id ?? null,
    agent_name: agent.agent_name ?? null,
    agent_kind: agent.agent_kind ?? null,
    agent_version: agent.agent_version ?? null,
    model_provider: agent.model_provider ?? null,
    model_version: agent.model_version ?? null,
    model: agent.model ?? null,
    reasoning_effort: agent.reasoning_effort ?? null,
    session_id: agent.session_id ?? null,
    turn_id: agent.turn_id ?? null,
    client_platform: agent.client_platform ?? null,
    client_os: agent.client_os ?? null,
    client_host: agent.client_host ?? null,
    auth_subject: agent.auth_subject ?? null,
    source_ip: agent.source_ip ?? null,
    forwarded_for: agent.forwarded_for ?? [],
    trust: agent.trust ?? {},
    task: agent.task ?? null,
    user_intent_summary: agent.user_intent_summary ?? null,
    agent_reply_summary: agent.agent_reply_summary ?? null,
    user_input_hash: agent.user_input_hash ?? null,
    agent_reply_hash: agent.agent_reply_hash ?? null,
    user_input_chars: agent.user_input_chars ?? null,
    agent_reply_chars: agent.agent_reply_chars ?? null,
    plan: agent.plan ?? [],
    observations: agent.observations ?? [],
    tags: agent.tags ?? [],
    parent_request_id: agent.parent_request_id ?? null,
    trace_id: agent.trace_id ?? null,
    turn_index: agent.turn_index ?? null,
  };
}

function publicSafeCallerContext(agent: AgentContext | null | undefined): Record<string, unknown> | null {
  if (!agent) {
    return null;
  }
  return {
    agent_kind: agent.agent_kind ?? null,
    model_provider: agent.model_provider ?? null,
    model_version: agent.model_version ?? null,
    reasoning_effort: agent.reasoning_effort ?? null,
    client_platform: agent.client_platform ?? null,
    plan_step_count: agent.plan?.length ?? 0,
    observation_count: agent.observations?.length ?? 0,
    has_user_intent_summary: Boolean(agent.user_intent_summary),
    has_agent_reply_summary: Boolean(agent.agent_reply_summary),
  };
}

function tokenAccounting(row: TokenCarrier | null | undefined): TokenAccounting | null {
  if (!row) {
    return null;
  }
  if (row.token_accounting) {
    return row.token_accounting;
  }
  if (
    row.response_format ||
    row.token_estimator ||
    row.original_tokens != null ||
    row.returned_tokens != null ||
    row.saved_tokens != null ||
    row.savings_pct != null
  ) {
    return {
      response_format: row.response_format,
      token_estimator: row.token_estimator,
      original_bytes: row.original_bytes,
      returned_bytes: row.returned_bytes,
      original_tokens: row.original_tokens,
      returned_tokens: row.returned_tokens,
      saved_tokens: row.saved_tokens,
      savings_pct: row.savings_pct,
    };
  }
  return null;
}

function numericValue(value: number | string | null | undefined): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function formatTokenCount(value: number | string | null | undefined): string {
  const n = numericValue(value);
  if (n == null) {
    return '-';
  }
  return Math.round(n).toLocaleString();
}

function formatSavingsPct(value: number | string | null | undefined): string {
  const n = numericValue(value);
  if (n == null) {
    return '-';
  }
  return `${n.toFixed(1)}%`;
}

function responseFormatLabel(row: TokenCarrier | null | undefined): string {
  return tokenAccounting(row)?.response_format || '-';
}

function returnedTokensLabel(row: TokenCarrier | null | undefined): string {
  return formatTokenCount(tokenAccounting(row)?.returned_tokens);
}

function savedTokensLabel(row: TokenCarrier | null | undefined): string {
  const tokens = tokenAccounting(row);
  if (!tokens) {
    return '-';
  }
  return `${formatTokenCount(tokens.saved_tokens)} (${formatSavingsPct(tokens.savings_pct)})`;
}

function formatDurationMs(value: number | null | undefined): string {
  if (value == null) {
    return '-';
  }
  if (value < 1_000) {
    return `${value} ms`;
  }
  return `${(value / 1_000).toFixed(2)} s`;
}

function compactId(value: string | null | undefined): string {
  if (!value) {
    return '-';
  }
  return value.length > 12 ? value.slice(0, 12) : value;
}

function trafficTimestamp(frame: TrafficFrameEnvelope): string | undefined {
  if (typeof frame.timestamp_ns === 'number') {
    return new Date(frame.timestamp_ns / 1_000_000).toISOString();
  }
  return undefined;
}

function trafficMethod(frame: TrafficFrameEnvelope): string {
  return frame.attributes?.mcp?.method ?? '-';
}

function trafficRequestId(frame: TrafficFrameEnvelope): string | undefined {
  return frame.correlation?.request_id;
}

function trafficSessionId(frame: TrafficFrameEnvelope): string | undefined {
  return frame.attributes?.session_id ?? frame.correlation?.session_id;
}

function trafficBodyBytes(frame: TrafficFrameEnvelope): number | undefined {
  return frame.attributes?.body?.size_bytes;
}

function trafficRedactedPaths(frame: TrafficFrameEnvelope): string[] {
  return frame.attributes?.body?.redacted_paths ?? [];
}

function trafficFrameDetail(frame: TrafficFrameEnvelope): string {
  return JSON.stringify(frame, null, 2);
}

function trafficStatusLabelKey(status: TrafficCaptureStatus | null | undefined): MessageKey {
  switch (status?.state) {
    case 'captured':
      return 'traffic.status.captured';
    case 'capture_disabled':
      return 'traffic.status.disabled';
    case 'capture_unavailable':
      return 'traffic.status.unavailable';
    case 'capture_filtered':
      return 'traffic.status.filtered';
    case 'no_traffic':
      return 'traffic.status.noTraffic';
    default:
      return 'traffic.status.unknown';
  }
}

function trafficStatusDetailKey(status: TrafficCaptureStatus | null | undefined): MessageKey {
  switch (status?.state) {
    case 'captured':
      return 'traffic.statusDetail.captured';
    case 'capture_disabled':
      return 'traffic.statusDetail.disabled';
    case 'capture_unavailable':
      return 'traffic.statusDetail.unavailable';
    case 'capture_filtered':
      return 'traffic.statusDetail.filtered';
    case 'no_traffic':
      return 'traffic.statusDetail.noTraffic';
    default:
      return 'traffic.statusDetail.unknown';
  }
}

function trafficEmptyKey(status: TrafficCaptureStatus | null | undefined): MessageKey {
  switch (status?.state) {
    case 'capture_disabled':
      return 'traffic.empty.disabled';
    case 'capture_unavailable':
      return 'traffic.empty.unavailable';
    case 'capture_filtered':
      return 'traffic.empty.filtered';
    case 'no_traffic':
      return 'traffic.empty.noTraffic';
    default:
      return 'traffic.empty.none';
  }
}

function trafficStatusTone(status: TrafficCaptureStatus | null | undefined): 'ok' | 'warn' | 'err' | undefined {
  switch (status?.state) {
    case 'captured':
    case 'no_traffic':
      return 'ok';
    case 'capture_disabled':
    case 'capture_unavailable':
    case 'capture_filtered':
      return 'warn';
    default:
      return undefined;
  }
}

function compactList(values: string[] | null | undefined, empty = 'Any'): string {
  const clean = (values ?? []).filter(Boolean);
  if (!clean.length) {
    return empty;
  }
  if (clean.length <= 3) {
    return clean.join(', ');
  }
  return `${clean.slice(0, 3).join(', ')} +${clean.length - 3}`;
}

function taskPrimaryRequestId(task: TaskRow): string | null {
  return task.correlation?.request_id ?? task.related?.request_ids?.[0] ?? null;
}

function taskActorLabel(task: TaskRow): string {
  return task.correlation?.actor_name
    ?? task.correlation?.actor_id
    ?? task.correlation?.agent_id
    ?? task.correlation?.client_platform
    ?? '-';
}

function taskWorkflowLabel(task: TaskRow): string {
  const workflows = task.related?.workflow_ids ?? [];
  if (workflows.length > 0) {
    return compactList(workflows.map(compactId), '-');
  }
  return compactId(task.correlation?.workflow_id);
}

function taskRequestCount(task: TaskRow): number {
  return task.related?.request_ids?.length ?? (task.correlation?.request_id ? 1 : 0);
}

function taskOutcomeText(task: TaskRow): string | null {
  return task.status && isErrStatus(task.status)
    ? task.failure_reason ?? task.final_result ?? task.summary ?? null
    : task.final_result ?? task.summary ?? task.goal ?? null;
}

function gatewayLabel(health: HealthPayload | null): string {
  const current = health?.gateway?.current;
  if (!current) {
    return health?.status ?? '?';
  }
  const pid = current.pid ? ` pid ${current.pid}` : '';
  return `${current.name}${pid}`;
}

function isProblemActivity(event: ActivityEvent): boolean {
  const text = haystack(event.status, event.severity, event.kind, event.message);
  return text.includes('err') || text.includes('fail') || text.includes('warn') || text.includes('timeout') || text.includes('stale');
}

function MiniSparkline({ buckets, t }: { buckets: number[]; t: Translator }) {
  const values = buckets.length ? buckets : Array.from({ length: 24 }, () => 0);
  const max = Math.max(1, ...values);
  return (
    <div className="mini-sparkline" role="img" aria-label={t('stats.chart.callDistribution')}>
      {values.map((value, index) => (
        <span key={index} style={{ height: `${Math.max(5, (value / max) * 100)}%` }} title={t('stats.chart.hourValue', { hour: index, count: value })} />
      ))}
    </div>
  );
}

function StatBarList({ title, items, t }: { title: string; items: TopEntry[]; t: Translator }) {
  const max = maxTopCount(items);
  return (
    <div className="chart-card">
      <h3 className="chart-title">{title}</h3>
      {!items.length ? <p className="empty">{t('stats.empty.data')}</p> : items.map((row) => (
        <div className="hbar-row" key={`${title}-${row.name}`}>
          <div className="hbar-label" title={row.name}>{row.name.length > 48 ? `${row.name.slice(0, 46)}…` : row.name}</div>
          <div className="hbar-track">
            <div className="hbar-fill" style={{ width: `${(row.count / max) * 100}%` }} />
          </div>
          <div className="hbar-count">{row.count}</div>
        </div>
      ))}
    </div>
  );
}

function AttributionFacetList({ title, items, t }: { title: string; items: AttributionFacet[]; t: Translator }) {
  const max = Math.max(1, ...items.map((row) => row.count ?? 0));
  return (
    <div className="chart-card">
      <h3 className="chart-title">{title}</h3>
      {!items.length ? <p className="empty">{t('stats.empty.data')}</p> : items.map((row) => (
        <div className="hbar-row" key={`${title}-${row.name}`}>
          <div className="hbar-label" title={row.name}>{row.name.length > 48 ? `${row.name.slice(0, 46)}...` : row.name}</div>
          <div className="hbar-track">
            <div className="hbar-fill" style={{ width: `${Math.max(2, ((row.count ?? 0) / max) * 100)}%` }} />
          </div>
          <div
            className="hbar-count"
            title={t('stats.chart.attributionDetail', {
              failed: row.failed ?? 0,
              failureRate: `${(((row.failure_rate ?? 0) <= 1 ? (row.failure_rate ?? 0) * 100 : (row.failure_rate ?? 0))).toFixed(1)}%`,
              p95: formatDurationMs(row.p95_latency_ms),
            })}
          >
            {row.count}
          </div>
        </div>
      ))}
    </div>
  );
}

function TokenBreakdownList({ title, items, t }: { title: string; items: TokenBreakdownEntry[]; t: Translator }) {
  const max = Math.max(1, ...items.map((row) => row.saved_tokens ?? 0));
  return (
    <div className="chart-card">
      <h3 className="chart-title">{title}</h3>
      {!items.length ? <p className="empty">{t('stats.empty.tokens')}</p> : items.map((row) => (
        <div className="hbar-row" key={`${title}-${row.name}`}>
          <div className="hbar-label" title={row.name}>{row.name.length > 48 ? `${row.name.slice(0, 46)}...` : row.name}</div>
          <div className="hbar-track">
            <div className="hbar-fill" style={{ width: `${Math.max(2, ((row.saved_tokens ?? 0) / max) * 100)}%` }} />
          </div>
          <div className="hbar-count" title={t('stats.chart.savingsDetail', { calls: row.calls, savings: formatSavingsPct(row.savings_pct) })}>
            {formatTokenCount(row.saved_tokens)}
          </div>
        </div>
      ))}
    </div>
  );
}

function TokenAccountingDetail({ row, t }: { row: TokenCarrier | null | undefined; t: Translator }) {
  const tokens = tokenAccounting(row);
  return (
    <div className="trace-detail-card">
      <div className="trace-card-head">
        <h3>{t('traces.detail.tokenAccounting')}</h3>
        <span>{tokens?.token_estimator ?? t('traces.label.noEstimator')}</span>
      </div>
      <div className="trace-summary-grid">
        <span><strong>{t('traces.label.format')}</strong>{tokens?.response_format ?? '-'}</span>
        <span><strong>{t('traces.label.returned')}</strong>{formatTokenCount(tokens?.returned_tokens)}</span>
        <span><strong>{t('traces.label.saved')}</strong>{formatTokenCount(tokens?.saved_tokens)}</span>
        <span><strong>{t('traces.label.savings')}</strong>{formatSavingsPct(tokens?.savings_pct)}</span>
        <span><strong>{t('traces.label.originalBytes')}</strong>{formatBytes(tokens?.original_bytes)}</span>
        <span><strong>{t('traces.label.returnedBytes')}</strong>{formatBytes(tokens?.returned_bytes)}</span>
      </div>
    </div>
  );
}

function HourlyChart({ buckets, t }: { buckets: number[]; t: Translator }) {
  if (!buckets.length) {
    return null;
  }
  const max = Math.max(1, ...buckets);
  return (
    <div className="chart-card">
      <h3 className="chart-title">{t('stats.chart.callsByHourUtc')}</h3>
      <div className="hourly-chart" role="img" aria-label={t('stats.chart.hourlyDistribution')}>
        {buckets.map((v, h) => (
          <div key={h} className="hour-col" title={t('stats.chart.hourValue', { hour: h, count: v })}>
            <div className="hour-bar" style={{ height: `${(v / max) * 100}%` }} />
            <span className="hour-tick">{h % 6 === 0 ? String(h) : ''}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function formatTraceDate(value: number | string | undefined): string {
  if (typeof value === 'number') {
    return new Date(value).toLocaleString();
  }
  if (typeof value === 'string' && value) {
    return new Date(value).toLocaleString();
  }
  return '-';
}

function payloadPreview(payload: TracePayload | null | undefined, t: Translator): string {
  if (!payload) {
    return t('traces.payload.empty');
  }
  const suffix = payload.truncated ? `\n\n[${t('traces.payload.truncated', { size: formatBytes(payload.original_size) })}]` : '';
  return `${payload.content}${suffix}`;
}

function buildAgentPacket(trace: TraceDetailPayload): string {
  const agent = trace.agent_context;
  const tokens = detailTraceTokens(trace);
  return JSON.stringify({
    purpose: 'dcc-mcp public-safe admin trace packet for LLM evaluation and issue triage',
    privacy_mode: 'public-safe',
    request_id: trace.request_id,
    method: trace.method,
    tool_family: publicToolFamily(trace.tool_slug, trace.method),
    dcc_type: trace.dcc_type,
    transport: trace.transport,
    status: trace.ok ? 'ok' : 'err',
    total_ms: trace.total_ms,
    token_accounting: tokenAccounting(trace),
    tokens: {
      input: tokens.inputTokens,
      output: tokens.outputTokens,
      total: tokens.totalTokens,
      estimated: tokens.estimatedTokens,
      estimator: tokens.estimator,
    },
    agent_context: publicSafeCallerContext(agent),
    slowest_span: [...(trace.spans ?? [])]
      .sort((a, b) => (b.duration_ns ?? 0) - (a.duration_ns ?? 0))
      .slice(0, 1)
      .map((span) => ({ name: span.name, duration_ms: Math.round((span.duration_ns ?? 0) / 1_000_000), ok: span.ok }))
      [0] ?? null,
    links: publicSafeIssuePaths(trace.request_id),
    raw_debug_bundle: {
      available: true,
      mode_query: 'mode=raw',
      privacy_note: 'Raw debug bundles may contain payload previews, prompts, scripts, auth material, local URLs, filesystem paths, or private scene identifiers. Review before sharing publicly.',
    },
  }, null, 2);
}

function TraceLinks({ links, t }: { links: AdminLinks; t: Translator }) {
  const rows = [
    [t('traces.link.adminTrace'), links.admin_trace_url],
    [t('traces.link.traceApi'), links.trace_api_url],
    [t('traces.link.agentTracePacket'), links.agent_trace_packet_url],
    [t('traces.link.debugBundle'), links.debug_bundle_url],
    [t('traces.link.issueReportJson'), links.issue_report_url],
    [t('traces.link.openapiInspector'), links.openapi_inspector_url],
    [t('traces.link.openapiSpec'), links.openapi_spec_url],
    [t('traces.link.openapiDocs'), links.openapi_docs_url],
    [t('traces.link.stats'), links.stats_url],
  ].filter(([, url]) => typeof url === 'string' && url.length > 0) as [string, string][];
  return (
    <div className="trace-links">
      {rows.map(([label, url]) => (
        <a key={label} href={url} target="_blank" rel="noopener noreferrer" title={url}>
          <strong>{label}</strong>
          <span>{url}</span>
        </a>
      ))}
    </div>
  );
}

function workflowAgentLabel(agent: WorkflowAgent | null | undefined): string {
  return agent?.agent_name || agent?.agent_id || agent?.agent_kind || agentModelLabel(agent) || 'unknown agent';
}

function agentModelLabel(agent: Pick<AgentContext, 'model' | 'model_provider' | 'model_version'> | null | undefined): string {
  if (!agent) {
    return '';
  }
  if (agent.model) {
    return agent.model;
  }
  if (agent.model_provider && agent.model_version) {
    return `${agent.model_provider}/${agent.model_version}`;
  }
  return agent.model_version || agent.model_provider || '';
}

function workflowMeta(workflow: WorkflowRow): string {
  const parts = [
    workflow.group_kind,
    workflow.correlation.session_id ? `session ${compactId(workflow.correlation.session_id)}` : '',
    workflow.correlation.turn_id ? `turn ${compactId(workflow.correlation.turn_id)}` : '',
    workflow.correlation.trace_id ? `trace ${compactId(workflow.correlation.trace_id)}` : '',
    `${workflow.step_count} steps`,
  ];
  return parts.filter(Boolean).join(' · ');
}

function agentTurnChips(agent: WorkflowAgent | AgentContext | null | undefined, t: Translator): string[] {
  if (!agent) {
    return [];
  }
  return [
    agentModelLabel(agent),
    agent.reasoning_effort ? t('traces.agent.effort', { value: agent.reasoning_effort }) : '',
    agent.session_id ? t('traces.agent.session', { id: compactId(agent.session_id) }) : '',
    agent.turn_id ? t('traces.agent.turn', { id: compactId(agent.turn_id) }) : '',
    agent.user_input_chars != null ? t('traces.agent.userChars', { count: agent.user_input_chars }) : '',
    agent.agent_reply_chars != null ? t('traces.agent.replyChars', { count: agent.agent_reply_chars }) : '',
    agent.user_input_hash ? t('traces.agent.userHash', { id: compactId(agent.user_input_hash) }) : '',
    agent.agent_reply_hash ? t('traces.agent.replyHash', { id: compactId(agent.agent_reply_hash) }) : '',
  ].filter(Boolean);
}

function WorkflowSearchChips({ signal, t }: { signal: WorkflowSearchSignal | null | undefined; t: Translator }) {
  if (!signal) {
    return null;
  }
  const chips = [
    t('workflows.chip.search', { id: compactId(signal.search_id) }),
    signal.result_count != null ? t('workflows.chip.hits', { count: signal.result_count }) : '',
    signal.zero_results ? t('workflows.chip.zeroResults') : '',
    signal.selected_rank != null ? t('workflows.chip.rank', { rank: signal.selected_rank }) : '',
    signal.selected_score != null ? t('workflows.chip.score', { score: signal.selected_score }) : '',
    signal.first_success_ms != null ? t('workflows.chip.firstSuccess', { duration: formatDurationMs(signal.first_success_ms) }) : '',
    ...(signal.match_reasons ?? []).slice(0, 3),
  ].filter(Boolean);
  return (
    <div className="workflow-chip-row">
      {chips.map((chip) => <span key={chip}>{chip}</span>)}
    </div>
  );
}

function workflowStageLabelKey(stage: WorkflowGraphStage): MessageKey {
  switch (stage) {
    case 'intent':
      return 'workflows.stage.intent';
    case 'discovery':
      return 'workflows.stage.discovery';
    case 'skillLoad':
      return 'workflows.stage.skillLoad';
    case 'toolCalls':
      return 'workflows.stage.toolCalls';
    case 'fallbacks':
      return 'workflows.stage.fallbacks';
    case 'artifacts':
      return 'workflows.stage.artifacts';
    case 'validation':
      return 'workflows.stage.validation';
    case 'report':
      return 'workflows.stage.report';
  }
}

function workflowStepText(step: WorkflowStep): string {
  return [step.kind, step.title, step.tool ?? ''].join(' ').toLowerCase();
}

function isEscapeHatchStep(step: WorkflowStep): boolean {
  const text = workflowStepText(step);
  return ['fallback', 'script', 'python', 'eval', 'execute_code', 'execute code', 'raw command'].some((needle) => text.includes(needle));
}

function workflowStepStage(step: WorkflowStep): WorkflowGraphStage {
  const text = workflowStepText(step);
  if (isEscapeHatchStep(step)) {
    return 'fallbacks';
  }
  if (text.includes('artifact') || text.includes('file') || text.includes('export') || text.includes('capture') || text.includes('render')) {
    return 'artifacts';
  }
  if (text.includes('validat') || text.includes('verify') || text.includes('check') || text.includes('readyz') || text.includes('health')) {
    return 'validation';
  }
  if (text.includes('report') || text.includes('issue')) {
    return 'report';
  }
  if (text.includes('load_skill') || text.includes('load skill') || text.includes('activate')) {
    return 'skillLoad';
  }
  if (step.kind === 'search' || step.kind === 'describe' || text.includes('search') || text.includes('describe') || text.includes('discover')) {
    return 'discovery';
  }
  return 'toolCalls';
}

function workflowNodeTone(node: WorkflowGraphNode): 'ok' | 'warn' | 'err' | 'muted' {
  if (isErrStatus(node.status)) {
    return 'err';
  }
  if (isWarnStatus(node.status) || node.escape_hatch) {
    return 'warn';
  }
  if (isOkStatus(node.status)) {
    return 'ok';
  }
  return 'muted';
}

function buildWorkflowGraphNodes(workflow: WorkflowRow): WorkflowGraphNode[] {
  const nodes: WorkflowGraphNode[] = [
    {
      node_id: `${workflow.workflow_id}:intent`,
      node_kind: 'intent',
      stage: 'intent',
      title: workflow.agent?.user_intent_summary || workflow.agent?.task || workflow.title,
      status: workflow.status,
      timestamp: workflow.started_at,
      duration_ms: null,
    },
  ];
  for (const step of workflow.steps) {
    const stage = workflowStepStage(step);
    nodes.push({
      node_id: step.step_id,
      node_kind: 'step',
      stage,
      title: step.title,
      status: step.status,
      timestamp: step.timestamp,
      duration_ms: step.duration_ms,
      step,
      escape_hatch: stage === 'fallbacks',
    });
  }
  if (workflow.agent?.agent_reply_summary) {
    nodes.push({
      node_id: `${workflow.workflow_id}:report`,
      node_kind: 'report',
      stage: 'report',
      title: workflow.agent.agent_reply_summary,
      status: workflow.status,
      timestamp: workflow.finished_at,
      duration_ms: null,
    });
  }
  return nodes;
}

function defaultWorkflowNodeId(nodes: WorkflowGraphNode[]): string {
  return nodes.find((node) => workflowNodeTone(node) === 'err')?.node_id
    ?? nodes.find((node) => node.escape_hatch)?.node_id
    ?? nodes.find((node) => workflowNodeTone(node) === 'warn')?.node_id
    ?? nodes[1]?.node_id
    ?? nodes[0]?.node_id
    ?? '';
}

function workflowPrimaryRequestId(workflow: WorkflowRow): string | undefined {
  return [...workflow.steps]
    .reverse()
    .find((step) => step.request_id && (step.links || step.kind === 'call' || workflowStepStage(step) === 'toolCalls'))
    ?.request_id ?? workflow.correlation.request_ids?.at(-1);
}

function workflowUniqueValues(workflow: WorkflowRow, field: 'dcc_type' | 'transport'): string[] {
  return Array.from(new Set(workflow.steps.map((step) => step[field]).filter(Boolean) as string[]));
}

function workflowNodeRows(node: WorkflowGraphNode, workflow: WorkflowRow, t: Translator): [string, string][] {
  if (node.node_kind === 'intent') {
    return [
      [t('workflows.detail.agent'), workflowAgentLabel(workflow.agent)],
      [t('workflows.detail.model'), agentModelLabel(workflow.agent) || '-'],
      [t('workflows.detail.intent'), workflow.agent?.user_intent_summary || workflow.agent?.task || workflow.title],
      [t('workflows.detail.session'), compactId(workflow.correlation.session_id)],
      [t('workflows.detail.turn'), compactId(workflow.correlation.turn_id)],
      [t('workflows.detail.apps'), compactList(workflowUniqueValues(workflow, 'dcc_type'), '-')],
    ];
  }
  if (node.node_kind === 'report') {
    return [
      [t('workflows.detail.reply'), workflow.agent?.agent_reply_summary ?? node.title],
      [t('workflows.detail.status'), workflow.status],
      [t('workflows.detail.duration'), formatDurationMs(workflow.duration_ms)],
      [t('workflows.detail.requests'), compactList(workflow.correlation.request_ids, '-')],
    ];
  }

  const step = node.step;
  if (!step) {
    return [];
  }
  return [
    [t('workflows.detail.stage'), t(workflowStageLabelKey(node.stage))],
    [t('workflows.detail.status'), step.status],
    [t('workflows.detail.time'), formatTime(step.timestamp)],
    [t('workflows.detail.duration'), formatDurationMs(step.duration_ms)],
    [t('workflows.detail.app'), step.dcc_type ?? 'gateway'],
    [t('workflows.detail.instance'), compactId(step.instance_id)],
    [t('workflows.detail.transport'), step.transport ?? '-'],
    [t('workflows.detail.request'), compactId(step.request_id)],
    [t('workflows.detail.parent'), compactId(step.parent_request_id)],
    [t('workflows.detail.tool'), step.tool ?? step.title],
    [t('workflows.detail.search'), step.search?.search_id ? compactId(step.search.search_id) : '-'],
  ];
}

function WorkflowStageStrip({ workflow, t }: { workflow: WorkflowRow; t: Translator }) {
  const stages = workflow.steps.map(workflowStepStage);
  const uniqueStages = stages.filter((stage, index) => stages.indexOf(stage) === index).slice(0, 6);
  return (
    <div className="workflow-stage-strip" aria-label={t('workflows.label.stagePreview')}>
      {uniqueStages.map((stage) => <span key={stage}>{t(workflowStageLabelKey(stage))}</span>)}
      {stages.length > uniqueStages.length ? <span>{`+${stages.length - uniqueStages.length}`}</span> : null}
    </div>
  );
}

function WorkflowStepCard({
  step,
  onOpenTrace,
  onCopyIssueReport,
  t,
}: {
  step: WorkflowStep;
  onOpenTrace: (requestId: string) => void;
  onCopyIssueReport: (requestId: string) => void;
  t: Translator;
}) {
  const requestId = step.request_id ?? undefined;
  const links = requestId ? traceLinks(requestId, step.links) : step.links;
  return (
    <article className={`workflow-step ${isErrStatus(step.status) ? 'err' : isWarnStatus(step.status) ? 'warn' : isOkStatus(step.status) ? 'ok' : ''}`}>
      <div className="workflow-step-line" />
      <div className="workflow-step-body">
        <div className="workflow-step-head">
          <span className="workflow-kind">{step.kind}</span>
          <StatusBadge value={step.status} />
          <TimeValue value={step.timestamp} />
          <span>{formatDurationMs(step.duration_ms)}</span>
        </div>
        <h4 title={step.title}>{step.title}</h4>
        <div className="workflow-step-meta">
          <span>{step.dcc_type ?? 'gateway'}</span>
          <span>{step.transport ?? '-'}</span>
          <span>{compactId(step.instance_id)}</span>
          {step.parent_request_id ? <span>parent {compactId(step.parent_request_id)}</span> : null}
        </div>
        <WorkflowSearchChips signal={step.search} t={t} />
        <div className="workflow-step-actions">
          {requestId ? (
            <button className="refresh-btn" type="button" title={requestId} onClick={() => onOpenTrace(requestId)}>
              {t('common.action.trace')}
            </button>
          ) : null}
          {links?.debug_bundle_url ? <a className="link-chip" href={links.debug_bundle_url} target="_blank" rel="noopener noreferrer">{t('workflows.link.bundle')}</a> : null}
          {links?.issue_report_url ? <a className="link-chip" href={links.issue_report_url} target="_blank" rel="noopener noreferrer">{t('workflows.link.issueJson')}</a> : null}
          {links?.openapi_docs_url ? <a className="link-chip" href={links.openapi_docs_url} target="_blank" rel="noopener noreferrer">{t('workflows.link.docs')}</a> : null}
          {requestId ? (
            <button className="refresh-btn" type="button" onClick={() => onCopyIssueReport(requestId)}>
              {t('workflows.action.copyJson')}
            </button>
          ) : null}
        </div>
      </div>
    </article>
  );
}

function WorkflowGraphDetail({
  workflow,
  onClose,
  onOpenTrace,
  onCopyIssueReport,
  t,
}: {
  workflow: WorkflowRow;
  onClose: () => void;
  onOpenTrace: (requestId: string) => void;
  onCopyIssueReport: (requestId: string) => void;
  t: Translator;
}) {
  const nodes = useMemo(() => buildWorkflowGraphNodes(workflow), [workflow]);
  const [selectedNodeId, setSelectedNodeId] = useState(() => defaultWorkflowNodeId(nodes));
  useEffect(() => {
    setSelectedNodeId(defaultWorkflowNodeId(nodes));
  }, [nodes]);

  const selectedNode = nodes.find((node) => node.node_id === selectedNodeId) ?? nodes[0];
  const appTypes = workflowUniqueValues(workflow, 'dcc_type');
  const transports = workflowUniqueValues(workflow, 'transport');
  const selectedStep = selectedNode?.step;

  return (
    <section className="workflow-detail-graph" aria-label={t('workflows.section.detail')}>
      <div className="workflow-detail-head">
        <div>
          <div className="workflow-kicker">{t('workflows.label.selectedWorkflow')}</div>
          <h3 title={workflow.title}>{workflow.title}</h3>
          <div className="workflow-subline">
            {t('workflows.detail.graphSummary', {
              stages: nodes.length,
              apps: compactList(appTypes, '-'),
              transports: compactList(transports, '-'),
            })}
          </div>
        </div>
        <button className="refresh-btn" type="button" onClick={onClose}>{t('workflows.action.closeDetail')}</button>
      </div>

      <div className="workflow-detail-layout">
        <div className="workflow-graph-rail">
          <h4>{t('workflows.section.graph')}</h4>
          <div className="workflow-graph-nodes">
            {nodes.map((node, index) => {
              const tone = workflowNodeTone(node);
              const selected = node.node_id === selectedNode?.node_id;
              return (
                <button
                  key={node.node_id}
                  className={`workflow-graph-node ${tone} ${selected ? 'selected' : ''} ${node.escape_hatch ? 'escape' : ''}`}
                  type="button"
                  onClick={() => setSelectedNodeId(node.node_id)}
                  aria-pressed={selected}
                >
                  <span className="workflow-node-index">{index + 1}</span>
                  <span className="workflow-node-stage">{t(workflowStageLabelKey(node.stage))}</span>
                  <strong title={node.title}>{node.title}</strong>
                  <span className="workflow-node-meta">
                    {node.timestamp ? <TimeValue value={node.timestamp} /> : null}
                    <StatusBadge value={node.status} />
                    {node.escape_hatch ? <span className="badge-warn">{t('workflows.badge.escapeHatch')}</span> : null}
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        <div className="workflow-node-detail">
          <div className="trace-card-head">
            <div>
              <h4>{selectedNode ? t(workflowStageLabelKey(selectedNode.stage)) : t('workflows.section.nodeDetail')}</h4>
              {selectedNode ? <p className="workflow-agent-task">{selectedNode.title}</p> : null}
            </div>
          </div>
          {selectedNode ? (
            <>
              <div className="workflow-detail-kv">
                {workflowNodeRows(selectedNode, workflow, t).map(([label, value]) => (
                  <span key={label}>
                    <strong>{label}</strong>
                    {value}
                  </span>
                ))}
              </div>
              {selectedStep?.search ? <WorkflowSearchChips signal={selectedStep.search} t={t} /> : null}
              {selectedStep ? (
                <div className="workflow-selected-step">
                  <WorkflowStepCard
                    step={selectedStep}
                    onOpenTrace={onOpenTrace}
                    onCopyIssueReport={onCopyIssueReport}
                    t={t}
                  />
                </div>
              ) : null}
            </>
          ) : (
            <p className="empty">{t('workflows.empty.detail')}</p>
          )}
        </div>
      </div>
    </section>
  );
}

function WorkflowCard({
  workflow,
  onInspect,
  onOpenTrace,
  onCopyIssueReport,
  t,
}: {
  workflow: WorkflowRow;
  onInspect: (workflowId: string) => void;
  onOpenTrace: (requestId: string) => void;
  onCopyIssueReport: (requestId: string) => void;
  t: Translator;
}) {
  const requestId = workflowPrimaryRequestId(workflow);
  return (
    <article className={`workflow-card ${isErrStatus(workflow.status) ? 'err' : isWarnStatus(workflow.status) ? 'warn' : 'ok'}`}>
      <div className="workflow-card-head">
        <div>
          <div className="workflow-kicker">{workflowAgentLabel(workflow.agent)}</div>
          <h3 title={workflow.title}>{workflow.title}</h3>
          <div className="workflow-subline">{workflowMeta(workflow)}</div>
        </div>
        <div className="workflow-status">
          <StatusBadge value={workflow.status} />
          <span>{formatDurationMs(workflow.duration_ms)}</span>
        </div>
      </div>
      {workflow.agent?.task ? <p className="workflow-agent-task">{workflow.agent.task}</p> : null}
      {workflow.agent?.user_intent_summary ? <p className="workflow-agent-task">{workflow.agent.user_intent_summary}</p> : null}
      {workflow.agent?.agent_reply_summary ? <p className="workflow-agent-task muted">{workflow.agent.agent_reply_summary}</p> : null}
      {agentTurnChips(workflow.agent, t).length ? (
        <div className="workflow-chip-row">
          {agentTurnChips(workflow.agent, t).map((chip) => <span key={chip}>{chip}</span>)}
        </div>
      ) : null}
      <div className="workflow-chip-row">
        <span>{t('workflows.chip.searches', { count: workflow.discovery.search_count })}</span>
        {workflow.discovery.zero_result_count ? <span>{t('workflows.chip.zeroResult', { count: workflow.discovery.zero_result_count })}</span> : null}
        {workflow.discovery.best_selected_rank != null ? <span>{t('workflows.chip.bestRank', { rank: workflow.discovery.best_selected_rank })}</span> : null}
        {workflow.discovery.time_to_first_success_ms != null ? <span>{t('workflows.chip.firstSuccess', { duration: formatDurationMs(workflow.discovery.time_to_first_success_ms) })}</span> : null}
        {workflow.failed_steps ? <span>{t('workflows.chip.failed', { count: workflow.failed_steps })}</span> : null}
      </div>
      <WorkflowStageStrip workflow={workflow} t={t} />
      <div className="workflow-card-actions">
        <button className="refresh-btn" type="button" onClick={() => onInspect(workflow.workflow_id)}>
          {t('workflows.action.inspect')}
        </button>
        {requestId ? (
          <button className="refresh-btn" type="button" title={requestId} onClick={() => onOpenTrace(requestId)}>
            {t('common.action.trace')}
          </button>
        ) : null}
        {requestId ? (
          <button className="refresh-btn" type="button" onClick={() => onCopyIssueReport(requestId)}>
            {t('workflows.action.copyJson')}
          </button>
        ) : null}
      </div>
    </article>
  );
}

function TraceDetailPanel({
  trace,
  fallback,
  t,
  onCopy,
  onCopyIssueReport,
  onDownloadIssueReport,
}: {
  trace: TraceDetailPayload | null;
  fallback: string;
  t: Translator;
  onCopy: (text: string, label: string) => void;
  onCopyIssueReport: (requestId: string) => void;
  onDownloadIssueReport: (requestId: string) => void;
}) {
  if (!trace) {
    return <pre className="trace-detail">{fallback}</pre>;
  }
  const spans = Array.isArray(trace.spans) ? trace.spans : [];
  const maxNs = Math.max(1, ...spans.map((span) => span.duration_ns ?? 0));
  const agent = trace.agent_context ?? null;
  const agentTitle = agent?.agent_name || agent?.agent_id || agent?.agent_kind || t('traces.label.callerContext');
  const links = traceLinks(trace.request_id, trace.links);
  const tokens = detailTraceTokens(trace);
  const attrsPreview = (attrs?: Record<string, unknown>) => {
    if (!attrs || Object.keys(attrs).length === 0) {
      return '';
    }
    return JSON.stringify(attrs);
  };

  return (
    <div className="trace-detail-panel">
      <div className="trace-detail-card trace-summary-card">
        <div>
          <span className="trace-kicker">{t('traces.label.request')}</span>
          <h3 title={trace.request_id}>{compactId(trace.request_id)}</h3>
        </div>
        <div className="trace-copy-actions">
          <button className="refresh-btn" type="button" onClick={() => onCopy(links.admin_trace_url ?? '', 'trace URL')}>
            {t('traces.action.copyUrl')}
          </button>
          <button className="refresh-btn" type="button" onClick={() => onCopy(buildAgentPacket(trace), 'agent packet')}>
            {t('traces.action.copyAgentPacket')}
          </button>
          <button className="refresh-btn" type="button" onClick={() => onCopyIssueReport(trace.request_id)}>
            {t('traces.action.copyIssueJson')}
          </button>
          <button className="refresh-btn" type="button" onClick={() => onDownloadIssueReport(trace.request_id)}>
            {t('traces.action.downloadJson')}
          </button>
        </div>
        <div className="trace-summary-grid">
          <span><strong>{t('traces.label.tool')}</strong>{trace.tool_slug ?? trace.method}</span>
          <span><strong>{t('traces.label.status')}</strong>{trace.ok ? 'ok' : 'err'}</span>
          <span><strong>{t('traces.label.latency')}</strong><LatencyValue value={trace.total_ms} t={t} /></span>
          <span><strong>{t('traces.label.inputTokens')}</strong>{tokens.inputTokens == null ? '-' : formatTokenCount(tokens.inputTokens)}</span>
          <span><strong>{t('traces.label.outputTokens')}</strong>{tokens.outputTokens == null ? '-' : formatTokenCount(tokens.outputTokens)}</span>
          <span><strong>{t('traces.label.totalTokens')}</strong>{tokens.totalTokens == null ? '-' : formatTokenCount(tokens.totalTokens)}</span>
          <span><strong>{t('traces.label.estimator')}</strong>{tokens.estimator ?? '-'}</span>
          <span><strong>{t('traces.label.transport')}</strong>{trace.transport ?? '-'}</span>
          <span><strong>{t('traces.label.started')}</strong>{formatTraceDate(trace.started_at)}</span>
          <span><strong>{t('traces.label.spans')}</strong>{spans.length}</span>
        </div>
        <TraceLinks links={links} t={t} />
      </div>

      {agent ? (
        <div className="trace-detail-card agent-context-card">
          <div className="trace-card-head">
            <h3>{agentTitle}</h3>
            {agentModelLabel(agent) ? <span>{agentModelLabel(agent)}</span> : null}
          </div>
          {agent.task ? <p className="agent-task">{agent.task}</p> : null}
          {agent.user_intent_summary ? <p className="agent-summary">{agent.user_intent_summary}</p> : null}
          {agent.agent_reply_summary ? <p className="agent-summary muted">{agent.agent_reply_summary}</p> : null}
          {agent.reasoning_summary ? <p className="agent-summary">{agent.reasoning_summary}</p> : null}
          {agent.plan?.length ? (
            <div className="agent-list">
              <strong>{t('traces.label.plan')}</strong>
              {agent.plan.map((step, index) => <span key={`${step}-${index}`}>{step}</span>)}
            </div>
          ) : null}
          {agent.observations?.length ? (
            <div className="agent-list">
              <strong>{t('traces.label.observations')}</strong>
              {agent.observations.map((step, index) => <span key={`${step}-${index}`}>{step}</span>)}
            </div>
          ) : null}
          <div className="agent-meta">
            {actorLabel(agent) !== '-' ? <span>{t('common.table.actor')} {actorLabel(agent)} {trustChip(firstTrust(agent, ['actor_name', 'actor_id', 'actor_email_hash']))}</span> : null}
            {platformLabel(agent) !== '-' ? <span>{t('common.table.platform')} {platformLabel(agent)} {trustChip(firstTrust(agent, ['client_platform', 'client_os', 'client_host']))}</span> : null}
            {sourceIpLabel(agent) !== '-' ? <span>{t('common.table.sourceIp')} {sourceIpLabel(agent)} {trustChip(trustFor(agent, 'source_ip'))}</span> : null}
            {agent.auth_subject ? <span>{t('common.table.auth')} {agent.auth_subject} {trustChip(trustFor(agent, 'auth_subject'))}</span> : null}
            {agentTurnChips(agent, t).map((chip) => <span key={chip}>{chip}</span>)}
            {agent.parent_request_id ? <span>parent {compactId(agent.parent_request_id)}</span> : null}
            {agent.trace_id ? <span>trace {compactId(agent.trace_id)}</span> : null}
            {agent.tags?.map((tag) => <span key={tag}>{tag}</span>)}
          </div>
          <pre className="payload-pre caller-context-pre">{JSON.stringify(safeCallerContext(agent), null, 2)}</pre>
        </div>
      ) : null}

      <TokenAccountingDetail row={trace} t={t} />

      <div className="trace-detail-card">
        <div className="trace-card-head">
          <h3>{t('traces.detail.spanWaterfall')}</h3>
          <span>{formatDurationMs(trace.total_ms)}</span>
        </div>
        <div className="span-waterfall">
          {spans.length === 0 ? <p className="empty">{t('traces.detail.noSpans')}</p> : spans.map((span, index) => (
            <div className={`span-row ${span.ok ? 'ok' : 'err'} ${latencyClass(spanDurationMs(span))}`} key={`${span.name}-${index}`}>
              <div className="span-row-label">
                <strong>{span.name}</strong>
                <LatencyValue value={spanDurationMs(span)} t={t} />
              </div>
              <div className="span-track">
                <div className="span-fill" style={{ width: `${Math.max(2, ((span.duration_ns ?? 0) / maxNs) * 100)}%` }} />
              </div>
              {attrsPreview(span.attributes) ? <code title={attrsPreview(span.attributes)}>{attrsPreview(span.attributes)}</code> : null}
            </div>
          ))}
        </div>
      </div>

      <div className="payload-grid">
        <div className="trace-detail-card">
          <div className="trace-card-head">
            <h3>{t('traces.detail.input')}</h3>
            <span>
              {formatBytes(trace.input?.original_size)}
              {trace.input ? ` / ${formatTokenCount(trace.input.estimated_tokens)} tok` : ''}
            </span>
          </div>
          <pre className="payload-pre">{payloadPreview(trace.input, t)}</pre>
        </div>
        <div className="trace-detail-card">
          <div className="trace-card-head">
            <h3>{t('traces.detail.output')}</h3>
            <span>
              {formatBytes(trace.output?.original_size)}
              {trace.output ? ` / ${formatTokenCount(trace.output.estimated_tokens)} tok` : ''}
            </span>
          </div>
          <pre className="payload-pre">{payloadPreview(trace.output, t)}</pre>
        </div>
      </div>
    </div>
  );
}

function GovernanceControlCard({ control, t }: { control: GovernanceMiddlewareControl; t: Translator }) {
  const config = control.config ?? {};
  const details = Object.entries(config)
    .filter(([key]) => key !== 'fields')
    .slice(0, 4);
  const fields = Array.isArray(config.fields) ? config.fields.map(String) : [];
  return (
    <div className="governance-card">
      <div className="governance-card-head">
        <span className="source-pill">{control.kind}</span>
        <span className="badge badge-muted">{control.mode}</span>
      </div>
      <h3>{control.summary}</h3>
      {fields.length ? <p className="mono-path">{compactList(fields, t('governance.empty.fields'))}</p> : null}
      {details.length ? (
        <div className="governance-kv">
          {details.map(([key, value]) => (
            <span key={key}><strong>{key}</strong>{String(value)}</span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function componentSchemaCount(spec: OpenApiSpec | null): number {
  const components = spec?.components;
  if (!components || typeof components !== 'object' || Array.isArray(components)) {
    return 0;
  }
  const schemas = (components as { schemas?: unknown }).schemas;
  if (!schemas || typeof schemas !== 'object' || Array.isArray(schemas)) {
    return 0;
  }
  return Object.keys(schemas).length;
}

function OpenApiInspectorPanel({
  spec,
  raw,
  operations,
  source,
  labels,
  t,
}: {
  spec: OpenApiSpec | null;
  raw: string;
  operations: OpenApiOperationRow[];
  source: OpenApiSource;
  labels: {
    emptyDocument: string;
    openapi: string;
    version: string;
    paths: string;
    operations: string;
    schemas: string;
    tags: string;
    operationsSection: string;
    emptyOperations: string;
    linksSection: string;
    body: string;
    noBody: string;
    params: (count: number) => string;
    responses: (codes: string) => string;
    noResponses: string;
  };
  t: Translator;
}) {
  if (!spec) {
    return <p className="empty">{labels.emptyDocument}</p>;
  }
  const pathsCount = Object.keys(spec.paths ?? {}).length;
  const tagCount = new Set(operations.flatMap((operation) => operation.tags)).size || (spec.tags?.length ?? 0);
  const methods = Array.from(new Set(operations.map((operation) => operation.method))).sort();
  const specLinks: AdminLinks = {
    openapi_inspector_url: source.inspectorUrl,
    openapi_spec_url: source.specUrl,
    openapi_docs_url: source.docsUrl,
  };

  return (
    <div className="openapi-inspector">
      <div className="metric-grid compact">
        <MetricTile label={labels.openapi} value={spec.openapi ?? '-'} detail={source.label} />
        <MetricTile label={labels.version} value={spec.info?.version ?? '-'} />
        <MetricTile label={labels.paths} value={pathsCount} />
        <MetricTile label={labels.operations} value={operations.length} />
        <MetricTile label={labels.schemas} value={componentSchemaCount(spec)} />
        <MetricTile label={labels.tags} value={tagCount} />
      </div>

      <div className="openapi-layout">
        <div className="openapi-operation-list">
          <div className="trace-group-head">
            <h3>{labels.operationsSection}</h3>
            <span>{operations.length} · {methods.join(', ') || '-'}</span>
          </div>
          {operations.length === 0 ? <p className="empty">{labels.emptyOperations}</p> : operations.map((operation) => (
            <article className="openapi-operation-card" key={operation.key}>
              <div className="openapi-operation-head">
                <span className={`method-pill ${operation.method.toLowerCase()}`}>{operation.method}</span>
                <span className="openapi-path">{operation.path}</span>
              </div>
              <h3>{operation.operationId}</h3>
              {operation.summary ? <p>{operation.summary}</p> : null}
              <div className="openapi-meta-row">
                <span>{operation.hasRequestBody ? labels.body : labels.noBody}</span>
                <span>{labels.params(operation.parameterCount)}</span>
                <span>{operation.responseCodes.length ? labels.responses(operation.responseCodes.join(', ')) : labels.noResponses}</span>
                {operation.tags.map((tag) => <span key={tag}>{tag}</span>)}
              </div>
            </article>
          ))}
        </div>

        <div className="trace-detail-card openapi-spec-card">
          <div className="trace-card-head">
            <h3>{labels.linksSection}</h3>
            <span>{formatBytes(raw.length)}</span>
          </div>
          <TraceLinks links={specLinks} t={t} />
          <pre className="payload-pre openapi-spec-pre">{raw}</pre>
        </div>
      </div>
    </div>
  );
}

function groupRows<T>(rows: T[], keyFn: (row: T) => string): Map<string, T[]> {
  const map = new Map<string, T[]>();
  for (const row of rows) {
    const key = keyFn(row);
    const bucket = map.get(key) ?? [];
    bucket.push(row);
    map.set(key, bucket);
  }
  return map;
}

function App() {
  const [localeOverride, setLocaleOverride] = useState<SupportedLocale | null>(() => readLocaleOverride());
  const localeDetection = useMemo(() => detectBrowserLocale(localeOverride), [localeOverride]);
  const t = useMemo(() => createTranslator(localeDetection.locale), [localeDetection.locale]);
  const [activePanel, setActivePanel] = useState<Panel>(() => readPanelFromUrl());
  const [health, setHealth] = useState<HealthPayload | null>(null);
  const [activity, setActivity] = useState<ActivityEvent[]>([]);
  const [tools, setTools] = useState<ToolRow[]>([]);
  const [workflows, setWorkflows] = useState<WorkflowRow[]>([]);
  const [selectedWorkflowId, setSelectedWorkflowId] = useState<string | null>(null);
  const [tasks, setTasks] = useState<TaskRow[]>([]);
  const [calls, setCalls] = useState<CallRow[]>([]);
  const [traces, setTraces] = useState<TraceRow[]>([]);
  const [traffic, setTraffic] = useState<TrafficPayload | null>(null);
  const [stats, setStats] = useState<StatsPayload | null>(null);
  const [governance, setGovernance] = useState<GovernancePayload | null>(null);
  const [statsRange, setStatsRange] = useState(() => readStatsRangeFromUrl());
  const [openApiSource, setOpenApiSource] = useState<OpenApiSource>(() => readOpenApiSourceFromUrl());
  const [openApiSpec, setOpenApiSpec] = useState<OpenApiSpec | null>(null);
  const [openApiRaw, setOpenApiRaw] = useState('');
  const [instanceRows, setInstanceRows] = useState<InstanceRow[]>([]);
  const [instanceSummary, setInstanceSummary] = useState<InstanceSummary>({ live: 0, stale: 0, unhealthy: 0 });
  const [setupUrlMode, setSetupUrlMode] = useState<SetupUrlMode>('local');
  const [clientPlatform] = useState<ClientPlatform>(() => detectClientPlatform());
  const [directInstanceId, setDirectInstanceId] = useState<string>('');
  const [logs, setLogs] = useState<LogRow[]>([]);
  const [logSeverityFilter, setLogSeverityFilter] = useState<LogSeverityFilter>('all');
  const [skillPaths, setSkillPaths] = useState<SkillPathRow[]>([]);
  const [skills, setSkills] = useState<SkillRow[]>([]);
  const [skillTotals, setSkillTotals] = useState({
    total: 0,
    loaded: 0,
    unloaded: 0,
    action_count: 0,
    searched: 0,
    used: 0,
    low_adoption: 0,
    load_errors: 0,
    missing_paths: 0,
  });
  const [skillPathInput, setSkillPathInput] = useState('');
  const [skillPathBusy, setSkillPathBusy] = useState(false);
  const [selectedSkill, setSelectedSkill] = useState<SkillRow | null>(null);
  const [skillDetail, setSkillDetail] = useState<SkillDetailPayload | null>(null);
  const [skillDetailBusy, setSkillDetailBusy] = useState(false);
  const [traceDetail, setTraceDetail] = useState<string>('Select a trace row for detail.');
  const [traceDetailPayload, setTraceDetailPayload] = useState<TraceDetailPayload | null>(null);
  const [trafficDetail, setTrafficDetail] = useState<string>('Select a traffic frame row for detail.');
  const [callDetail, setCallDetail] = useState<string>('Select a call row for trace detail.');
  const [slowOnly, setSlowOnly] = useState(false);
  const [copiedNotice, setCopiedNotice] = useState<string>('');
  const [updatedAt, setUpdatedAt] = useState<Record<Panel, string>>(() => ({
    setup: t('common.status.loading'),
    debug: t('common.status.loading'),
    activity: t('common.status.loading'),
    health: t('common.status.loading'),
    instances: t('common.status.loading'),
    tools: t('common.status.loading'),
    workflows: t('common.status.loading'),
    tasks: t('common.status.loading'),
    openapi: t('common.status.loading'),
    calls: t('common.status.loading'),
    traces: t('common.status.loading'),
    traffic: t('common.status.loading'),
    stats: t('common.status.loading'),
    governance: t('common.status.loading'),
    logs: t('common.status.loading'),
    'skill-paths': t('common.status.loading'),
  }));
  const [errors, setErrors] = useState<Partial<Record<Panel, string>>>({});
  const [listSearch, setListSearch] = useState('');

  const panels = useMemo(
    () => PANELS.map((panel) => ({ ...panel, label: t(panel.labelKey), group: t(panel.groupKey) })),
    [t],
  );

  const changeLocale = useCallback((locale: SupportedLocale) => {
    storeLocaleOverride(locale);
    setLocaleOverride(locale);
  }, []);

  useEffect(() => {
    document.documentElement.lang = localeDetection.locale;
    document.documentElement.dataset.adminLocale = localeDetection.locale;
    document.documentElement.dataset.adminLocaleSource = localeDetection.source;
    if (localeDetection.matchedTag) {
      document.documentElement.dataset.adminLocaleMatchedTag = localeDetection.matchedTag;
    } else {
      delete document.documentElement.dataset.adminLocaleMatchedTag;
    }
  }, [localeDetection]);

  useEffect(() => {
    setListSearch('');
  }, [activePanel]);

  const filteredActivity = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return activity;
    }
    return activity.filter((event) =>
      matchesListFilter(
        q,
        haystack(
          event.timestamp,
          event.kind,
          event.severity,
          event.status,
          event.message,
          event.tool ?? '',
          event.correlation?.request_id ?? '',
          event.correlation?.session_id ?? '',
          event.correlation?.instance_id ?? '',
          event.correlation?.dcc_type ?? '',
          event.correlation?.workflow_id ?? '',
          event.correlation?.job_id ?? '',
          event.correlation?.agent_id ?? '',
          event.correlation?.actor_id ?? '',
          event.correlation?.actor_name ?? '',
          event.correlation?.client_platform ?? '',
          event.correlation?.source_ip ?? '',
        ),
      ),
    );
  }, [activity, listSearch]);

  const filteredTools = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return tools;
    }
    return tools.filter((t) =>
      matchesListFilter(
        q,
        haystack(t.slug, t.dcc_type, t.summary, t.instance_id, t.instance_prefix, t.skill_name ?? '', t.name ?? ''),
      ),
    );
  }, [tools, listSearch]);

  const openApiOperations = useMemo(() => flattenOpenApiOperations(openApiSpec), [openApiSpec]);

  const filteredOpenApiOperations = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return openApiOperations;
    }
    return openApiOperations.filter((operation) =>
      matchesListFilter(
        q,
        haystack(
          operation.method,
          operation.path,
          operation.operationId,
          operation.summary,
          operation.tags.join(' '),
          operation.responseCodes.join(' '),
        ),
      ),
    );
  }, [openApiOperations, listSearch]);

  const filteredCalls = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    const rows = slowOnly
      ? [...calls].filter((call) => isSlowLatency(call.duration_ms)).sort((a, b) => (b.duration_ms ?? 0) - (a.duration_ms ?? 0))
      : calls;
    if (!q) {
      return rows;
    }
    return rows.filter((c) =>
      matchesListFilter(
        q,
        haystack(
          c.timestamp,
          c.request_id,
          c.tool,
          c.dcc_type,
          c.status,
          c.error ?? '',
          String(c.duration_ms ?? ''),
          c.instance_id ?? '',
          c.transport ?? '',
          c.agent_id ?? '',
          c.agent_name ?? '',
          c.agent_model ?? '',
          c.actor ?? '',
          c.actor_id ?? '',
          c.actor_name ?? '',
          c.actor_email_hash ?? '',
          c.client_platform ?? '',
          c.client_os ?? '',
          c.client_host ?? '',
          c.auth_subject ?? '',
          c.source_ip ?? '',
          ...Object.values(c.attribution_trust ?? {}),
          c.parent_request_id ?? '',
        ),
      ),
    );
  }, [calls, listSearch, slowOnly]);

  const filteredTraces = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    const rows = slowOnly
      ? [...traces].filter((trace) => isSlowLatency(trace.total_ms)).sort((a, b) => traceLatency(b) - traceLatency(a))
      : traces;
    if (!q) {
      return rows;
    }
    return rows.filter((t) =>
      matchesListFilter(
        q,
        haystack(
          t.timestamp,
          t.request_id,
          t.tool,
          t.status,
          String(t.total_ms ?? ''),
          t.instance_id ?? '',
          t.dcc_type ?? '',
          t.transport ?? '',
          t.agent_id ?? '',
          t.agent_name ?? '',
          t.agent_model ?? '',
          t.actor ?? '',
          t.actor_id ?? '',
          t.actor_name ?? '',
          t.actor_email_hash ?? '',
          t.client_platform ?? '',
          t.client_os ?? '',
          t.client_host ?? '',
          t.auth_subject ?? '',
          t.source_ip ?? '',
          ...Object.values(t.attribution_trust ?? {}),
          t.slowest_span_name ?? '',
          t.input_tokens != null ? String(t.input_tokens) : '',
          t.output_tokens != null ? String(t.output_tokens) : '',
          t.total_tokens != null ? String(t.total_tokens) : '',
        ),
      ),
    );
  }, [traces, listSearch, slowOnly]);

  const trafficFrames = useMemo(() => traffic?.frames ?? [], [traffic]);
  const filteredTrafficFrames = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return trafficFrames;
    }
    return trafficFrames.filter((frame) =>
      matchesListFilter(
        q,
        haystack(
          frame.id ?? '',
          frame.name ?? '',
          trafficRequestId(frame) ?? '',
          frame.correlation?.trace_id ?? '',
          trafficSessionId(frame) ?? '',
          frame.attributes?.capture_id ?? '',
          frame.attributes?.direction ?? '',
          frame.attributes?.leg ?? '',
          frame.attributes?.transport ?? '',
          frame.attributes?.http?.method ?? '',
          frame.attributes?.http?.url ?? '',
          String(frame.attributes?.http?.status ?? ''),
          frame.attributes?.mcp?.kind ?? '',
          trafficMethod(frame),
          trafficRedactedPaths(frame).join(' '),
        ),
      ),
    );
  }, [trafficFrames, listSearch]);

  const filteredTasks = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return tasks;
    }
    return tasks.filter((task) =>
      matchesListFilter(
        q,
        haystack(
          task.task_id,
          task.task_type,
          task.status,
          task.title,
          task.goal ?? '',
          task.summary ?? '',
          task.final_result ?? '',
          task.failure_reason ?? '',
          task.started_at,
          task.finished_at ?? '',
          String(task.duration_ms ?? ''),
          task.app_types?.join(' ') ?? '',
          task.artifacts?.map((artifact) => haystack(artifact.kind, artifact.name, artifact.request_id ?? '')).join(' ') ?? '',
          task.validation_checks?.map((check) => haystack(check.title, check.status, check.request_id ?? '')).join(' ') ?? '',
          task.related?.workflow_ids?.join(' ') ?? '',
          task.related?.request_ids?.join(' ') ?? '',
          task.related?.trace_ids?.join(' ') ?? '',
          task.related?.session_ids?.join(' ') ?? '',
          task.correlation?.request_id ?? '',
          task.correlation?.instance_id ?? '',
          task.correlation?.dcc_type ?? '',
          task.correlation?.workflow_id ?? '',
          task.correlation?.job_id ?? '',
        ),
      ),
    );
  }, [tasks, listSearch]);

  const filteredWorkflows = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return workflows;
    }
    return workflows.filter((workflow) =>
      matchesListFilter(
        q,
        haystack(
          workflow.workflow_id,
          workflow.group_kind,
          workflow.title,
          workflow.status,
          workflow.agent?.agent_id ?? '',
          workflow.agent?.agent_name ?? '',
          workflow.agent?.model ?? '',
          workflow.agent?.task ?? '',
          workflow.correlation.session_id ?? '',
          workflow.correlation.trace_id ?? '',
          workflow.discovery.search_ids?.join(' ') ?? '',
          workflow.steps.map((step) => haystack(step.kind, step.title, step.request_id ?? '', step.search?.search_id ?? '')).join(' '),
        ),
      ),
    );
  }, [workflows, listSearch]);

  const selectedWorkflow = useMemo(
    () => workflows.find((workflow) => workflow.workflow_id === selectedWorkflowId) ?? null,
    [workflows, selectedWorkflowId],
  );
  const visibleSelectedWorkflow = useMemo(
    () => selectedWorkflow && filteredWorkflows.some((workflow) => workflow.workflow_id === selectedWorkflow.workflow_id) ? selectedWorkflow : null,
    [filteredWorkflows, selectedWorkflow],
  );

  const filteredInstanceRows = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return instanceRows;
    }
    return instanceRows.filter((w) =>
      matchesListFilter(
        q,
        haystack(
          w.instance_id,
          w.display_name,
          w.dcc_type,
          w.status,
          w.mcp_url,
          w.version ?? '',
          w.adapter_version ?? '',
          String(w.pid ?? ''),
          w.scene ?? '',
        ),
      ),
    );
  }, [instanceRows, listSearch]);

  const directSetupInstanceRows = useMemo(
    () => instanceRows.filter((instance) => !instance.stale && instance.mcp_url && !instance.mcp_url.includes(':0/')),
    [instanceRows],
  );
  const selectedDirectInstance = useMemo(
    () => directSetupInstanceRows.find((instance) => instance.instance_id === directInstanceId) ?? directSetupInstanceRows[0] ?? null,
    [directInstanceId, directSetupInstanceRows],
  );
  const lanUrl = useMemo(() => lanGatewayMcpUrl(), []);
  const setupMcpUrl = useMemo(() => {
    if (setupUrlMode === 'lan' && lanUrl) {
      return lanUrl;
    }
    if (setupUrlMode === 'direct' && selectedDirectInstance) {
      try {
        return backendAccessUrls(selectedDirectInstance.mcp_url).mcp;
      } catch {
        return selectedDirectInstance.mcp_url;
      }
    }
    return gatewayMcpUrl(health);
  }, [health, lanUrl, selectedDirectInstance, setupUrlMode]);

  useEffect(() => {
    if (!directInstanceId && directSetupInstanceRows.length > 0) {
      setDirectInstanceId(directSetupInstanceRows[0].instance_id);
    }
  }, [directInstanceId, directSetupInstanceRows]);

  const filteredLogs = useMemo(() => filterLogs(logs, listSearch, logSeverityFilter), [logSeverityFilter, logs, listSearch]);
  const logSeverityCounts = useMemo(() => summarizeLogSeverity(logs), [logs]);

  const filteredSkillPaths = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return skillPaths;
    }
    return skillPaths.filter((r) =>
      matchesListFilter(
        q,
        haystack(
          r.display_path ?? r.path,
          r.path_alias ?? '',
          r.path_hash ?? '',
          r.status ?? '',
          r.source,
          r.id != null ? String(r.id) : '',
        ),
      ),
    );
  }, [skillPaths, listSearch]);

  const filteredSkills = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return skills;
    }
    return skills.filter((skill) =>
      matchesListFilter(
        q,
        haystack(
          skill.name,
          skill.dcc_type,
          skill.loaded ? 'loaded' : 'unloaded',
          skill.summary ?? '',
          skill.instances.join(' '),
          skill.tools.join(' '),
          skill.adoption.low_adoption ? 'low adoption' : '',
          skill.adoption.searched ? 'searched' : '',
          skill.adoption.used ? 'used' : '',
        ),
      ),
    );
  }, [skills, listSearch]);

  const failureSignals = useMemo<FailureSignal[]>(() => {
    const rows = new Map<string, FailureSignal>();
    for (const call of calls) {
      if (call.success !== false && !isErrStatus(call.status)) {
        continue;
      }
      rows.set(call.request_id, {
        request_id: call.request_id,
        status: call.status || 'failed',
        tool: call.tool,
        detail: call.error || call.dcc_type || call.instance_id || 'call failed',
        ms: call.duration_ms,
      });
    }
    for (const trace of traces) {
      if (trace.success !== false && !isErrStatus(trace.status)) {
        continue;
      }
      const current = rows.get(trace.request_id);
      const detail = trace.slowest_span_name
        ? `${trace.slowest_span_name} span`
        : trace.dcc_type || trace.instance_id || 'trace failed';
      rows.set(trace.request_id, {
        request_id: trace.request_id,
        status: current?.status || trace.status || 'failed',
        tool: current?.tool || trace.tool,
        detail: current?.detail || detail,
        ms: current?.ms ?? trace.total_ms ?? null,
      });
    }
    return Array.from(rows.values()).slice(0, 8);
  }, [calls, traces]);

  const slowTraces = useMemo(
    () => [...traces].filter((trace) => trace.total_ms != null).sort((a, b) => traceLatency(b) - traceLatency(a)).slice(0, 8),
    [traces],
  );

  const traceByRequest = useMemo(() => {
    const rows = new Map<string, TraceRow>();
    for (const trace of traces) {
      rows.set(trace.request_id, trace);
    }
    return rows;
  }, [traces]);

  const slowCallCount = useMemo(
    () => calls.filter((call) => isSlowLatency(call.duration_ms)).length,
    [calls],
  );

  const slowTraceCount = useMemo(
    () => traces.filter((trace) => isSlowLatency(trace.total_ms)).length,
    [traces],
  );

  const tokenHeavyTraces = useMemo(
    () => [...traces]
      .filter((trace) => totalTraceTokens(trace) != null)
      .sort((a, b) => (totalTraceTokens(b) ?? 0) - (totalTraceTokens(a) ?? 0))
      .slice(0, 8),
    [traces],
  );

  const problemActivity = useMemo(
    () => activity.filter(isProblemActivity).slice(0, 8),
    [activity],
  );

  const problemLogs = useMemo(
    () => logs.filter(isProblemLog).slice(0, 10),
    [logs],
  );

  const unhealthyInstanceRows = useMemo(
    () => instanceRows.filter((instance) => instance.stale || !statusClass(instance.status).includes('ok')),
    [instanceRows],
  );

  const filteredTopTools = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    const rows = stats?.top_tools ?? [];
    if (!q) {
      return rows;
    }
    return rows.filter((r) => r.name.toLowerCase().includes(q));
  }, [stats, listSearch]);

  const filteredTopInstances = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    const rows = stats?.top_instances ?? [];
    if (!q) {
      return rows;
    }
    return rows.filter((r) => r.name.toLowerCase().includes(q));
  }, [stats, listSearch]);

  const filteredTopAgents = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    const rows = stats?.top_agents ?? [];
    if (!q) {
      return rows;
    }
    return rows.filter((r) => r.name.toLowerCase().includes(q));
  }, [stats, listSearch]);

  const filteredTopActors = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    const rows = stats?.top_actors ?? [];
    if (!q) {
      return rows;
    }
    return rows.filter((r) => r.name.toLowerCase().includes(q));
  }, [stats, listSearch]);

  const filteredTopClientPlatforms = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    const rows = stats?.top_client_platforms ?? [];
    if (!q) {
      return rows;
    }
    return rows.filter((r) => r.name.toLowerCase().includes(q));
  }, [stats, listSearch]);

  const filteredTopSourceIps = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    const rows = stats?.top_source_ips ?? [];
    if (!q) {
      return rows;
    }
    return rows.filter((r) => r.name.toLowerCase().includes(q));
  }, [stats, listSearch]);

  const filteredTopAppTypes = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    const rows = stats?.top_app_types ?? [];
    if (!q) {
      return rows;
    }
    return rows.filter((r) => r.name.toLowerCase().includes(q));
  }, [stats, listSearch]);

  const filterTokenBreakdowns = useCallback((rows: TokenBreakdownEntry[] | undefined) => {
    const q = listSearch.trim().toLowerCase();
    const safeRows = rows ?? [];
    if (!q) {
      return safeRows;
    }
    return safeRows.filter((r) => r.name.toLowerCase().includes(q));
  }, [listSearch]);

  const filteredTokenByTool = useMemo(() => filterTokenBreakdowns(stats?.token_usage?.by_tool), [filterTokenBreakdowns, stats]);
  const filteredTokenByInstance = useMemo(() => filterTokenBreakdowns(stats?.token_usage?.by_instance), [filterTokenBreakdowns, stats]);
  const filteredTokenByAgent = useMemo(() => filterTokenBreakdowns(stats?.token_usage?.by_agent), [filterTokenBreakdowns, stats]);
  const filteredTokenByTransport = useMemo(() => filterTokenBreakdowns(stats?.token_usage?.by_transport), [filterTokenBreakdowns, stats]);
  const filteredTokenByFormat = useMemo(() => filterTokenBreakdowns(stats?.token_usage?.by_response_format), [filterTokenBreakdowns, stats]);

  const filteredGovernanceDecisions = useMemo(() => {
    const rows = governance?.recent_decisions ?? [];
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return rows;
    }
    return rows.filter((row) =>
      matchesListFilter(
        q,
        haystack(
          row.timestamp,
          row.request_id ?? '',
          row.trace_id ?? '',
          row.session_id ?? '',
          row.transport ?? '',
          row.agent_id ?? '',
          row.agent_name ?? '',
          row.agent_model ?? '',
          row.actor_id ?? '',
          row.actor_name ?? '',
          row.client_platform ?? '',
          row.source_ip ?? '',
          row.tool ?? '',
          row.dcc_type ?? '',
          row.outcome ?? '',
          row.reason ?? '',
          row.policy?.reason ?? '',
          row.privacy?.redacted_paths?.join(' ') ?? '',
          row.traffic_capture?.reasons?.join(' ') ?? '',
        ),
      ),
    );
  }, [governance, listSearch]);

  const governanceSummary = useMemo(() => {
    const stats = governance?.stats ?? {};
    const capture = governance?.traffic_capture;
    const policy = governance?.policy;
    return {
      allowed: stats.recent_allowed ?? 0,
      denied: stats.recent_policy_denied ?? 0,
      throttled: stats.recent_throttled ?? 0,
      captured: stats.captured_frames ?? 0,
      skipped: stats.skipped_capture_frames ?? 0,
      redacted: stats.redacted_path_count ?? capture?.redaction?.paths?.length ?? 0,
      captureEnabled: capture?.enabled ?? false,
      readOnly: policy?.read_only ?? false,
      allowlists: Object.values(policy?.allowlists_active ?? {}).filter(Boolean).length,
    };
  }, [governance]);

  const taskSummary = useMemo(() => {
    const completed = tasks.filter((task) => isOkStatus(task.status)).length;
    const failed = tasks.filter((task) => isErrStatus(task.status)).length;
    const active = tasks.filter((task) => isWarnStatus(task.status)).length;
    return { completed, failed, active };
  }, [tasks]);

  const workflowSummary = useMemo(() => {
    const completed = workflows.filter((workflow) => isOkStatus(workflow.status)).length;
    const failed = workflows.filter((workflow) => isErrStatus(workflow.status)).length;
    const warning = workflows.filter((workflow) => isWarnStatus(workflow.status)).length;
    const zeroResults = workflows.filter((workflow) => workflow.discovery.zero_result_count > 0).length;
    return { completed, failed, warning, zeroResults };
  }, [workflows]);

  const traceSummary = useMemo(() => {
    const ok = traces.filter((trace) => isOkStatus(trace.status)).length;
    const failed = traces.filter((trace) => isErrStatus(trace.status)).length;
    const p95 = stats?.latency_ms?.p95_ms ?? stats?.p95_ms ?? null;
    const p99 = stats?.latency_ms?.p99_ms ?? null;
    const agentContext = traces.filter((trace) => agentLabel(trace) !== '-').length;
    const spans = traces.reduce((sum, trace) => sum + (trace.span_count ?? 0), 0);
    const slow = traces.filter((trace) => isSlowLatency(trace.total_ms)).length;
    const totalTokens = traces.reduce((sum, trace) => {
      const next = totalTraceTokens(trace);
      return sum + (next ?? 0);
    }, 0);
    const avgTokens = traces.length > 0 ? totalTokens / traces.length : 0;
    const totalInputTokens = traces.reduce((sum, trace) => sum + (trace.input_tokens ?? 0), 0);
    const totalOutputTokens = traces.reduce((sum, trace) => sum + (trace.output_tokens ?? 0), 0);
    return {
      ok,
      failed,
      p95,
      p99,
      slow,
      agentContext,
      spans,
      totalTokens,
      avgTokens,
      totalInputTokens,
      totalOutputTokens,
    };
  }, [stats, traces]);

  const trafficSummary = useMemo(() => {
    const sessions = new Set(trafficFrames.map(trafficSessionId).filter(Boolean)).size;
    const redacted = trafficFrames.reduce((sum, frame) => sum + trafficRedactedPaths(frame).length, 0);
    const bytes = trafficFrames.reduce((sum, frame) => sum + (trafficBodyBytes(frame) ?? 0), 0);
    const transports = new Set(trafficFrames.map((frame) => frame.attributes?.transport).filter(Boolean)).size;
    return { sessions, redacted, bytes, transports };
  }, [trafficFrames]);
  const trafficCaptureStatus = traffic?.capture_status;
  const trafficStatusDetail = useMemo(() => {
    const status = trafficCaptureStatus;
    const base = t(trafficStatusDetailKey(status), {
      captured: status?.captured_decision_count ?? 0,
      skipped: status?.skipped_decision_count ?? 0,
      reasons: compactList(status?.skip_reasons, t('traffic.statusDetail.noReasons')),
    });
    const redacted = status?.redacted_path_count ?? trafficSummary.redacted;
    if (redacted > 0) {
      return `${base} ${t('traffic.statusDetail.redacted', { count: redacted })}`;
    }
    return base;
  }, [t, trafficCaptureStatus, trafficSummary.redacted]);

  const statsSummary = useMemo(() => {
    const failed = stats?.failed_calls ?? Math.max(0, (stats?.total_calls ?? 0) - (stats?.successful_calls ?? 0));
    const success = stats?.successful_calls ?? Math.max(0, (stats?.total_calls ?? 0) - failed);
    return {
      success,
      failed,
      totalTokens: stats?.total_tokens ?? traceSummary.totalTokens,
      totalInputTokens: stats?.total_input_tokens ?? traceSummary.totalInputTokens,
      totalOutputTokens: stats?.total_output_tokens ?? traceSummary.totalOutputTokens,
      avgTokens: stats?.avg_tokens_per_call ?? stats?.avg_total_tokens_per_call ?? traceSummary.avgTokens,
    };
  }, [stats, traceSummary]);

  const tokenPressure = useMemo(() => ({
    total: statsSummary.totalTokens,
    input: statsSummary.totalInputTokens,
    output: statsSummary.totalOutputTokens,
    avg: statsSummary.avgTokens,
    returned: stats?.token_usage?.total_returned_tokens ?? 0,
    saved: stats?.token_usage?.total_saved_tokens ?? 0,
    estimator: stats?.payload_token_estimator ?? health?.response_format?.token_estimator ?? '-',
  }), [health, stats, statsSummary]);

  const slowLatencyDetail = useMemo(() => {
    const slowest = slowTraces[0];
    if (!slowest) {
      return t('stats.detail.slowTraces', { count: slowTraceCount });
    }
    const span = slowest.slowest_span_name
      ? t('traces.detail.slowestSpan', { name: slowest.slowest_span_name, duration: formatDurationMs(slowest.slowest_span_ms) })
      : t('stats.detail.noSlowestSpan');
    return t('stats.detail.slowestTrace', {
      id: compactId(slowest.request_id),
      latency: formatDurationMs(slowest.total_ms),
      span,
    });
  }, [slowTraceCount, slowTraces, t]);

  const debugSignals = useMemo<DebugSignal[]>(() => {
    const signals: DebugSignal[] = [];
    const p95Latency = stats?.latency_ms?.p95_ms ?? stats?.p95_ms ?? null;
    const p99Latency = stats?.latency_ms?.p99_ms ?? null;
    const eventWarnings = problemLogs.length + problemActivity.length;
    if (health && !isOkStatus(health.status)) {
      signals.push({
        key: 'gateway',
        label: t('debug.signal.gatewayHealth'),
        value: health.status,
        detail: t('debug.detail.instancesReady', { ready: health.instances_ready, total: health.instances_total }),
        tone: 'err',
        panel: 'health',
      });
    }
    if (failureSignals.length > 0) {
      const first = failureSignals[0];
      signals.push({
        key: 'failures',
        label: t('debug.signal.failedExecution'),
        value: t('debug.detail.requestCount', { count: failureSignals.length }),
        detail: `${compactId(first.request_id)} · ${first.detail}`,
        tone: 'err',
        panel: 'traces',
        traceId: first.request_id,
      });
    }
    if (unhealthyInstanceRows.length > 0) {
      const first = unhealthyInstanceRows[0];
      signals.push({
        key: 'instances',
        label: t('debug.signal.instanceHealth'),
        value: t('debug.detail.flagged', { count: unhealthyInstanceRows.length }),
        detail: first.failure_reason || first.failure_stage || `${first.dcc_type} ${first.status}`,
        tone: 'warn',
        panel: 'instances',
      });
    }
    if (governanceSummary.denied > 0 || governanceSummary.throttled > 0) {
      signals.push({
        key: 'governance',
        label: t('debug.signal.governancePressure'),
        value: t('debug.detail.governancePressure', { denied: governanceSummary.denied, throttled: governanceSummary.throttled }),
        detail: governanceSummary.redacted ? t('debug.detail.redactedPaths', { count: governanceSummary.redacted }) : t('debug.detail.policyQuota'),
        tone: governanceSummary.denied > 0 ? 'err' : 'warn',
        panel: 'governance',
      });
    }
    if (workflowSummary.zeroResults > 0) {
      signals.push({
        key: 'discovery',
        label: t('debug.signal.discoveryQuality'),
        value: t('debug.detail.zeroResultWorkflows', { count: workflowSummary.zeroResults }),
        detail: t('debug.detail.discoveryReview'),
        tone: 'warn',
        panel: 'workflows',
      });
    }
    if (isSlowLatency(p95Latency) || isSlowLatency(p99Latency)) {
      const slowest = slowTraces[0];
      const slowestSpan = slowest?.slowest_span_name
        ? ` · ${t('traces.detail.slowestSpan', { name: slowest.slowest_span_name, duration: formatDurationMs(slowest.slowest_span_ms) })}`
        : '';
      signals.push({
        key: 'latency',
        label: t('debug.signal.latency'),
        value: `${formatDurationMs(p95Latency)} p95 / ${formatDurationMs(p99Latency)} p99`,
        detail: slowest ? `${compactId(slowest.request_id)} · ${slowest.tool}${slowestSpan}` : t('debug.detail.retainedGatewayCalls'),
        tone: 'warn',
        panel: 'traces',
        traceId: slowest?.request_id,
      });
    }
    if (eventWarnings > 0) {
      signals.push({
        key: 'events',
        label: t('debug.signal.warningEvents'),
        value: t('debug.detail.retained', { count: eventWarnings }),
        detail: problemLogs[0]?.message || problemActivity[0]?.message || t('debug.detail.logsActivityWarnings'),
        tone: 'warn',
        panel: problemLogs.length ? 'logs' : 'activity',
      });
    }
    if (tokenPressure.total > 0) {
      signals.push({
        key: 'tokens',
        label: t('debug.signal.payloadBudget'),
        value: t('debug.detail.perCall', { value: formatTokenCount(tokenPressure.avg) }),
        detail: t('debug.detail.payloadBudget', { total: formatTokenCount(tokenPressure.total), saved: formatTokenCount(tokenPressure.saved) }),
        tone: tokenPressure.avg > 4_000 ? 'warn' : 'ok',
        panel: 'stats',
      });
    }
    signals.push({
      key: 'coverage',
      label: t('debug.signal.evidenceCoverage'),
      value: t('debug.detail.traceCount', { count: traces.length }),
      detail: t('debug.detail.callsWithAgentContext', { calls: calls.length, agents: traceSummary.agentContext }),
      tone: traces.length > 0 && traceSummary.agentContext === 0 ? 'warn' : 'ok',
      panel: 'traces',
    });
    if (signals.length === 1 && signals[0].key === 'coverage' && signals[0].tone === 'ok') {
      return [{
        key: 'ready',
        label: t('debug.signal.gatewayReady'),
        value: t('debug.detail.live', { count: instanceSummary.live }),
        detail: t('debug.detail.noWarnings'),
        tone: 'ok',
        panel: 'health',
      }, signals[0]];
    }
    return signals.slice(0, 8);
  }, [
    calls.length,
    failureSignals,
    governanceSummary,
    health,
    problemActivity,
    problemLogs,
    slowTraces,
    stats,
    t,
    tokenPressure,
    traceSummary.agentContext,
    traces.length,
    unhealthyInstanceRows,
    instanceSummary.live,
    workflowSummary.zeroResults,
  ]);

  const debugIssues = debugSignals.filter((signal) => signal.tone !== 'ok').length;

  const markUpdated = useCallback((panel: Panel, text: string) => {
    setUpdatedAt((current) => ({ ...current, [panel]: text }));
    setErrors((current) => ({ ...current, [panel]: undefined }));
  }, []);

  const markError = useCallback((panel: Panel, error: unknown) => {
    setErrors((current) => ({ ...current, [panel]: error instanceof Error ? error.message : String(error) }));
  }, []);

  const copyText = useCallback(async (text: string, label: string) => {
    if (!text) {
      return;
    }
    try {
      let copied = false;
      if (navigator.clipboard?.writeText) {
        try {
          await navigator.clipboard.writeText(text);
          copied = true;
        } catch {
          copied = false;
        }
      }
      if (!copied) {
        const textarea = document.createElement('textarea');
        textarea.value = text;
        textarea.setAttribute('readonly', 'true');
        textarea.style.position = 'fixed';
        textarea.style.opacity = '0';
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand('copy');
        document.body.removeChild(textarea);
      }
      setCopiedNotice(t('common.notice.copied', { label }));
      window.setTimeout(() => setCopiedNotice(''), 1800);
    } catch (error) {
      setCopiedNotice(t('common.notice.copyFailed', { message: error instanceof Error ? error.message : String(error) }));
      window.setTimeout(() => setCopiedNotice(''), 2400);
    }
  }, [t]);

  const openConfigLocation = useCallback((target: IdeTarget, configPath: string) => {
    const href = configPathFileUrl(configPath);
    if (href) {
      window.open(href, '_blank', 'noopener,noreferrer');
      setCopiedNotice(t('common.notice.openedConfigPath', { label: target.label }));
      window.setTimeout(() => setCopiedNotice(''), 1800);
      return;
    }
    void copyText(configPath, `${target.label} config path`);
  }, [copyText, t]);

  const copyIssueReport = useCallback(async (requestId: string) => {
    try {
      const text = await issueReportJsonText(requestId);
      await copyText(text, 'issue report JSON');
    } catch (error) {
      setCopiedNotice(t('common.notice.issueReportFailed', { message: error instanceof Error ? error.message : String(error) }));
      window.setTimeout(() => setCopiedNotice(''), 2400);
    }
  }, [copyText, t]);

  const downloadIssueReport = useCallback(async (requestId: string) => {
    try {
      const text = await issueReportJsonText(requestId);
      downloadJsonText(issueReportFilename(requestId), text);
      setCopiedNotice(t('common.notice.downloadedIssueReport'));
      window.setTimeout(() => setCopiedNotice(''), 1800);
    } catch (error) {
      setCopiedNotice(t('common.notice.issueReportFailed', { message: error instanceof Error ? error.message : String(error) }));
      window.setTimeout(() => setCopiedNotice(''), 2400);
    }
  }, [t]);

  const fetchActivity = useCallback(async () => {
    try {
      const payload = await apiJson<{ events: ActivityEvent[] }>('/activity?limit=300');
      setActivity(Array.isArray(payload.events) ? payload.events : []);
      markUpdated('activity', t('common.updated.events', { count: payload.events?.length ?? 0, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('activity', error);
    }
  }, [markError, markUpdated, t]);

  const fetchHealth = useCallback(async () => {
    try {
      const payload = await apiJson<HealthPayload>('/health');
      setHealth(payload);
      markUpdated('health', t('common.updated.lastUpdated', { time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('health', error);
    }
  }, [markError, markUpdated, t]);

  const fetchInstanceBackends = useCallback(async () => {
    try {
      const payload = await apiJson<{ workers: InstanceRow[]; summary: InstanceSummary }>('/workers');
      setInstanceRows(payload.workers);
      setInstanceSummary(payload.summary);
      markUpdated(
        'instances',
        t('common.updated.instances', { count: payload.workers.length, live: payload.summary.live, stale: payload.summary.stale, unhealthy: payload.summary.unhealthy, time: new Date().toLocaleTimeString() }),
      );
    } catch (error) {
      markError('instances', error);
    }
  }, [markError, markUpdated, t]);

  const fetchTools = useCallback(async () => {
    try {
      const payload = await apiJson<{ tools: ToolRow[] }>('/tools');
      setTools(payload.tools);
      markUpdated('tools', t('common.updated.tools', { count: payload.tools.length, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('tools', error);
    }
  }, [markError, markUpdated, t]);

  const fetchOpenApi = useCallback(async () => {
    try {
      const { spec, raw } = await fetchOpenApiSpecText(openApiSource.specUrl);
      const operations = flattenOpenApiOperations(spec);
      setOpenApiSpec(spec);
      setOpenApiRaw(raw);
      markUpdated('openapi', t('common.updated.operations', { label: openApiSource.label, count: operations.length, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('openapi', error);
    }
  }, [markError, markUpdated, openApiSource, t]);

  const fetchCalls = useCallback(async () => {
    try {
      const payload = await apiJson<{ calls: CallRow[] }>('/calls');
      const rows = Array.isArray(payload.calls) ? payload.calls : [];
      setCalls(rows);
      markUpdated('calls', t('common.updated.calls', { count: rows.length, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('calls', error);
    }
  }, [markError, markUpdated, t]);

  const fetchTraces = useCallback(async () => {
    try {
      const payload = await apiJson<{ traces: TraceRow[] }>('/traces?limit=200');
      const rows = Array.isArray(payload.traces) ? payload.traces : [];
      setTraces(rows);
      markUpdated('traces', t('common.updated.traces', { count: rows.length, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('traces', error);
    }
  }, [markError, markUpdated, t]);

  const fetchTraffic = useCallback(async () => {
    try {
      const payload = await apiJson<TrafficPayload>('/traffic?limit=300');
      const rows = Array.isArray(payload.frames) ? payload.frames : [];
      setTraffic({ ...payload, frames: rows });
      markUpdated('traffic', t('common.updated.frames', { count: rows.length, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('traffic', error);
    }
  }, [markError, markUpdated, t]);

  const fetchTasks = useCallback(async () => {
    try {
      const payload = await apiJson<{ tasks: TaskRow[] }>('/tasks?limit=300');
      setTasks(Array.isArray(payload.tasks) ? payload.tasks : []);
      markUpdated('tasks', t('common.updated.tasks', { count: payload.tasks?.length ?? 0, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('tasks', error);
    }
  }, [markError, markUpdated, t]);

  const fetchWorkflows = useCallback(async () => {
    try {
      const payload = await apiJson<{ workflows: WorkflowRow[] }>('/workflows?limit=200');
      const rows = Array.isArray(payload.workflows) ? payload.workflows : [];
      setWorkflows(rows);
      markUpdated('workflows', t('common.updated.workflows', { count: rows.length, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('workflows', error);
    }
  }, [markError, markUpdated, t]);

  const fetchStats = useCallback(async () => {
    try {
      const payload = await apiJson<StatsPayload>(`/stats?range=${encodeURIComponent(statsRange)}`);
      setStats(payload);
      markUpdated('stats', t('common.updated.range', { range: payload.range, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('stats', error);
    }
  }, [markError, markUpdated, statsRange, t]);

  const fetchGovernance = useCallback(async () => {
    try {
      const payload = await apiJson<GovernancePayload>('/governance?limit=300');
      setGovernance(payload);
      markUpdated('governance', t('common.updated.decisions', { count: payload.recent_decisions?.length ?? 0, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('governance', error);
    }
  }, [markError, markUpdated, t]);

  const fetchLogs = useCallback(async () => {
    try {
      const payload = await apiJson<{ logs?: unknown[] }>('/logs');
      const raw = Array.isArray(payload.logs) ? payload.logs : [];
      setLogs(raw.map(normalizeLogRow));
      markUpdated('logs', t('common.updated.events', { count: raw.length, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      markError('logs', error);
    }
  }, [markError, markUpdated, t]);

  const fetchSkillPaths = useCallback(async () => {
    try {
      const payload = await apiJson<{ paths: SkillPathRow[] }>('/skill-paths');
      setSkillPaths(Array.isArray(payload.paths) ? payload.paths.map(normalizeSkillPathRow) : []);
      markUpdated(
        'skill-paths',
        t('common.updated.paths', { count: payload.paths?.length ?? 0, time: new Date().toLocaleTimeString() }),
      );
    } catch (error) {
      markError('skill-paths', error);
    }
  }, [markError, markUpdated, t]);

  const fetchSkills = useCallback(async () => {
    try {
      const payload = await apiJson<SkillPayload>('/skills');
      const rows = Array.isArray(payload.skills) ? payload.skills.map(normalizeSkillRow) : [];
      const health = payload.health ?? {};
      setSkills(rows);
      setSkillTotals({
        total: Number(payload.total ?? rows.length),
        loaded: Number(payload.loaded ?? rows.filter((skill) => skill.loaded).length),
        unloaded: Number(payload.unloaded ?? rows.filter((skill) => !skill.loaded).length),
        action_count: Number(payload.action_count ?? rows.reduce((sum, skill) => sum + skill.action_count, 0)),
        searched: Number(health.searched_skills ?? rows.filter((skill) => skill.adoption.searched).length),
        used: Number(health.used_skills ?? rows.filter((skill) => skill.adoption.used).length),
        low_adoption: Number(health.low_adoption_skills ?? rows.filter((skill) => skill.adoption.low_adoption).length),
        load_errors: Number(health.load_error_count ?? rows.reduce((sum, skill) => sum + skill.adoption.load_error_count, 0)),
        missing_paths: Number(health.missing_path_count ?? 0),
      });
      markUpdated(
        'skill-paths',
        t('common.updated.skillInventory', { loaded: payload.loaded ?? rows.filter((skill) => skill.loaded).length, actions: payload.action_count ?? rows.reduce((sum, skill) => sum + skill.action_count, 0), time: new Date().toLocaleTimeString() }),
      );
    } catch (error) {
      markError('skill-paths', error);
    }
  }, [markError, markUpdated, t]);

  const fetchSkillDetail = useCallback(async (skill: SkillRow) => {
    setSelectedSkill(skill);
    setSkillDetailBusy(true);
    setSkillDetail(null);
    try {
      const params = new URLSearchParams({ name: skill.name });
      if (skill.dcc_type) {
        params.set('dcc_type', skill.dcc_type);
      }
      const instanceId = skill.instance_details[0]?.id || skill.instance_ids[0];
      if (instanceId) {
        params.set('instance_id', instanceId);
      }
      const payload = await apiJson<SkillDetailPayload>(`/skill-detail?${params.toString()}`);
      setSkillDetail(normalizeSkillDetailPayload(payload));
      markUpdated('skill-paths', t('common.updated.skillDetail', { name: skill.name, time: new Date().toLocaleTimeString() }));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSkillDetail({ skill: null, instances: [], error: message });
      markError('skill-paths', error);
    } finally {
      setSkillDetailBusy(false);
    }
  }, [markError, markUpdated, t]);

  const fetchSkillInventory = useCallback(async () => {
    await Promise.allSettled([fetchSkillPaths(), fetchSkills()]);
  }, [fetchSkillPaths, fetchSkills]);

  const fetchSetup = useCallback(async () => {
    await Promise.allSettled([fetchHealth(), fetchInstanceBackends()]);
    markUpdated('setup', t('common.updated.gatewayTarget', { time: new Date().toLocaleTimeString() }));
  }, [fetchHealth, fetchInstanceBackends, markUpdated, t]);

  const fetchDebug = useCallback(async () => {
    await Promise.allSettled([
      fetchHealth(),
      fetchInstanceBackends(),
      fetchActivity(),
      fetchCalls(),
      fetchTraces(),
      fetchTraffic(),
      fetchStats(),
      fetchGovernance(),
      fetchLogs(),
    ]);
    markUpdated('debug', t('common.updated.debugSnapshot', { time: new Date().toLocaleTimeString() }));
  }, [fetchActivity, fetchCalls, fetchGovernance, fetchHealth, fetchInstanceBackends, fetchLogs, fetchStats, fetchTraces, fetchTraffic, markUpdated, t]);

  const addSkillPath = useCallback(async () => {
    const path = skillPathInput.trim();
    if (!path) {
      return;
    }
    setSkillPathBusy(true);
    try {
      const ctrl = new AbortController();
      const tid = window.setTimeout(() => ctrl.abort(), ADMIN_FETCH_TIMEOUT_MS);
      try {
        const res = await fetch(`${API_BASE}/skill-paths`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ path }),
          signal: ctrl.signal,
        });
        if (!res.ok) {
          throw new Error(`${res.status} ${res.statusText}`);
        }
      } catch (err) {
        if (err instanceof DOMException && err.name === 'AbortError') {
          throw new Error(`Request timed out after ${ADMIN_FETCH_TIMEOUT_MS / 1000}s`);
        }
        throw err;
      } finally {
        clearTimeout(tid);
      }
      setSkillPathInput('');
      await fetchSkillInventory();
    } catch (error) {
      markError('skill-paths', error);
    } finally {
      setSkillPathBusy(false);
    }
  }, [fetchSkillInventory, markError, skillPathInput]);

  const deleteSkillPath = useCallback(
    async (id: number) => {
      setSkillPathBusy(true);
      try {
        const ctrl = new AbortController();
        const tid = window.setTimeout(() => ctrl.abort(), ADMIN_FETCH_TIMEOUT_MS);
        try {
          const res = await fetch(`${API_BASE}/skill-paths/${encodeURIComponent(String(id))}`, {
            method: 'DELETE',
            signal: ctrl.signal,
          });
          if (!res.ok) {
            throw new Error(`${res.status} ${res.statusText}`);
          }
        } catch (err) {
          if (err instanceof DOMException && err.name === 'AbortError') {
            throw new Error(`Request timed out after ${ADMIN_FETCH_TIMEOUT_MS / 1000}s`);
          }
          throw err;
        } finally {
          clearTimeout(tid);
        }
        await fetchSkillInventory();
      } catch (error) {
        markError('skill-paths', error);
      } finally {
        setSkillPathBusy(false);
      }
    },
    [fetchSkillInventory, markError],
  );

  const fetchTraceInto = useCallback(async (requestId: string, target: 'call' | 'trace') => {
    try {
      const payload = await apiJson<unknown>(`/traces/${encodeURIComponent(requestId)}`);
      const detail = JSON.stringify(payload, null, 2);
      if (target === 'call') {
        setCallDetail(detail);
      } else {
        setTraceDetail(detail);
        setTraceDetailPayload(payload as TraceDetailPayload);
      }
    } catch (error) {
      const detail = `Error: ${error instanceof Error ? error.message : String(error)}`;
      if (target === 'call') {
        setCallDetail(detail);
      } else {
        setTraceDetail(detail);
        setTraceDetailPayload(null);
      }
    }
  }, []);

  const pushAdminUrl = useCallback(
    (panel: Panel, opts?: { traceId?: string | null; range?: string | null; openApiSource?: OpenApiSource | null; replace?: boolean }) => {
      const u = new URL(window.location.href);
      u.searchParams.set('panel', panel);
      u.searchParams.delete('range');
      u.searchParams.delete('trace');
      u.searchParams.delete('spec');
      u.searchParams.delete('docs');
      u.searchParams.delete('label');
      if (panel === 'stats') {
        const r = opts?.range;
        if (r && STATS_RANGE_IDS.has(r)) {
          u.searchParams.set('range', r);
        }
      }
      if (panel === 'traces' && opts?.traceId) {
        u.searchParams.set('trace', opts.traceId);
      }
      if (panel === 'openapi' && opts?.openApiSource && opts.openApiSource.kind === 'instance') {
        u.searchParams.set('spec', opts.openApiSource.specUrl);
        u.searchParams.set('docs', opts.openApiSource.docsUrl);
        u.searchParams.set('label', opts.openApiSource.label);
      }
      const next = `${u.pathname}${u.search}`;
      const cur = `${window.location.pathname}${window.location.search}`;
      if (next === cur) {
        return;
      }
      if (opts?.replace) {
        window.history.replaceState({ panel }, '', next);
      } else {
        window.history.pushState({ panel }, '', next);
      }
    },
    [],
  );

  const goToPanel = useCallback(
    (panel: Panel, opts?: { traceId?: string; range?: string; openApiSource?: OpenApiSource; replace?: boolean }) => {
      let effectiveRange = statsRange;
      if (opts?.range && STATS_RANGE_IDS.has(opts.range)) {
        effectiveRange = opts.range;
        setStatsRange(opts.range);
      }
      if (panel === 'openapi') {
        setOpenApiSource(opts?.openApiSource ?? gatewayOpenApiSource());
      }
      setActivePanel(panel);
      pushAdminUrl(panel, {
        traceId: opts?.traceId,
        range: panel === 'stats' ? effectiveRange : null,
        openApiSource: panel === 'openapi' ? (opts?.openApiSource ?? gatewayOpenApiSource()) : null,
        replace: opts?.replace,
      });
      if (panel === 'traces' && opts?.traceId) {
        void fetchTraceInto(opts.traceId, 'trace');
      } else if (panel === 'traces' && !opts?.traceId) {
        setTraceDetail('Select a trace row for detail.');
        setTraceDetailPayload(null);
      }
    },
    [fetchTraceInto, pushAdminUrl, statsRange],
  );

  useEffect(() => {
    const onPop = () => {
      const panel = readPanelFromUrl();
      setActivePanel(panel);
      setStatsRange(readStatsRangeFromUrl());
      setOpenApiSource(readOpenApiSourceFromUrl());
      const tid = readTraceIdFromUrl();
      if (panel === 'traces' && tid) {
        void fetchTraceInto(tid, 'trace');
      } else if (panel === 'traces') {
        setTraceDetail('Select a trace row for detail.');
        setTraceDetailPayload(null);
      }
    };
    window.addEventListener('popstate', onPop);
    return () => window.removeEventListener('popstate', onPop);
  }, [fetchTraceInto]);

  useEffect(() => {
    const panel = readPanelFromUrl();
    setOpenApiSource(readOpenApiSourceFromUrl());
    const tid = readTraceIdFromUrl();
    if (panel === 'traces' && tid) {
      void fetchTraceInto(tid, 'trace');
    }
  }, [fetchTraceInto]);

  const fetchPanel = useCallback((panel: Panel) => {
    if (panel === 'setup') void fetchSetup();
    if (panel === 'debug') void fetchDebug();
    if (panel === 'activity') void fetchActivity();
    if (panel === 'health') void fetchHealth();
    if (panel === 'instances') void fetchInstanceBackends();
    if (panel === 'tools') void fetchTools();
    if (panel === 'openapi') void fetchOpenApi();
    if (panel === 'workflows') void fetchWorkflows();
    if (panel === 'tasks') void fetchTasks();
    if (panel === 'calls') void fetchCalls();
    if (panel === 'traces') void fetchTraces();
    if (panel === 'traffic') void fetchTraffic();
    if (panel === 'stats') void Promise.allSettled([fetchStats(), fetchCalls(), fetchTraces()]);
    if (panel === 'governance') void fetchGovernance();
    if (panel === 'skill-paths') void fetchSkillInventory();
    if (panel === 'logs') void fetchLogs();
  }, [fetchActivity, fetchCalls, fetchDebug, fetchGovernance, fetchHealth, fetchInstanceBackends, fetchLogs, fetchOpenApi, fetchSetup, fetchSkillInventory, fetchStats, fetchTasks, fetchTools, fetchTraces, fetchTraffic, fetchWorkflows]);

  useEffect(() => {
    fetchPanel(activePanel);
    const timer = window.setInterval(() => fetchPanel(activePanel), 5000);
    return () => window.clearInterval(timer);
  }, [activePanel, fetchPanel]);

  const hasLatencyFilter = activePanel === 'calls' || activePanel === 'traces';
  const showListSearchMeta = Boolean(listSearch.trim()) || (hasLatencyFilter && slowOnly);
  const latencyThresholdDetail = t('common.detail.slowThreshold', {
    slow: formatDurationMs(SLOW_LATENCY_MS),
    tail: formatDurationMs(CRITICAL_LATENCY_MS),
  });

  return (
    <div className="app-shell">
      <nav className="side-rail" aria-label={t('common.aria.adminNavigation')}>
        <div className="brand-lockup">
          <div className="brand-accent" aria-hidden="true" />
          <div className="brand-text">
            <h1>{t('chrome.app.title')}</h1>
            <p className="brand-tag">{t('chrome.app.subtitle')}</p>
          </div>
        </div>
        <LanguageSelector
          locale={localeDetection.locale}
          source={localeDetection.source}
          onChange={changeLocale}
          t={t}
        />
        <div className="nav-links">
          {panels.map((panel, index) => {
            const showGroup = index === 0 || panels[index - 1].group !== panel.group;
            return (
              <div className="nav-entry" key={panel.id}>
                {showGroup ? <div className="nav-section-title">{panel.group}</div> : null}
                <a
                  href={hrefForAdmin(panel.id, panel.id === 'stats' ? { range: statsRange } : undefined)}
                  className={panel.id === activePanel ? 'nav-link active' : 'nav-link'}
                  aria-current={panel.id === activePanel ? 'page' : undefined}
                  onClick={(e) => {
                    e.preventDefault();
                    goToPanel(panel.id);
                  }}
                >
                  <NavIcon panel={panel.id} />
                  <span>{panel.label}</span>
                </a>
              </div>
            );
          })}
          <div className="nav-entry">
            <a
              href={projectDocsHref()}
              className="nav-link"
              target="_blank"
              rel="noopener noreferrer"
              title={t('navigation.docs.title')}
            >
              <DocsIcon />
              <span>{t('navigation.docs.label')}</span>
            </a>
          </div>
        </div>
      </nav>
      <main className="main-stage">
        {activePanel !== 'setup' && activePanel !== 'health' && activePanel !== 'debug' && (
          <div className="list-search-wrap">
            <input
              type="search"
              className="list-search-input"
              placeholder={activePanel === 'stats' ? t('search.input.stats') : activePanel === 'openapi' ? t('search.input.openapi') : t('search.input.default')}
              value={listSearch}
              onChange={(e) => setListSearch(e.target.value)}
              aria-label={t('search.input.ariaLabel')}
            />
            {hasLatencyFilter ? (
              <button
                className={`filter-chip ${slowOnly ? 'active' : ''}`}
                type="button"
                aria-pressed={slowOnly}
                title={latencyThresholdDetail}
                onClick={() => setSlowOnly((value) => !value)}
              >
                {slowOnly ? t('common.filter.allLatency') : t('common.filter.slowOnly')}
              </button>
            ) : null}
            {showListSearchMeta ? (
              <span className="list-search-meta">
                {activePanel === 'activity' ? `${filteredActivity.length} / ${activity.length}` : ''}
                {activePanel === 'instances' ? `${filteredInstanceRows.length} / ${instanceRows.length}` : ''}
                {activePanel === 'tools' ? `${filteredTools.length} / ${tools.length}` : ''}
                {activePanel === 'workflows' ? `${filteredWorkflows.length} / ${workflows.length}` : ''}
                {activePanel === 'openapi' ? `${filteredOpenApiOperations.length} / ${openApiOperations.length}` : ''}
                {activePanel === 'tasks' ? `${filteredTasks.length} / ${tasks.length}` : ''}
                {activePanel === 'calls' ? `${filteredCalls.length} / ${calls.length}` : ''}
                {activePanel === 'traces' ? `${filteredTraces.length} / ${traces.length}` : ''}
                {activePanel === 'traffic' ? `${filteredTrafficFrames.length} / ${trafficFrames.length}` : ''}
                {activePanel === 'governance' ? `${filteredGovernanceDecisions.length} / ${governance?.recent_decisions?.length ?? 0}` : ''}
                {activePanel === 'skill-paths' ? t('search.meta.skillsPaths', { skills: filteredSkills.length, paths: filteredSkillPaths.length }) : ''}
                {activePanel === 'logs' ? `${filteredLogs.length} / ${logs.length}` : ''}
                {activePanel === 'stats' ? t('search.meta.statsCharts', {
                  apps: filteredTopAppTypes.length,
                  tools: filteredTopTools.length,
                  instances: filteredTopInstances.length,
                  agents: filteredTopAgents.length,
                  actors: filteredTopActors.length,
                  platforms: filteredTopClientPlatforms.length,
                  sources: filteredTopSourceIps.length,
                  formats: filteredTokenByFormat.length,
                }) : ''}
                {activePanel === 'governance' ? t('search.meta.governancePressure', { denied: governanceSummary.denied, throttled: governanceSummary.throttled }) : ''}
              </span>
            ) : null}
          </div>
        )}
        {activePanel === 'setup' && (
          <section className="panel active setup-panel">
            <PanelHeader
              title={t('navigation.panel.setup')}
              meta={setupMcpUrl}
              action={<button className="refresh-btn" type="button" onClick={fetchSetup}>{t('common.action.refresh')}</button>}
            />
            <StatusLine text={copiedNotice || updatedAt.setup} error={errors.setup} />
            <div className="setup-controls">
              <div className="setup-mode-group" role="group" aria-label={t('setup.aria.endpoint')}>
                <button
                  className={setupUrlMode === 'local' ? 'setup-mode active' : 'setup-mode'}
                  type="button"
                  aria-pressed={setupUrlMode === 'local'}
                  onClick={() => setSetupUrlMode('local')}
                >
                  {t('setup.mode.local')}
                </button>
                <button
                  className={setupUrlMode === 'lan' ? 'setup-mode active' : 'setup-mode'}
                  type="button"
                  aria-pressed={setupUrlMode === 'lan'}
                  disabled={!lanUrl}
                  onClick={() => lanUrl && setSetupUrlMode('lan')}
                >
                  {t('setup.mode.lan')}
                </button>
                <button
                  className={setupUrlMode === 'direct' ? 'setup-mode active' : 'setup-mode'}
                  type="button"
                  aria-pressed={setupUrlMode === 'direct'}
                  disabled={directSetupInstanceRows.length === 0}
                  onClick={() => directSetupInstanceRows.length > 0 && setSetupUrlMode('direct')}
                >
                  {t('setup.mode.direct')}
                </button>
              </div>
              <div className="setup-url-box">
                <span>{t('setup.label.url')}</span>
                <code>{setupMcpUrl}</code>
                <button className="copy-btn" type="button" onClick={() => copyText(setupMcpUrl, 'MCP URL')}>
                  {t('common.action.copy')}
                </button>
              </div>
              {setupUrlMode === 'direct' ? (
                <label className="setup-instance-picker">
                  <span>Instance</span>
                  <select
                    value={selectedDirectInstance?.instance_id ?? ''}
                    onChange={(event) => setDirectInstanceId(event.target.value)}
                    disabled={directSetupInstanceRows.length === 0}
                  >
                    {directSetupInstanceRows.map((instance) => (
                      <option key={instance.instance_id} value={instance.instance_id}>
                        {instanceSetupLabel(instance)}
                      </option>
                    ))}
                  </select>
                </label>
              ) : null}
            </div>
            <div className="ide-grid">
              {IDE_TARGETS.map((target) => {
                const config = ideConfigText(target, setupMcpUrl);
                const configPath = configPathForTarget(target, clientPlatform);
                return (
                  <article key={target.id} className="ide-card">
                    <div className="ide-card-head">
                      <IdeIcon target={target} />
                      <div>
                        <h3>{target.label}</h3>
                        <p className="mono-path">{configPath}</p>
                      </div>
                    </div>
                    <pre className="ide-config-preview">{config}</pre>
                    <div className="ide-card-actions">
                      <button className="copy-btn" type="button" onClick={() => copyText(config, `${target.label} config`)}>
                        Copy
                      </button>
                      <button className="refresh-btn" type="button" onClick={() => openConfigLocation(target, configPath)}>
                        Open file
                      </button>
                    </div>
                  </article>
                );
              })}
            </div>
          </section>
        )}
        {activePanel === 'debug' && (
          <section className="panel active debug-panel">
            <div className="debug-hero">
              <div>
                <h2>{t('debug.title.workbench')}</h2>
                <StatusLine text={updatedAt.debug} error={errors.debug} />
              </div>
              <div className="debug-pulse">
                <span className={debugIssues > 0 ? 'pulse-dot warn' : 'pulse-dot ok'} />
                {debugIssues > 0 ? t('debug.status.attention', { count: debugIssues }) : t('debug.status.clean')}
              </div>
            </div>
            <div className="debug-grid">
              <HealthCard tone={health?.status === 'ok' ? 'ok' : 'warn'} label={t('debug.metric.gateway')} value={gatewayLabel(health)} />
              <HealthCard tone={unhealthyInstanceRows.length ? 'warn' : 'ok'} label={t('debug.metric.instances')} value={t('debug.detail.liveFlagged', { live: instanceSummary.live, flagged: unhealthyInstanceRows.length })} />
              <HealthCard tone={errorRateTone(stats)} label={t('debug.metric.success')} value={stats ? `${stats.success_rate.toFixed(1)}%` : '?'} />
              <HealthCard tone={latencyTone(stats?.latency_ms?.p95_ms ?? stats?.p95_ms)} label={t('debug.metric.latency')} value={stats?.latency_ms?.p95_ms ?? stats?.p95_ms ?? '-'} />
              <HealthCard label={t('debug.metric.tokensPerCall')} value={formatTokenCount(tokenPressure.avg)} />
            </div>
            <div className="debug-map">
              <div className="debug-card debug-wide">
                <div className="debug-card-head">
                  <h3>{t('debug.section.agentTriage')}</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('traces')}>{t('debug.action.openEvidence')}</button>
                </div>
                <div className="debug-signal-list">
                  {debugSignals.map((signal) => (
                    <button
                      key={signal.key}
                      className={`debug-signal ${signal.tone}`}
                      type="button"
                      onClick={() => goToPanel(signal.panel, signal.traceId ? { traceId: signal.traceId } : undefined)}
                    >
                      <span>{signal.label}</span>
                      <strong>{signal.value}</strong>
                      <em>{signal.detail}</em>
                    </button>
                  ))}
                </div>
              </div>

              <div className="debug-card debug-wide">
                <div className="debug-card-head">
                  <h3>{t('debug.section.trafficShape')}</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('stats')}>{t('debug.action.openStats')}</button>
                </div>
                <MiniSparkline buckets={stats?.hourly_distribution ?? []} t={t} />
                <div className="debug-metrics">
                  <span>{stats?.total_calls ?? 0} calls</span>
                  <span>{formatDurationMs(stats?.latency_ms?.p50_ms ?? stats?.p50_ms)} p50</span>
                  <span>{formatDurationMs(stats?.latency_ms?.p95_ms ?? stats?.p95_ms)} p95</span>
                  <span>{formatDurationMs(stats?.latency_ms?.p99_ms)} p99</span>
                  <span>{slowLatencyDetail}</span>
                  <span>{formatTokenCount(tokenPressure.total)} payload tokens</span>
                </div>
              </div>

              <div className="debug-card">
                <div className="debug-card-head">
                  <h3>{t('debug.section.tokenPressure')}</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('stats')}>{t('debug.action.openStats')}</button>
                </div>
                <div className="debug-metrics">
                  <span>{formatTokenCount(tokenPressure.total)} total</span>
                  <span>{formatTokenCount(tokenPressure.input)} in</span>
                  <span>{formatTokenCount(tokenPressure.output)} out</span>
                  <span>{t('debug.detail.saved', { value: formatTokenCount(tokenPressure.saved) })}</span>
                  <span>{tokenPressure.estimator}</span>
                </div>
                {tokenHeavyTraces.length === 0 ? <p className="empty">{t('debug.empty.tokenPressure')}</p> : tokenHeavyTraces.map((trace) => (
                  <button key={trace.request_id} className="debug-row" type="button" onClick={() => goToPanel('traces', { traceId: trace.request_id })}>
                    <span>{formatTokenCount(totalTraceTokens(trace))} tok</span>
                    <span>{compactId(trace.request_id)}</span>
                    <span title={trace.tool}>{trace.tool}</span>
                  </button>
                ))}
              </div>

              <div className="debug-card">
                <div className="debug-card-head">
                  <h3>{t('debug.section.failures')}</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('calls')}>{t('debug.action.openCalls')}</button>
                </div>
                {failureSignals.length === 0 ? <p className="empty">{t('debug.empty.failures')}</p> : failureSignals.map((failure) => (
                  <button key={failure.request_id} className="debug-row" type="button" onClick={() => goToPanel('traces', { traceId: failure.request_id })}>
                    <span><StatusBadge value={failure.status} /></span>
                    <span>{compactId(failure.request_id)}</span>
                    <span title={`${failure.tool} · ${failure.detail}`}>{failure.detail}</span>
                  </button>
                ))}
              </div>

              <div className="debug-card">
                <div className="debug-card-head">
                  <h3>{t('debug.section.slowestTraces')}</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('traces')}>{t('debug.action.openTraces')}</button>
                </div>
                {slowTraces.length === 0 ? <p className="empty">{t('debug.empty.latency')}</p> : slowTraces.map((trace) => (
                  <button key={trace.request_id} className={`debug-row ${latencyClass(trace.total_ms)}`} type="button" onClick={() => goToPanel('traces', { traceId: trace.request_id })}>
                    <LatencyValue value={trace.total_ms} t={t} />
                    <span>{compactId(trace.request_id)}</span>
                    <span title={trace.tool}>
                      {trace.tool}
                      {trace.slowest_span_name ? ` - ${t('traces.detail.slowestSpan', { name: trace.slowest_span_name, duration: formatDurationMs(trace.slowest_span_ms) })}` : ''}
                    </span>
                  </button>
                ))}
              </div>

              <div className="debug-card">
                <div className="debug-card-head">
                  <h3>{t('debug.section.instanceSignals')}</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('instances')}>{t('debug.action.openInstances')}</button>
                </div>
                {unhealthyInstanceRows.length === 0 ? <p className="empty">{t('debug.empty.instances')}</p> : unhealthyInstanceRows.slice(0, 8).map((instance) => (
                  <div key={instance.instance_id} className="debug-row static">
                    <span><StatusBadge value={instance.stale ? 'stale' : instance.status} /></span>
                    <span>{instance.dcc_type}</span>
                    <span title={instance.failure_reason ?? instance.failure_stage ?? instance.instance_id}>
                      {instance.display_name} · {instance.failure_reason ?? instance.failure_stage ?? compactId(instance.instance_id)}
                    </span>
                  </div>
                ))}
              </div>

              <div className="debug-card debug-wide">
                <div className="debug-card-head">
                  <h3>{t('debug.section.openapiEntryPoints')}</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('openapi')}>{t('debug.action.gatewaySpec')}</button>
                </div>
                {instanceRows.length === 0 ? <p className="empty">{t('debug.empty.openapi')}</p> : (
                  Array.from(groupRows(instanceRows.slice(0, 8), instanceGroupLabel).entries())
                    .sort(([a], [b]) => a.localeCompare(b))
                    .map(([group, groupInstances]) => (
                      <div key={group} className="contract-group">
                        <h4>{group}</h4>
                        {groupInstances.map((instance) => (
                          <div key={instance.instance_id} className="contract-row">
                            <span>
                              <strong>{instance.display_name}</strong>
                              <em>{instance.dcc_type} · {compactInstanceId(instance.instance_id)}</em>
                            </span>
                            <BackendOpenApiLinks instance={instance} />
                          </div>
                        ))}
                      </div>
                    ))
                )}
              </div>

              <div className="debug-card">
                <div className="debug-card-head">
                  <h3>{t('debug.section.eventWarnings')}</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('logs')}>{t('debug.action.openLogs')}</button>
                </div>
                {[...problemLogs, ...problemActivity.map((event) => ({
                  timestamp: event.timestamp,
                  level: event.severity,
                  message: event.message,
                  source: event.kind,
                  request_id: event.correlation?.request_id,
                  dcc_type: event.correlation?.dcc_type,
                } as LogRow))].slice(0, 10).map((row, index) => (
                  <button
                    key={`${row.timestamp}-${row.message}-${index}`}
                    className="debug-row"
                    type="button"
                    onClick={() => row.request_id ? goToPanel('traces', { traceId: row.request_id }) : goToPanel('logs')}
                  >
                    <TimeValue value={row.timestamp} />
                    <span>{row.source ?? row.level}</span>
                    <span title={row.message}>{row.message}</span>
                  </button>
                ))}
                {problemLogs.length === 0 && problemActivity.length === 0 ? <p className="empty">{t('debug.empty.events')}</p> : null}
              </div>
            </div>
            <button className="refresh-btn" type="button" onClick={fetchDebug}>{t('debug.action.refreshSnapshot')}</button>
          </section>
        )}
        {activePanel === 'activity' && (
          <section className="panel active activity-panel">
            <h2>{t('activity.title')}</h2>
            <StatusLine text={updatedAt.activity} error={errors.activity} />
            {activity.length === 0 ? <p className="empty">{t('activity.empty.none')}</p> : filteredActivity.length === 0 ? (
              <p className="empty">{t('activity.empty.search')}</p>
            ) : (
              <table>
                <thead><tr><th>{t('common.table.time')}</th><th>{t('common.table.status')}</th><th>{t('common.table.kind')}</th><th>{t('common.table.message')}</th><th>{t('common.table.dcc')}</th><th>{t('common.table.actor')}</th><th>{t('common.table.platform')}</th><th>{t('common.table.sourceIp')}</th><th>{t('common.table.request')}</th><th>{t('common.table.ms')}</th></tr></thead>
                <tbody>
                  {filteredActivity.map((event) => {
                    const requestId = event.correlation?.request_id;
                    return (
                      <tr key={event.event_id}>
                        <td><TimeValue value={event.timestamp} /></td>
                        <td><StatusBadge value={event.status} /></td>
                        <td>{event.kind}</td>
                        <td title={event.message}>{event.message}</td>
                        <td>{event.correlation?.dcc_type ?? '-'}</td>
                        <td>{event.correlation?.actor_name ?? event.correlation?.actor_id ?? '-'}</td>
                        <td>{event.correlation?.client_platform ?? '-'}</td>
                        <td>{event.correlation?.source_ip ?? '-'}</td>
                        <td>
                          {requestId ? (
                            <button className="refresh-btn" type="button" title={requestId} onClick={() => goToPanel('traces', { traceId: requestId })}>
                              {requestId.slice(0, 12)}
                            </button>
                          ) : (
                            '-'
                          )}
                        </td>
                        <td>{event.duration_ms ?? '-'}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
            <button className="refresh-btn" type="button" onClick={fetchActivity}>{t('common.action.refresh')}</button>
          </section>
        )}

        {activePanel === 'health' && (
          <section className="panel active health-panel">
            <h2>{t('health.title')}</h2>
            <StatusLine text={updatedAt.health} error={errors.health} />
            <div className="health-grid">
              <HealthCard tone={health?.status === 'ok' ? 'ok' : 'warn'} label={t('health.metric.status')} value={health?.status ?? '?'} />
              <HealthCard label={t('health.metric.uptime')} value={formatUptime(health?.uptime_secs)} />
              <HealthCard tone={health && health.instances_ready > 0 ? 'ok' : 'warn'} label={t('health.metric.ready')} value={`${health?.instances_ready ?? 0} / ${health?.instances_total ?? 0}`} />
              <HealthCard label={t('health.metric.version')} value={health?.version ?? '?'} />
              <HealthCard label={t('health.metric.gatewayOwner')} value={gatewayLabel(health)} />
              <HealthCard label={t('health.metric.gatewayCandidates')} value={String(health?.gateway?.candidates?.length ?? 0)} />
              <HealthCard
                label={t('health.metric.responseFormat')}
                value={`${health?.response_format?.default ?? 'toon'} / ${health?.response_format?.token_estimator ?? '-'}`}
              />
              <HealthCard label={t('health.metric.rss')} value={formatBytes(health?.rss_bytes ?? undefined)} />
              <HealthCard label={t('health.metric.bodyLimit')} value={health?.limits ? formatBytes(health.limits.body_max_bytes) : '?'} />
              <HealthCard
                label={t('health.metric.rateLimit')}
                value={health?.limits ? (health.limits.rate_limit_per_minute_per_ip === 0 ? 'off' : String(health.limits.rate_limit_per_minute_per_ip)) : '?'}
              />
              <HealthCard
                label={t('health.metric.xffTrustedDepth')}
                value={health?.limits ? String(health.limits.xff_trusted_depth) : '?'}
              />
              <HealthCard label={t('health.metric.readRetries')} value={health?.limits ? String(health.limits.read_retry_max) : '?'} />
              <HealthCard label={t('health.metric.circuitLimit')} value={health?.limits ? `${health.limits.circuit_failure_threshold} / ${health.limits.circuit_open_secs}s` : '?'} />
              <HealthCard
                tone={health?.circuits && health.circuits.circuits_open > 0 ? 'warn' : undefined}
                label={t('health.metric.circuitsOpenTracked')}
                value={health?.circuits ? `${health.circuits.circuits_open} / ${health.circuits.tracked_backends}` : '?'}
              />
            </div>
            <button className="refresh-btn" type="button" onClick={fetchHealth}>{t('common.action.refresh')}</button>
          </section>
        )}

        {activePanel === 'instances' && (
          <section className="panel active instances-panel">
            <h2>{t('instances.title')}</h2>
            <p className="empty log-hint">
              {t('instances.description')}
            </p>
            <StatusLine text={updatedAt.instances} error={errors.instances} />
            {instanceRows.length === 0 ? (
              <p className="empty">{t('instances.empty.none')}</p>
            ) : filteredInstanceRows.length === 0 ? (
              <p className="empty">{t('instances.empty.search')}</p>
            ) : (
              <div className="instance-groups">
                {Array.from(groupRows(filteredInstanceRows, instanceGroupLabel).entries())
                  .sort(([a], [b]) => a.localeCompare(b))
                  .map(([group, groupInstances]) => {
                    const flagged = groupInstances.filter((instance) => instance.stale || !statusClass(instance.status).includes('ok')).length;
                    return (
                      <div key={group} className="instance-group">
                        <div className="instance-group-head">
                          <h3>{group}</h3>
                          <span>{t('instances.group.meta', { count: groupInstances.length, flagged })}</span>
                        </div>
                        <div className="instances-grid">
                          {groupInstances.map((instance) => (
                            <div key={instance.instance_id} className={`instance-card ${instance.stale ? 'stale' : statusClass(instance.status).replace('badge badge-', '')}`}>
                              <div className="instance-name">
                                <img src={resolveDccIcon(instance.dcc_type)} alt="" className="dcc-icon" aria-hidden />
                                {instance.display_name} <span>{compactInstanceId(instance.instance_id)}</span>
                              </div>
                              <div className="instance-kv">
                                <span>App type</span><span>{instance.dcc_type}</span>
                                <span>Status</span><span><StatusBadge value={instance.status} /></span>
                                {instance.failure_reason ? (
                                  <>
                                    <span>Failure</span><span>{instance.failure_reason}</span>
                                  </>
                                ) : null}
                                <span>PID</span><span>{instance.pid ?? '-'}</span>
                                <span>Uptime</span><span>{formatUptime(instance.uptime_secs)}</span>
                                <span>Version</span><span>{instance.version ?? '-'}</span>
                                <span>Adapter</span><span>{instance.adapter_version ?? '-'}</span>
                                <span>Scene</span><span>{instance.scene ?? '-'}</span>
                                <span>CPU%</span><span>{instance.cpu_percent == null ? '-' : instance.cpu_percent.toFixed(1)}</span>
                                <span>Memory</span><span>{formatBytes(instance.memory_bytes)}</span>
                                <span>Access URL</span><span><BackendAccessUrl mcpUrl={instance.mcp_url} /></span>
                                <span>Endpoints</span><span><McpBackendLinks mcpUrl={instance.mcp_url} /></span>
                                <span>OpenAPI</span><span><BackendOpenApiLinks instance={instance} /></span>
                              </div>
                            </div>
                          ))}
                        </div>
                      </div>
                    );
                  })}
              </div>
            )}
            <div className="status-bar">Summary: live {instanceSummary.live}, stale {instanceSummary.stale}, unhealthy {instanceSummary.unhealthy}</div>
            <button className="refresh-btn" type="button" onClick={fetchInstanceBackends}>{t('common.action.refresh')}</button>
          </section>
        )}

        {activePanel === 'tools' && (
          <section className="panel active tools-panel">
            <h2>{t('tools.title')}</h2>
            <StatusLine text={updatedAt.tools} error={errors.tools} />
            {tools.length === 0 ? <p className="empty">{t('tools.empty.none')}</p> : filteredTools.length === 0 ? (
              <p className="empty">{t('tools.empty.search')}</p>
            ) : (
              Array.from(groupRows(filteredTools, toolGroupLabel).entries())
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([group, groupTools]) => (
                <div key={group} className="group-block">
                  <h3 className="group-title">{group}</h3>
                  <p className="group-meta">{t('tools.group.toolCount', { count: groupTools.length })}</p>
                  <table>
                    <thead><tr><th>{t('tools.table.slug')}</th><th>{t('common.table.appType')}</th><th>{t('common.table.instance')}</th><th>{t('common.table.summary')}</th></tr></thead>
                    <tbody>
                      {groupTools.map((tool) => (
                        <tr key={tool.slug}>
                          <td>{tool.slug}</td>
                          <td>{tool.dcc_type}</td>
                          <td>{toolInstanceLabel(tool)}</td>
                          <td>{tool.summary.slice(0, 120)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )))}
            <button className="refresh-btn" type="button" onClick={fetchTools}>{t('common.action.refresh')}</button>
          </section>
        )}

        {activePanel === 'openapi' && (
          <section className="panel active openapi-panel" data-panel="openapi">
            <PanelHeader
              title={t('openapi.title')}
              meta={t('openapi.meta')}
              action={(
                <>
                  <a className="refresh-btn" href={openApiSource.docsUrl} target="_blank" rel="noopener noreferrer">{t('openapi.action.openReference')}</a>
                  <a className="refresh-btn" href={openApiSource.specUrl} target="_blank" rel="noopener noreferrer">{t('openapi.action.specJson')}</a>
                  <button className="refresh-btn" type="button" disabled={!openApiRaw} onClick={() => void copyText(openApiRaw, 'OpenAPI spec JSON')}>{t('openapi.action.copyJson')}</button>
                  <button className="refresh-btn" type="button" disabled={!openApiRaw} onClick={() => {
                    downloadJsonText(openApiSpecFilename(openApiSource.label), openApiRaw);
                    setCopiedNotice(t('openapi.notice.downloadedSpec'));
                    window.setTimeout(() => setCopiedNotice(''), 1800);
                  }}>{t('openapi.action.downloadJson')}</button>
                  {openApiSource.kind === 'instance' ? (
                    <button className="refresh-btn" type="button" onClick={() => goToPanel('openapi', { replace: true })}>{t('openapi.action.gatewaySpec')}</button>
                  ) : null}
                  <button className="refresh-btn" type="button" onClick={fetchOpenApi}>{t('common.action.refresh')}</button>
                </>
              )}
            />
            <StatusLine text={copiedNotice || updatedAt.openapi} error={errors.openapi} />
            <OpenApiInspectorPanel
              spec={openApiSpec}
              raw={openApiRaw}
              operations={filteredOpenApiOperations}
              source={openApiSource}
              labels={{
                emptyDocument: t('openapi.empty.document'),
                openapi: t('openapi.metric.openapi'),
                version: t('openapi.metric.version'),
                paths: t('openapi.metric.paths'),
                operations: t('openapi.metric.operations'),
                schemas: t('openapi.metric.schemas'),
                tags: t('openapi.metric.tags'),
                operationsSection: t('openapi.section.operations'),
                emptyOperations: t('openapi.empty.operations'),
                linksSection: t('openapi.section.links'),
                body: t('openapi.label.body'),
                noBody: t('openapi.label.noBody'),
                params: (count) => t('openapi.label.params', { count }),
                responses: (codes) => t('openapi.label.responses', { codes }),
                noResponses: t('openapi.label.noResponses'),
              }}
              t={t}
            />
          </section>
        )}

        {activePanel === 'workflows' && (
          <section className="panel active workflows-panel">
            <PanelHeader
              title={t('workflows.title')}
              meta={t('workflows.meta')}
              action={<button className="refresh-btn" type="button" onClick={fetchWorkflows}>{t('common.action.refresh')}</button>}
            />
            <StatusLine text={copiedNotice || updatedAt.workflows} error={errors.workflows} />
            <div className="metric-grid compact">
              <MetricTile tone="ok" label={t('workflows.metric.completed')} value={workflowSummary.completed} />
              <MetricTile tone={workflowSummary.warning > 0 ? 'warn' : undefined} label={t('workflows.metric.warnings')} value={workflowSummary.warning} />
              <MetricTile tone={workflowSummary.failed > 0 ? 'err' : undefined} label={t('workflows.metric.failed')} value={workflowSummary.failed} />
              <MetricTile tone={workflowSummary.zeroResults > 0 ? 'warn' : undefined} label={t('workflows.metric.zeroResult')} value={workflowSummary.zeroResults} />
              <MetricTile label={t('common.metric.visible')} value={`${filteredWorkflows.length} / ${workflows.length}`} />
            </div>
            {visibleSelectedWorkflow ? (
              <WorkflowGraphDetail
                workflow={visibleSelectedWorkflow}
                onClose={() => setSelectedWorkflowId(null)}
                onOpenTrace={(requestId) => goToPanel('traces', { traceId: requestId })}
                onCopyIssueReport={(requestId) => void copyIssueReport(requestId)}
                t={t}
              />
            ) : null}
            {workflows.length === 0 ? <p className="empty">{t('workflows.empty.none')}</p> : filteredWorkflows.length === 0 ? (
              <p className="empty">{t('workflows.empty.search')}</p>
            ) : (
              <div className="workflow-board">
                {filteredWorkflows.map((workflow) => (
                  <WorkflowCard
                    key={`${workflow.group_kind}-${workflow.workflow_id}`}
                    workflow={workflow}
                    onInspect={(workflowId) => setSelectedWorkflowId(workflowId)}
                    onOpenTrace={(requestId) => goToPanel('traces', { traceId: requestId })}
                    onCopyIssueReport={(requestId) => void copyIssueReport(requestId)}
                    t={t}
                  />
                ))}
              </div>
            )}
          </section>
        )}

        {activePanel === 'tasks' && (
          <section className="panel active tasks-panel">
            <PanelHeader
              title={t('tasks.title')}
              meta={t('tasks.meta')}
              action={<button className="refresh-btn" type="button" onClick={fetchTasks}>{t('common.action.refresh')}</button>}
            />
            <StatusLine text={updatedAt.tasks} error={errors.tasks} />
            <div className="metric-grid compact">
              <MetricTile tone="ok" label={t('tasks.metric.completed')} value={taskSummary.completed} />
              <MetricTile tone={taskSummary.failed > 0 ? 'err' : undefined} label={t('tasks.metric.failed')} value={taskSummary.failed} />
              <MetricTile tone={taskSummary.active > 0 ? 'warn' : undefined} label={t('tasks.metric.activeWaiting')} value={taskSummary.active} />
              <MetricTile label={t('common.metric.visible')} value={`${filteredTasks.length} / ${tasks.length}`} />
            </div>
            {tasks.length === 0 ? <p className="empty">{t('tasks.empty.none')}</p> : filteredTasks.length === 0 ? (
              <p className="empty">{t('tasks.empty.search')}</p>
            ) : (
              <div className="task-board">
                {filteredTasks.map((task) => {
                  const requestId = taskPrimaryRequestId(task);
                  const tone = isErrStatus(task.status) ? 'err' : isWarnStatus(task.status) ? 'warn' : isOkStatus(task.status) ? 'ok' : 'muted';
                  const outcome = taskOutcomeText(task);
                  const requestCount = taskRequestCount(task);
                  return (
                    <article key={task.task_id} className={`task-card ${tone}`}>
                      <div className="task-main">
                        <div className="task-title-row">
                          <StatusBadge value={task.status} />
                          <span className="task-type">{task.task_type}</span>
                          <TimeValue className="task-time" value={task.started_at} />
                          <span>{formatDurationMs(task.duration_ms)}</span>
                        </div>
                        <h3 title={task.title}>{task.title}</h3>
                        {task.goal && task.goal !== task.title ? (
                          <p className="task-outcome"><strong>{t('tasks.label.goal')}</strong>{task.goal}</p>
                        ) : null}
                        {outcome ? (
                          <p className={`task-outcome ${tone === 'err' ? 'err' : ''}`}>
                            <strong>{tone === 'err' ? t('tasks.label.failure') : t('tasks.label.result')}</strong>
                            {outcome}
                          </p>
                        ) : null}
                        <div className="task-meta">
                          <span>{compactList(task.app_types, task.correlation?.dcc_type ?? 'gateway')}</span>
                          <span>{t('tasks.label.workflow', { id: taskWorkflowLabel(task) })}</span>
                          <span>{t('tasks.label.calls', { count: requestCount })}</span>
                          <span>{t('tasks.label.client', { value: taskActorLabel(task) })}</span>
                        </div>
                        {task.artifacts?.length ? (
                          <div className="task-chip-row" aria-label={t('tasks.label.artifacts')}>
                            {task.artifacts.map((artifact) => (
                              <span key={`${artifact.kind}-${artifact.name}-${artifact.request_id ?? ''}`}>
                                {artifact.kind}: {artifact.name}
                              </span>
                            ))}
                          </div>
                        ) : null}
                        {task.validation_checks?.length ? (
                          <div className="task-chip-row" aria-label={t('tasks.label.validation')}>
                            {task.validation_checks.map((check) => (
                              <span key={`${check.title}-${check.request_id ?? ''}`}>
                                {check.title} <StatusBadge value={check.status} />
                              </span>
                            ))}
                          </div>
                        ) : null}
                      </div>
                      <div className="task-side">
                        {requestId ? (
                          <button className="link-chip" type="button" title={requestId} onClick={() => goToPanel('traces', { traceId: requestId })}>
                            {t('tasks.link.trace', { id: requestId.slice(0, 12) })}
                          </button>
                        ) : (
                          <span className="mono-path">{task.task_id.slice(0, 12)}</span>
                        )}
                        {task.related?.workflow_ids?.length ? (
                          <button className="link-chip" type="button" onClick={() => goToPanel('workflows')}>
                            {t('tasks.link.workflows', { count: task.related.workflow_ids.length })}
                          </button>
                        ) : null}
                        {requestCount ? (
                          <button className="link-chip" type="button" onClick={() => goToPanel('calls')}>
                            {t('tasks.link.calls', { count: requestCount })}
                          </button>
                        ) : null}
                      </div>
                    </article>
                  );
                })}
              </div>
            )}
          </section>
        )}

        {activePanel === 'calls' && (
          <section className="panel active calls-panel">
            <h2>{t('calls.title')}</h2>
            <StatusLine text={updatedAt.calls} error={errors.calls} />
            {calls.length === 0 ? <p className="empty">{t('calls.empty.none')}</p> : filteredCalls.length === 0 ? (
              <p className="empty">{t('calls.empty.search')}</p>
            ) : (
              Array.from(groupRows(filteredCalls, callGroupLabel).entries())
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([group, groupCalls]) => (
                <div key={group} className="group-block">
                  <h3 className="group-title">{group}</h3>
                  <table>
                    <thead><tr><th>{t('common.table.time')}</th><th>{t('common.table.request')}</th><th>{t('common.table.tool')}</th><th>{t('common.table.appType')}</th><th>{t('common.table.instance')}</th><th>{t('common.table.actor')}</th><th>{t('calls.table.agent')}</th><th>{t('common.table.platform')}</th><th>{t('common.table.sourceIp')}</th><th>{t('calls.table.transport')}</th><th>{t('calls.table.format')}</th><th>{t('calls.table.returned')}</th><th>{t('calls.table.saved')}</th><th>{t('common.table.status')}</th><th>{t('calls.table.error')}</th><th>{t('common.table.ms')}</th><th>{t('calls.table.detail')}</th></tr></thead>
                    <tbody>
                      {groupCalls.map((call) => {
                        const trace = traceByRequest.get(call.request_id);
                        const slowestSpan = trace?.slowest_span_name
                          ? t('traces.detail.slowestSpan', { name: trace.slowest_span_name, duration: formatDurationMs(trace.slowest_span_ms) })
                          : '';
                        return (
                          <tr key={call.request_id} className={`latency-row ${latencyClass(call.duration_ms)}`}>
                            <td><TimeValue value={call.timestamp} /></td>
                            <td>
                              <button className="refresh-btn" type="button" title={call.request_id} onClick={() => goToPanel('traces', { traceId: call.request_id })}>
                                {call.request_id.slice(0, 12)}
                              </button>
                            </td>
                            <td>{call.tool}</td>
                            <td>{call.dcc_type}</td>
                            <td>{compactInstanceId(call.instance_id)}</td>
                            <td title={call.actor_id ?? call.auth_subject ?? ''}>
                              <span className="trust-cell">{actorLabel(call)}{trustChip(firstTrust(call, ['actor_name', 'actor_id', 'actor_email_hash', 'auth_subject']))}</span>
                            </td>
                            <td title={call.agent_id ?? call.agent_name ?? ''}>{agentLabel(call)}</td>
                            <td title={[call.client_platform, call.client_os, call.client_host].filter(Boolean).join(' / ')}>
                              <span className="trust-cell">{platformLabel(call)}{trustChip(firstTrust(call, ['client_platform', 'client_os', 'client_host']))}</span>
                            </td>
                            <td><span className="trust-cell">{sourceIpLabel(call)}{trustChip(trustFor(call, 'source_ip'))}</span></td>
                            <td>{call.transport ?? '-'}</td>
                            <td>{responseFormatLabel(call)}</td>
                            <td>{returnedTokensLabel(call)}</td>
                            <td>{savedTokensLabel(call)}</td>
                            <td><StatusBadge value={call.status} /></td>
                            <td title={call.error ?? ''}>{call.error ? call.error.slice(0, 80) : '-'}</td>
                            <td className="latency-cell">
                              <LatencyValue value={call.duration_ms} t={t} />
                              {slowestSpan ? <div className="latency-subtext">{slowestSpan}</div> : null}
                            </td>
                            <td>
                              <div className="table-actions">
                                <button className="refresh-btn" type="button" onClick={() => void fetchTraceInto(call.request_id, 'call')}>{t('calls.action.expand')}</button>
                                <button className="refresh-btn" type="button" onClick={() => void copyText(traceLinks(call.request_id, call.links).admin_trace_url ?? '', 'trace URL')}>{t('traces.action.copyUrl')}</button>
                                <button className="refresh-btn" type="button" onClick={() => void copyIssueReport(call.request_id)}>{t('traces.action.copyIssueJson')}</button>
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              )))}
            <pre className="empty">{callDetail}</pre>
            <button className="refresh-btn" type="button" onClick={fetchCalls}>{t('common.action.refresh')}</button>
          </section>
        )}

        {activePanel === 'traces' && (
          <section className="panel active traces-panel" data-panel="traces">
            <PanelHeader
              title={t('traces.title')}
              meta={t('traces.meta')}
              action={<button className="refresh-btn" type="button" onClick={fetchTraces}>{t('common.action.refresh')}</button>}
            />
            <StatusLine text={copiedNotice || updatedAt.traces} error={errors.traces} />
            <div className="metric-grid compact">
              <MetricTile tone="ok" label="OK" value={traceSummary.ok} />
              <MetricTile tone={traceSummary.failed > 0 ? 'err' : undefined} label={t('workflows.metric.failed')} value={traceSummary.failed} />
              <MetricTile tone={latencyTone(traceSummary.p95)} label={t('debug.metric.latency')} value={formatDurationMs(traceSummary.p95)} />
              <MetricTile tone={latencyTone(traceSummary.p99)} label={t('stats.metric.p99Latency')} value={formatDurationMs(traceSummary.p99)} detail={latencyThresholdDetail} />
              <MetricTile tone={traceSummary.slow > 0 ? 'warn' : undefined} label={t('stats.metric.slowCalls')} value={traceSummary.slow} detail={slowLatencyDetail} />
              <MetricTile label={t('traces.metric.totalTokens')} value={formatTokenCount(traceSummary.totalTokens)} detail={t('traces.detail.inputOutput', { input: formatTokenCount(traceSummary.totalInputTokens), output: formatTokenCount(traceSummary.totalOutputTokens) })} />
              <MetricTile label={t('traces.metric.agentContext')} value={traceSummary.agentContext} />
              <MetricTile label={t('traces.metric.spans')} value={traceSummary.spans} />
              <MetricTile label={t('common.metric.visible')} value={`${filteredTraces.length} / ${traces.length}`} />
            </div>
            {traces.length === 0 ? <p className="empty">{t('traces.empty.none')}</p> : filteredTraces.length === 0 ? (
              <p className="empty">{t('traces.empty.search')}</p>
            ) : (
              <div className="trace-layout">
                <div className="trace-list">
                  {Array.from(groupRows(filteredTraces, traceGroupLabel).entries())
                    .sort(([a], [b]) => a.localeCompare(b))
                    .map(([group, groupTraces]) => (
                    <div key={group} className="trace-group">
                      <div className="trace-group-head">
                        <h3>{group}</h3>
                        <span>{groupTraces.length}</span>
                      </div>
                      {groupTraces.map((trace) => (
                        <button
                          key={trace.request_id}
                          className={`trace-item ${isErrStatus(trace.status) ? 'err' : isWarnStatus(trace.status) ? 'warn' : isOkStatus(trace.status) ? 'ok' : ''} ${latencyClass(trace.total_ms)}`}
                          type="button"
                          onClick={() => goToPanel('traces', { traceId: trace.request_id, replace: true })}
                        >
                          <span className="trace-item-main">
                            <strong>{trace.tool}</strong>
                            <span>{compactId(trace.request_id)} - {compactInstanceId(trace.instance_id)} - <TimeValue value={trace.timestamp} /> - {trace.transport ?? '?'}</span>
                            <span>
                              {actorLabel(trace)} {trustChip(firstTrust(trace, ['actor_name', 'actor_id', 'actor_email_hash', 'auth_subject']))}
                              {' - '}
                              {platformLabel(trace)} {trustChip(firstTrust(trace, ['client_platform', 'client_os', 'client_host']))}
                              {' - '}
                              {sourceIpLabel(trace)} {trustChip(trustFor(trace, 'source_ip'))}
                            </span>
                            <span>{agentLabel(trace)}{trace.slowest_span_name ? ` - ${t('traces.detail.slowestSpan', { name: trace.slowest_span_name, duration: formatDurationMs(trace.slowest_span_ms) })}` : ''}</span>
                          </span>
                          <span className="trace-item-side">
                            <StatusBadge value={trace.status} />
                            <LatencyValue value={trace.total_ms} t={t} />
                            <span>{t('traces.detail.spanCount', { count: trace.span_count ?? 0 })}</span>
                            <span>{t('traces.detail.tokenCount', { count: formatTokenCount(totalTraceTokens(trace)) })}</span>
                          </span>
                        </button>
                      ))}
                    </div>
                  ))}
                </div>
                <TraceDetailPanel
                  trace={traceDetailPayload}
                  fallback={traceDetail}
                  t={t}
                  onCopy={copyText}
                  onCopyIssueReport={(requestId) => void copyIssueReport(requestId)}
                  onDownloadIssueReport={(requestId) => void downloadIssueReport(requestId)}
                />
              </div>
            )}
          </section>
        )}

        {activePanel === 'traffic' && (
          <section className="panel active traffic-panel" data-panel="traffic">
            <PanelHeader
              title={t('traffic.title')}
              meta={t('traffic.meta')}
              action={(
                <div className="table-actions">
                  <a
                    className="refresh-btn"
                    href={traffic?.links?.traffic_export_jsonl_url ?? `${API_BASE}/traffic/export?limit=1000`}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {t('common.action.exportJsonl')}
                  </a>
                  <button className="refresh-btn" type="button" onClick={fetchTraffic}>{t('common.action.refresh')}</button>
                </div>
              )}
            />
            <StatusLine text={copiedNotice || updatedAt.traffic} error={errors.traffic} />
            <div className="metric-grid compact">
              <MetricTile
                tone={trafficStatusTone(trafficCaptureStatus)}
                label={t('traffic.metric.captureState')}
                value={t(trafficStatusLabelKey(trafficCaptureStatus))}
                detail={trafficStatusDetail}
              />
              <MetricTile label={t('traffic.metric.retained')} value={trafficFrames.length} detail={t('stats.detail.visible', { visible: filteredTrafficFrames.length })} />
              <MetricTile label={t('traffic.metric.sessions')} value={trafficSummary.sessions} />
              <MetricTile label={t('traffic.metric.transports')} value={trafficSummary.transports} />
              <MetricTile tone={trafficSummary.redacted > 0 ? 'warn' : undefined} label={t('traffic.metric.redactions')} value={trafficSummary.redacted} />
              <MetricTile label={t('traffic.metric.payload')} value={formatBytes(trafficSummary.bytes)} />
            </div>
            {trafficFrames.length === 0 ? <p className="empty">{t(trafficEmptyKey(trafficCaptureStatus))}</p> : filteredTrafficFrames.length === 0 ? (
              <p className="empty">{t('traffic.empty.search')}</p>
            ) : (
              <div className="trace-layout">
                <div className="trace-list">
                  <table>
                    <thead>
                      <tr>
                        <th>{t('common.table.time')}</th>
                        <th>{t('common.table.request')}</th>
                        <th>{t('traffic.table.method')}</th>
                        <th>{t('traffic.table.leg')}</th>
                        <th>{t('traffic.table.http')}</th>
                        <th>{t('traffic.table.session')}</th>
                        <th>{t('traffic.table.bytes')}</th>
                        <th>{t('traffic.table.redaction')}</th>
                        <th>{t('common.table.actions')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {filteredTrafficFrames.map((frame, index) => {
                        const requestId = trafficRequestId(frame);
                        return (
                          <tr key={frame.id ?? `${requestId ?? 'traffic'}-${index}`}>
                            <td><TimeValue value={trafficTimestamp(frame)} /></td>
                            <td>
                              <span className="mono-path">{compactId(requestId)}</span>
                              <div className="muted">{compactId(frame.correlation?.trace_id)}</div>
                            </td>
                            <td>
                              <span className="mono-path">{trafficMethod(frame)}</span>
                              <div className="muted">{frame.attributes?.mcp?.kind ?? '-'}</div>
                            </td>
                            <td>
                              {frame.attributes?.leg ?? '-'}
                              <div className="muted">{frame.attributes?.transport ?? '-'}</div>
                            </td>
                            <td>
                              {frame.attributes?.http?.method ?? '-'} {frame.attributes?.http?.url ?? ''}
                              <div className="muted">{frame.attributes?.http?.status ?? '-'}</div>
                            </td>
                            <td className="mono-path">{compactId(trafficSessionId(frame))}</td>
                            <td>{formatBytes(trafficBodyBytes(frame))}</td>
                            <td className="mono-path">{compactList(trafficRedactedPaths(frame), t('governance.privacy.none'))}</td>
                            <td>
                              <div className="table-actions">
                                <button className="refresh-btn" type="button" onClick={() => setTrafficDetail(trafficFrameDetail(frame))}>{t('common.action.view')}</button>
                                {requestId ? (
                                  <button className="refresh-btn" type="button" onClick={() => goToPanel('traces', { traceId: requestId })}>{t('common.action.trace')}</button>
                                ) : null}
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
                <div className="trace-detail-card">
                  <div className="trace-card-head">
                    <h3>{t('traffic.detail.frameJson')}</h3>
                    <button className="refresh-btn" type="button" onClick={() => void copyText(trafficDetail, 'traffic frame JSON')}>{t('common.action.copy')}</button>
                  </div>
                  <pre className="payload-pre">{trafficDetail}</pre>
                </div>
              </div>
            )}
          </section>
        )}

        {activePanel === 'stats' && (
          <section className="panel active stats-panel" data-panel="stats">
            <PanelHeader
              title={t('stats.title')}
              meta={t('stats.meta')}
              action={(
                <div className="stats-actions">
                  <label className="range-label" htmlFor="stats-range-select">
                    {t('stats.label.range')}
                    <select
                      id="stats-range-select"
                      aria-label={t('stats.label.range')}
                      value={statsRange}
                      onChange={(event) => {
                        const v = event.target.value;
                        setStatsRange(v);
                        pushAdminUrl('stats', { range: v, replace: true });
                      }}
                    >
                      <option value="1h">1h</option>
                      <option value="24h">24h</option>
                      <option value="7d">7d</option>
                      <option value="all">All</option>
                    </select>
                  </label>
                  <button className="refresh-btn" type="button" onClick={fetchStats}>{t('common.action.refresh')}</button>
                </div>
              )}
            />
            <StatusLine text={updatedAt.stats} error={errors.stats} />
            {stats?.error ? <p className="empty">{stats.error}</p> : null}
            <div className="metric-grid">
              <MetricTile label={t('stats.metric.calls')} value={stats?.total_calls ?? 0} detail={t('stats.detail.window', { range: statsRange })} />
              <MetricTile tone={errorRateTone(stats)} label={t('stats.metric.success')} value={stats ? `${stats.success_rate.toFixed(1)}%` : '0.0%'} detail={t('stats.detail.okFailed', { ok: statsSummary.success, failed: statsSummary.failed })} />
              <MetricTile
                label={t('stats.metric.payloadTokens')}
                value={formatTokenCount(stats?.payload_token_usage?.total_tokens ?? stats?.total_tokens ?? statsSummary.totalTokens)}
                detail={t('stats.detail.payloadCoverage', {
                  avg: formatTokenCount(stats?.payload_token_usage?.avg_total_tokens_per_call ?? stats?.avg_tokens_per_call ?? stats?.avg_total_tokens_per_call ?? statsSummary.avgTokens),
                  recorded: stats?.payload_token_usage?.calls_with_any_payload_tokens ?? 0,
                  missing: stats?.payload_token_usage?.calls_missing_payload_tokens ?? 0,
                })}
              />
              <MetricTile
                label={t('stats.metric.inputOutputTokens')}
                value={formatTokenCount(stats?.payload_token_usage?.total_input_tokens ?? stats?.total_input_tokens ?? statsSummary.totalInputTokens)}
                detail={t('stats.detail.output', { value: formatTokenCount(stats?.payload_token_usage?.total_output_tokens ?? stats?.total_output_tokens ?? statsSummary.totalOutputTokens) })}
              />
              <MetricTile tone={latencyTone(stats?.latency_ms?.p50_ms ?? stats?.p50_ms)} label={t('stats.metric.p50Latency')} value={formatDurationMs(stats?.latency_ms?.p50_ms ?? stats?.p50_ms)} />
              <MetricTile tone={latencyTone(stats?.latency_ms?.p95_ms ?? stats?.p95_ms)} label={t('stats.metric.p95Latency')} value={formatDurationMs(stats?.latency_ms?.p95_ms ?? stats?.p95_ms)} />
              <MetricTile tone={latencyTone(stats?.latency_ms?.p99_ms)} label={t('stats.metric.p99Latency')} value={formatDurationMs(stats?.latency_ms?.p99_ms)} detail={latencyThresholdDetail} />
              <MetricTile tone={slowCallCount > 0 ? 'warn' : undefined} label={t('stats.metric.slowCalls')} value={slowCallCount} detail={slowLatencyDetail} />
              <MetricTile
                label={t('stats.metric.responseTokensReturned')}
                value={formatTokenCount(stats?.token_usage?.total_returned_tokens)}
                detail={t('stats.detail.original', { value: formatTokenCount(stats?.token_usage?.total_original_tokens) })}
              />
              <MetricTile
                tone={(stats?.token_usage?.total_saved_tokens ?? 0) > 0 ? 'ok' : undefined}
                label={t('stats.metric.responseTokensSaved')}
                value={formatTokenCount(stats?.token_usage?.total_saved_tokens)}
                detail={t('stats.detail.average', { value: formatSavingsPct(stats?.token_usage?.average_savings_pct) })}
              />
              <MetricTile
                label={t('stats.metric.responseFormat')}
                value={health?.response_format?.default ?? 'toon'}
                detail={stats?.payload_token_usage?.token_estimator ?? stats?.payload_token_estimator ?? health?.response_format?.token_estimator ?? t('stats.detail.tokenEstimatorUnavailable')}
              />
            </div>
            <div className="stats-charts">
              <StatBarList title={t('stats.chart.topAppTypes')} items={filteredTopAppTypes} t={t} />
              <StatBarList title={t('stats.chart.topTools')} items={filteredTopTools} t={t} />
              <StatBarList title={t('stats.chart.topInstances')} items={filteredTopInstances} t={t} />
              <StatBarList title={t('stats.chart.topAgents')} items={filteredTopAgents} t={t} />
              <AttributionFacetList title={t('stats.chart.topActors')} items={filteredTopActors} t={t} />
              <AttributionFacetList title={t('stats.chart.topClientPlatforms')} items={filteredTopClientPlatforms} t={t} />
              <AttributionFacetList title={t('stats.chart.topSourceIps')} items={filteredTopSourceIps} t={t} />
              {stats?.hourly_distribution?.length ? <HourlyChart buckets={stats.hourly_distribution} t={t} /> : null}
              <TokenBreakdownList title={t('stats.chart.savingsByTool')} items={filteredTokenByTool} t={t} />
              <TokenBreakdownList title={t('stats.chart.savingsByInstance')} items={filteredTokenByInstance} t={t} />
              <TokenBreakdownList title={t('stats.chart.savingsByAgent')} items={filteredTokenByAgent} t={t} />
              <TokenBreakdownList title={t('stats.chart.savingsByTransport')} items={filteredTokenByTransport} t={t} />
              <TokenBreakdownList title={t('stats.chart.savingsByFormat')} items={filteredTokenByFormat} t={t} />
            </div>
          </section>
        )}

        {activePanel === 'governance' && (
          <section className="panel active governance-panel" data-panel="governance">
            <PanelHeader
              title={t('governance.title')}
              meta={governance?.mode?.reason ?? t('governance.meta')}
              action={<button className="refresh-btn" type="button" onClick={fetchGovernance}>{t('common.action.refresh')}</button>}
            />
            <StatusLine text={updatedAt.governance} error={errors.governance} />
            <div className="metric-grid">
              <MetricTile
                tone={governanceSummary.captureEnabled ? 'warn' : 'ok'}
                label={t('governance.metric.capture')}
                value={governanceSummary.captureEnabled ? t('common.status.on') : t('common.status.off')}
                detail={governance?.traffic_capture?.mode ?? t('governance.detail.safeAggregateOnly')}
              />
              <MetricTile
                tone={governanceSummary.readOnly ? 'warn' : undefined}
                label={t('governance.metric.readOnly')}
                value={governanceSummary.readOnly ? t('common.status.on') : t('common.status.off')}
                detail={t('governance.detail.activeAllowlists', { count: governanceSummary.allowlists })}
              />
              <MetricTile label={t('governance.metric.denied')} value={governanceSummary.denied} detail={t('governance.detail.recentPolicyDecisions')} />
              <MetricTile tone={governanceSummary.throttled ? 'warn' : undefined} label={t('governance.metric.throttled')} value={governanceSummary.throttled} detail={t('governance.detail.recentPressureDecisions')} />
            </div>
            <div className="governance-layout">
              <section className="governance-section">
                <h3 className="section-kicker">{t('governance.section.effectivePolicy')}</h3>
                <div className="governance-card">
                  <div className="governance-kv">
                    <span><strong>DCC</strong>{compactList(governance?.policy?.allowed_dcc_types)}</span>
                    <span><strong>{t('governance.label.skills')}</strong>{compactList([...(governance?.policy?.allowed_skill_names ?? []), ...(governance?.policy?.allowed_skill_families ?? [])])}</span>
                    <span><strong>{t('governance.label.tools')}</strong>{compactList([...(governance?.policy?.allowed_tool_slugs ?? []), ...(governance?.policy?.allowed_tool_slug_prefixes ?? [])])}</span>
                    <span><strong>{t('governance.label.mode')}</strong>{governance?.policy?.unrestricted ? t('governance.state.unrestricted') : t('governance.state.constrained')}</span>
                  </div>
                </div>
              </section>
              <section className="governance-section">
                <h3 className="section-kicker">{t('governance.section.trafficCapture')}</h3>
                <div className="governance-card">
                  <div className="governance-kv">
                    <span><strong>{t('governance.label.sinks')}</strong>{governance?.traffic_capture?.sink_count ?? 0}</span>
                    <span><strong>{t('governance.label.guardrail')}</strong>{governance?.traffic_capture?.production_guardrail ?? t('governance.state.inactive')}</span>
                    <span><strong>{t('governance.label.captured')}</strong>{governanceSummary.captured}</span>
                    <span><strong>{t('governance.label.skipped')}</strong>{governanceSummary.skipped}</span>
                  </div>
                  <p className="mono-path">{compactList(governance?.traffic_capture?.redaction?.paths, t('governance.empty.captureRedactionRules'))}</p>
                </div>
              </section>
              <section className="governance-section wide">
                <h3 className="section-kicker">{t('governance.section.middlewareControls')}</h3>
                <div className="governance-card-grid">
                  {(governance?.middleware?.controls ?? []).length === 0 ? (
                    <p className="empty">{t('governance.empty.controls')}</p>
                  ) : (
                    (governance?.middleware?.controls ?? []).map((control, index) => (
                      <GovernanceControlCard key={`${control.kind}-${control.mode}-${index}`} control={control} t={t} />
                    ))
                  )}
                </div>
              </section>
              <section className="governance-section wide">
                <h3 className="section-kicker">{t('governance.section.recentRequestDecisions')}</h3>
                <table>
                  <thead>
                    <tr>
                      <th>{t('common.table.request')}</th>
                      <th>{t('governance.table.outcome')}</th>
                      <th>{t('governance.table.agentSession')}</th>
                      <th>{t('common.table.tool')}</th>
                      <th>{t('governance.table.capture')}</th>
                      <th>{t('governance.table.redaction')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(governance?.recent_decisions ?? []).length === 0 ? (
                      <EmptyRow columns={6}>{t('governance.empty.decisions')}</EmptyRow>
                    ) : filteredGovernanceDecisions.length === 0 ? (
                      <EmptyRow columns={6}>{t('governance.empty.decisionsSearch')}</EmptyRow>
                    ) : (
                      filteredGovernanceDecisions.map((row, index) => (
                        <tr key={`${row.request_id ?? row.trace_id ?? 'decision'}-${index}`}>
                          <td>
                            <span className="mono-path">{compactId(row.request_id)}</span>
                            <div className="muted">{formatTraceDate(row.timestamp)}</div>
                          </td>
                          <td>
                            <span className={`badge ${row.outcome === 'allowed' ? 'badge-ok' : row.outcome === 'throttled' || row.outcome === 'denied' ? 'badge-err' : 'badge-muted'}`}>
                              {row.outcome ?? 'unknown'}
                            </span>
                            {row.reason ? <div className="muted">{row.policy?.reason ?? row.reason}</div> : null}
                          </td>
                          <td>
                            {agentLabel(row)}
                            <div className="muted">{compactId(row.session_id)}</div>
                          </td>
                          <td>
                            <span className="mono-path">{row.tool ?? '-'}</span>
                            <div className="muted">{row.dcc_type ?? '-'}</div>
                          </td>
                          <td>
                            {(row.traffic_capture?.captured ?? 0) > 0 ? t('governance.capture.captured') : t('governance.capture.skipped')}
                            <div className="muted">{compactList(row.traffic_capture?.reasons, t('governance.capture.noReason'))}</div>
                          </td>
                          <td className="mono-path">{compactList(row.privacy?.redacted_paths, t('governance.privacy.none'))}</td>
                        </tr>
                      ))
                    )}
                  </tbody>
                </table>
              </section>
            </div>
          </section>
        )}

        {activePanel === 'skill-paths' && (
          <section className="panel active skill-paths-panel">
            <PanelHeader
              title={t('skillPaths.title')}
              action={
                <button className="refresh-btn" type="button" disabled={skillPathBusy} onClick={() => void fetchSkillInventory()}>
                  {t('common.action.refresh')}
                </button>
              }
            />
            <StatusLine text={updatedAt['skill-paths']} error={errors['skill-paths']} />
            <p className="empty log-hint">
              {t('skillPaths.description')}
            </p>
            <div className="metric-grid compact skill-summary-grid">
              <MetricTile label={t('skillPaths.metric.loadedSkills')} value={skillTotals.loaded} detail={t('skillPaths.detail.indexed', { count: skillTotals.total })} />
              <MetricTile label={t('skillPaths.metric.actions')} value={skillTotals.action_count} detail={t('skillPaths.detail.fromLoadedSkills')} />
              <MetricTile label={t('skillPaths.metric.searchPaths')} value={skillPaths.length} detail={t('skillPaths.detail.activeDiscoveryRoots')} />
              <MetricTile label={t('skillPaths.metric.searchedUsed')} value={`${skillTotals.searched} / ${skillTotals.used}`} detail={t('skillPaths.detail.searchedUsed')} />
              <MetricTile tone={skillTotals.low_adoption > 0 ? 'warn' : 'ok'} label={t('skillPaths.metric.lowAdoption')} value={skillTotals.low_adoption} detail={t('skillPaths.detail.lowAdoption')} />
              <MetricTile tone={skillTotals.load_errors > 0 || skillTotals.missing_paths > 0 ? 'warn' : 'ok'} label={t('skillPaths.metric.healthSignals')} value={skillTotals.load_errors + skillTotals.missing_paths} detail={t('skillPaths.detail.healthSignals', { loads: skillTotals.load_errors, paths: skillTotals.missing_paths })} />
            </div>
            <div className="skill-inventory-section">
              <h3 className="section-kicker">{t('skillPaths.section.loadedSkills')}</h3>
              <table>
                <thead>
                  <tr>
                    <th>{t('skillPaths.table.skill')}</th>
                    <th>DCC</th>
                    <th>{t('skillPaths.table.state')}</th>
                    <th>{t('skillPaths.metric.actions')}</th>
                    <th>{t('skillPaths.table.discovery')}</th>
                    <th>{t('skillPaths.table.usage')}</th>
                    <th>{t('skillPaths.table.instances')}</th>
                    <th>{t('common.table.tool')}</th>
                  </tr>
                </thead>
                <tbody>
                  {skills.length === 0 ? (
                    <EmptyRow columns={8}>{t('skillPaths.empty.skills')}</EmptyRow>
                  ) : filteredSkills.length === 0 ? (
                    <EmptyRow columns={8}>{t('skillPaths.empty.skillsSearch')}</EmptyRow>
                  ) : (
                    filteredSkills.map((skill) => (
                      <tr
                        className={selectedSkill?.name === skill.name && selectedSkill?.dcc_type === skill.dcc_type ? 'skill-row selected' : 'skill-row'}
                        key={`${skill.dcc_type}-${skill.name}-${skill.loaded ? 'loaded' : 'unloaded'}`}
                      >
                        <td>
                          <button className="linkish skill-name-button" type="button" onClick={() => void fetchSkillDetail(skill)}>
                            {skill.name}
                          </button>
                          {skill.summary ? <div className="muted skill-summary-text">{skill.summary}</div> : null}
                        </td>
                        <td><span className="source-pill">{skill.dcc_type || t('common.status.unknown')}</span></td>
                        <td><span className={`badge ${skill.loaded ? 'badge-ok' : 'badge-muted'}`}>{skill.loaded ? t('skillPaths.state.loaded') : t('skillPaths.state.unloaded')}</span></td>
                        <td>{skill.action_count}</td>
                        <td>
                          <span>{t('skillPaths.usage.searchHits', { count: skill.adoption.search_hits })}</span>
                          <div className="muted">
                            {skill.adoption.best_rank == null ? t('skillPaths.usage.noRank') : t('skillPaths.usage.bestRank', { rank: skill.adoption.best_rank })}
                            {' · '}
                            {t('skillPaths.usage.selected', { count: skill.adoption.selected_count })}
                          </div>
                        </td>
                        <td>
                          <span className={`badge ${skill.adoption.failure_count > 0 || skill.adoption.load_error_count > 0 ? 'badge-err' : skill.adoption.used ? 'badge-ok' : skill.adoption.low_adoption ? 'badge-warn' : 'badge-muted'}`}>
                            {skill.adoption.low_adoption ? t('skillPaths.state.lowAdoption') : skill.adoption.used ? t('skillPaths.state.used') : t('skillPaths.state.notUsed')}
                          </span>
                          <div className="muted">
                            {t('skillPaths.usage.callsFailures', { calls: skill.adoption.call_count, failures: skill.adoption.failure_count })}
                          </div>
                          <div className="muted">
                            {skill.adoption.last_used ? t('skillPaths.usage.lastUsed', { time: formatTraceDate(skill.adoption.last_used ?? undefined) }) : t('skillPaths.usage.neverUsed')}
                          </div>
                        </td>
                        <td className="mono-path">{skill.instances.join(', ') || '—'}</td>
                        <td className="mono-path">{skill.tools.slice(0, 8).join(', ')}{skill.tools.length > 8 ? ` +${skill.tools.length - 8}` : ''}</td>
                      </tr>
                    ))
                  )}
                </tbody>
              </table>
              {selectedSkill ? (
                <SkillDetailPanel
                  skill={selectedSkill}
                  detail={skillDetail}
                  busy={skillDetailBusy}
                  onReload={() => void fetchSkillDetail(selectedSkill)}
                  onClose={() => {
                    setSelectedSkill(null);
                    setSkillDetail(null);
                  }}
                  t={t}
                />
              ) : null}
            </div>
            <div className="skill-inventory-section">
              <h3 className="section-kicker">{t('skillPaths.section.searchPaths')}</h3>
            </div>
            <div className="skill-path-add">
              <input
                type="text"
                className="list-search-input"
                placeholder={t('skillPaths.placeholder.addDirectoryPath')}
                value={skillPathInput}
                onChange={(e) => setSkillPathInput(e.target.value)}
                aria-label={t('skillPaths.input.newPath')}
              />
              <button className="refresh-btn" type="button" disabled={skillPathBusy} onClick={() => void addSkillPath()}>
                {t('skillPaths.action.addPath')}
              </button>
            </div>
            <table>
              <thead>
                <tr>
                  <th>{t('skillPaths.table.source')}</th>
                  <th>{t('skillPaths.table.pathAlias')}</th>
                  <th>{t('skillPaths.table.status')}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {skillPaths.length === 0 ? (
                  <EmptyRow columns={4}>{t('skillPaths.empty.paths')}</EmptyRow>
                ) : filteredSkillPaths.length === 0 ? (
                  <EmptyRow columns={4}>{t('skillPaths.empty.pathsSearch')}</EmptyRow>
                ) : (
                  filteredSkillPaths.map((row) => (
                    <tr key={`${row.source}-${row.path_hash ?? row.path}-${row.id ?? 'x'}`}>
                      <td>
                        <span className="source-pill" data-source={row.source}>
                          {row.source}
                        </span>
                      </td>
                      <td>
                        <span className="mono-path">{row.display_path ?? row.path}</span>
                        {row.path_alias ? <div className="muted">{row.path_alias}</div> : null}
                      </td>
                      <td>
                        <span className={`badge ${row.status === 'present' ? 'badge-ok' : row.status === 'missing' ? 'badge-warn' : 'badge-muted'}`}>
                          {row.status === 'present' ? t('skillPaths.state.present') : row.status === 'missing' ? t('skillPaths.state.missing') : row.status ?? t('common.status.unknown')}
                        </span>
                      </td>
                      <td>
                        {row.id != null ? (
                          <button type="button" className="linkish" disabled={skillPathBusy} onClick={() => void deleteSkillPath(row.id!)}>
                            {t('common.action.remove')}
                          </button>
                        ) : (
                          '—'
                        )}
                      </td>
                    </tr>
                  ))
                )}
              </tbody>
            </table>
          </section>
        )}

        {activePanel === 'logs' && (
          <LogsPanel
            logs={logs}
            filteredLogs={filteredLogs}
            severityCounts={logSeverityCounts}
            severityFilter={logSeverityFilter}
            updatedAt={updatedAt.logs}
            error={errors.logs}
            onSeverityFilterChange={setLogSeverityFilter}
            onRefresh={fetchLogs}
            t={t}
          />
        )}
      </main>
    </div>
  );
}

export default App;
