import { type InterpolationValues, type MessageKey } from './i18n';

export type Translator = (key: MessageKey, values?: InterpolationValues) => string;

export type Panel = 'setup' | 'debug' | 'activity' | 'health' | 'instances' | 'tools' | 'workflows' | 'tasks' | 'openapi' | 'calls' | 'traces' | 'traffic' | 'stats' | 'governance' | 'logs' | 'skill-paths' | 'analytics' | 'marketplace' | 'integrations' | 'discover' | 'overview';

export type AnalyticsOverview = {
  range: string;
  period_start: string;
  period_end: string;
  kpi: {
    calls_total: number;
    calls_failed: number;
    failure_rate_pct: string;
    success_rate_pct: string;
    tokens_input_total: number;
    tokens_output_total: number;
    tokens_response_saved: number;
    tokens_total: number;
    llm_tokens_total: number;
    avg_duration_ms: string;
    avg_tokens_per_call: string;
    unique_instances: number;
    unique_agents: number;
  };
  top_tools: { name: string; calls: number; failures: number; success_rate_pct: number; avg_duration_ms: number }[];
  daily_series: { date: string; dcc_type: string; calls: number; failures: number; tokens_input: number; tokens_output: number }[];
};

export type AnalyticsTimeseriesPoint = {
  date: string;
  calls: number;
  failures: number;
  tokens_input: number;
  tokens_output: number;
  avg_duration_ms: string;
  max_duration_ms?: number;
};

export type AnalyticsHeatmapCell = {
  weekday: number;
  hour: number;
  calls: number;
  failures: number;
  avg_duration_ms: number;
  tokens_total: number;
};

export type SignalTone = 'ok' | 'warn' | 'err';
export type LatencySeverity = 'slow' | 'critical';

export const SLOW_LATENCY_MS = 1_000;
export const CRITICAL_LATENCY_MS = 5_000;

export type DebugSignal = {
  key: string;
  label: string;
  value: string;
  detail: string;
  tone: SignalTone;
  panel: Panel;
  traceId?: string;
};

export type FailureSignal = {
  request_id: string;
  status: string;
  tool: string;
  detail: string;
  ms: number | null;
};

export type HealthPayload = {
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

export type GatewaySentinel = {
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

export type ToolRow = {
  slug: string;
  dcc_type: string;
  summary: string;
  skill_name?: string | null;
  name?: string;
  instance_id?: string;
  instance_prefix?: string;
};

export type AttributionTrust = {
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

export type CallRow = {
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

export type TraceRow = {
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
  llm_usage?: LlmUsage | null;
  payload_token_estimator?: string | null;
  links?: AdminLinks;
};

export type TokenAccounting = {
  response_format?: string | null;
  token_estimator?: string | null;
  original_bytes?: number | null;
  returned_bytes?: number | null;
  original_tokens?: number | null;
  returned_tokens?: number | null;
  saved_tokens?: number | null;
  savings_pct?: number | string | null;
};

/** Upstream LLM billing token counts — separate from byte4 TokenAccounting. */
export type LlmUsage = {
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
  total_tokens?: number | null;
  model?: string | null;
};

export type TokenCarrier = {
  token_accounting?: TokenAccounting | null;
  llm_usage?: LlmUsage | null;
  response_format?: string | null;
  token_estimator?: string | null;
  original_bytes?: number | null;
  returned_bytes?: number | null;
  original_tokens?: number | null;
  returned_tokens?: number | null;
  saved_tokens?: number | null;
  savings_pct?: number | string | null;
};

export type AdminLinks = {
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

export type TrafficLinks = {
  admin_traffic_url?: string;
  traffic_api_url?: string;
  traffic_export_jsonl_url?: string;
};

export type TrafficFrameEnvelope = {
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

export type TrafficFrameAttributes = {
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

export type TrafficCaptureStatus = {
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

export type TrafficPayload = {
  schema_version?: string;
  total?: number;
  frames?: TrafficFrameEnvelope[];
  capture_status?: TrafficCaptureStatus;
  links?: TrafficLinks;
};

export type AgentContext = {
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

export type TracePayload = {
  content: string;
  mime_type: string;
  truncated: boolean;
  original_size: number;
  estimated_tokens?: number | null;
};

export type TraceSpan = {
  name: string;
  started_ns: number;
  duration_ns: number;
  ok: boolean;
  attributes?: Record<string, unknown>;
};

export type TraceDetailPayload = {
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

export type OpenApiSpec = {
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

export type OpenApiOperationObject = {
  operationId?: string;
  summary?: string;
  description?: string;
  tags?: string[];
  parameters?: unknown[];
  requestBody?: unknown;
  responses?: Record<string, unknown>;
};

export type OpenApiOperationRow = {
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

export type OpenApiSource = {
  label: string;
  specUrl: string;
  docsUrl: string;
  inspectorUrl: string;
  kind: 'gateway' | 'instance';
};

export type ActivityEvent = {
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

export type TaskRow = {
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

export type TaskRelated = {
  workflow_ids?: string[];
  request_ids?: string[];
  trace_ids?: string[];
  session_ids?: string[];
};

export type TaskArtifact = {
  name: string;
  kind: string;
  request_id?: string | null;
};

export type TaskValidation = {
  title: string;
  status: string;
  request_id?: string | null;
};

export type WorkflowAgent = {
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

export type WorkflowSearchSignal = {
  search_id: string;
  selected_rank?: number | null;
  selected_score?: number | null;
  match_reasons?: string[];
  zero_results?: boolean | null;
  result_count?: number | null;
  first_success_ms?: number | null;
};

export type WorkflowStep = {
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

export type WorkflowRow = {
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

export type WorkflowGraphStage = 'intent' | 'discovery' | 'skillLoad' | 'toolCalls' | 'fallbacks' | 'artifacts' | 'validation' | 'report';

export type WorkflowGraphNode = {
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

export type LatencyBlock = {
  min_ms?: number;
  max_ms?: number;
  mean_ms?: number;
  p50_ms?: number;
  p95_ms?: number;
  p99_ms?: number;
};

export type TopEntry = { name: string; count: number };

export type AttributionFacet = {
  name: string;
  count: number;
  failed?: number;
  failure_rate?: number;
  mean_latency_ms?: number;
  p95_latency_ms?: number;
};

export type TokenBreakdownEntry = {
  name: string;
  calls: number;
  returned_tokens: number;
  saved_tokens: number;
  savings_pct: number;
};

export type TokenUsageStats = {
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

export type PayloadTokenUsageStats = {
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

export type StatsPayload = {
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

export type GovernanceStats = {
  recent_allowed?: number;
  recent_policy_denied?: number;
  recent_throttled?: number;
  captured_frames?: number;
  skipped_capture_frames?: number;
  redacted_path_count?: number;
  redacted_paths?: string[];
};

export type GovernancePolicyPayload = {
  read_only?: boolean;
  unrestricted?: boolean;
  allowlists_active?: Record<string, boolean>;
  allowed_dcc_types?: string[];
  allowed_skill_names?: string[];
  allowed_skill_families?: string[];
  allowed_tool_slugs?: string[];
  allowed_tool_slug_prefixes?: string[];
};

export type GovernanceTrafficPayload = {
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

export type GovernanceCaptureDecision = {
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

export type GovernanceMiddlewareControl = {
  kind: string;
  mode: string;
  summary: string;
  config?: Record<string, unknown>;
};

export type GovernancePayload = {
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

export type GovernanceDecisionRow = {
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

export type InstanceRow = {
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
  dispatch_status?: string | null;
  dispatch_ready?: boolean;
  dispatch_ready_at_unix?: string | null;
  host_rpc_uri?: string | null;
  host_rpc_scheme?: string | null;
};

export type InstanceSummary = {
  live: number;
  stale: number;
  unhealthy: number;
};

export type InstanceUpdatePayload = {
  status: 'available' | 'binary_not_found' | 'download_failed' | 'manifest_error' | 'not_configured' | 'stage_failed' | 'staged' | 'up_to_date';
  error?: string | null;
  message?: string | null;
  hint?: string | null;
  instance_id?: string | null;
  instance_short?: string | null;
  binary_name?: string | null;
  current_version?: string | null;
  current_version_source?: string | null;
  latest_version?: string | null;
  download_url?: string | null;
  sha256?: string | null;
  release_notes?: string | null;
  update_available?: boolean;
  requires_restart?: boolean;
  staged_at?: string | null;
};

export type SkillPathRow = {
  path: string;
  display_path?: string;
  source_label?: string;
  path_tail?: string;
  path_alias?: string;
  path_hash?: string;
  path_redacted?: boolean;
  status?: string;
  exists?: boolean;
  source: string;
  id?: number;
};

export type SkillAdoptionMetrics = {
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

/// Author-supplied visual identity for a skill's marketplace card.
/// Mirrors `dcc_mcp_models::SkillBranding` — see Rust source for field semantics.
export type SkillBranding = {
  accent_color?: string | null;
  secondary_color?: string | null;
  emoji?: string | null;
  logo_url?: string | null;
  tagline?: string | null;
};

/// Author-supplied external link set. Mirrors `dcc_mcp_models::SkillLinks`.
export type SkillLinks = {
  docs?: string | null;
  repo?: string | null;
  homepage?: string | null;
  issues?: string | null;
  chat?: string | null;
};

export type SkillRow = {
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
  branding?: SkillBranding | null;
  links?: SkillLinks | null;
  example_prompts?: string[];
};

export type SkillInstanceRef = {
  id: string;
  prefix: string;
  dcc_type?: string;
};

export type SkillPayload = {
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

export type SkillDetailInstance = {
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

export type SkillDetailPayload = {
  skill?: SkillDetailInstance | null;
  instances?: SkillDetailInstance[];
  error?: string | null;
};

export type SetupUrlMode = 'local' | 'lan' | 'direct';
export type ClientPlatform = 'windows' | 'macos' | 'linux';

export type IdeTarget = {
  id: string;
  label: string;
  configPath: string | Record<ClientPlatform, string>;
  icon: string;
  build: (url: string) => string;
};

export function stringList(value: unknown): string[] {
  return Array.isArray(value) ? value.map((item) => String(item)) : [];
}

export function recordOrNull(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' ? value as Record<string, unknown> : null;
}

export function normalizeSkillInstanceRef(raw: unknown, fallbackPrefix = '', fallbackDcc = ''): SkillInstanceRef {
  const o = recordOrNull(raw);
  const id = String(o?.id ?? o?.instance_id ?? fallbackPrefix);
  const prefix = String(o?.prefix ?? o?.instance_short ?? fallbackPrefix ?? id);
  return {
    id,
    prefix,
    dcc_type: String(o?.dcc_type ?? fallbackDcc),
  };
}

export function defaultSkillAdoption(): SkillAdoptionMetrics {
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

export function normalizeSkillAdoption(raw: unknown): SkillAdoptionMetrics {
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

export function safeSkillPathDisplay(source: string, id?: number): string {
  const safeSource = source.replace(/[^\w:.-]+/g, '_').slice(0, 64) || 'skill_path';
  return `${safeSource} #${id ?? 1}`;
}

export function normalizeSkillPathRow(raw: unknown): SkillPathRow {
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
    source_label: o.source_label == null ? undefined : String(o.source_label),
    path_tail: o.path_tail == null ? undefined : String(o.path_tail),
    path_alias: o.path_alias == null ? undefined : String(o.path_alias),
    path_hash: o.path_hash == null ? undefined : String(o.path_hash),
    path_redacted: o.path_redacted !== false,
    status: o.status == null ? undefined : String(o.status),
    exists: o.exists == null ? undefined : o.exists === true,
    source,
    id: safeId,
  };
}

export function normalizeSkillRow(raw: unknown): SkillRow {
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
    branding: normalizeSkillBranding(o.branding),
    links: normalizeSkillLinks(o.links),
    example_prompts: stringList(o.example_prompts ?? o['example-prompts']),
  };
}

function normalizeSkillBranding(raw: unknown): SkillBranding | null {
  const o = recordOrNull(raw);
  if (!o) return null;
  const b: SkillBranding = {
    accent_color: o.accent_color == null ? null : String(o.accent_color),
    secondary_color: o.secondary_color == null ? null : String(o.secondary_color),
    emoji: o.emoji == null ? null : String(o.emoji),
    logo_url: o.logo_url == null ? null : String(o.logo_url),
    tagline: o.tagline == null ? null : String(o.tagline),
  };
  // Discard fully-empty objects so the UI can rely on `branding ?? fallback`.
  if (!b.accent_color && !b.secondary_color && !b.emoji && !b.logo_url && !b.tagline) {
    return null;
  }
  return b;
}

function normalizeSkillLinks(raw: unknown): SkillLinks | null {
  const o = recordOrNull(raw);
  if (!o) return null;
  const l: SkillLinks = {
    docs: o.docs == null ? null : String(o.docs),
    repo: o.repo == null ? null : String(o.repo),
    homepage: o.homepage == null ? null : String(o.homepage),
    issues: o.issues == null ? null : String(o.issues),
    chat: o.chat == null ? null : String(o.chat),
  };
  if (!l.docs && !l.repo && !l.homepage && !l.issues && !l.chat) {
    return null;
  }
  return l;
}

export function normalizeSkillDetailInstance(raw: unknown): SkillDetailInstance {
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

export function normalizeSkillDetailPayload(raw: unknown): SkillDetailPayload {
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

/// Marketplace catalog entry — mirrors dcc_mcp_catalog::CatalogEntry.
export type MarketplaceEntry = {
  name: string;
  description: string;
  dcc: string[];
  url?: string | null;
  tags: string[];
  version?: string | null;
  min_core_version?: string | null;
  maintainer?: string | null;
  icon?: string | null;
  source_name?: string;
  source_url?: string;
  install?: {
    type: string;
    url?: string | null;
    ref?: string | null;
  } | null;
};

/// Installed marketplace package — mirrors CLI domain InstalledMarketplacePackage.
export type InstalledMarketplacePackage = {
  name: string;
  dcc: string;
  version?: string | null;
  path: string;
  source_name: string;
  source_url: string;
  install_type: string;
  install_url?: string | null;
  install_ref?: string | null;
  installed_at_ms: number;
};

/// Marketplace install result.
export type MarketplaceInstallResult = {
  installed: boolean;
  name: string;
  dcc: string;
  version?: string | null;
  path: string;
  skill_search_path: string;
  install_type: string;
  reload_required: boolean;
};

/// Marketplace uninstall result.
export type MarketplaceUninstallResult = {
  uninstalled: boolean;
  name: string;
  dcc: string;
  path: string;
  removed_state: boolean;
  removed_files: boolean;
  reload_required: boolean;
};

// ── Marketplace M2 (PIP-699 / PIP-700) ─────────────────────────────────────

/// A single marketplace source entry (GET /sources).
export type MarketplaceSourceEntry = {
  name: string;
  url: string;
  origin: string;
};

/// Payload for GET /marketplace/sources.
export type MarketplaceSourcesPayload = {
  sources: MarketplaceSourceEntry[];
};

/// Request for POST /marketplace/sources.
export type MarketplaceAddSourceRequest = {
  source: string;
};

/// An outdated installed package entry (GET /outdated).
export type MarketplaceOutdatedPackage = {
  name: string;
  dcc: string;
  installed_version?: string | null;
  latest_version?: string | null;
  source_name: string;
  source_url: string;
  install_type: string;
  install_url?: string | null;
  install_ref?: string | null;
  path: string;
};

/// Payload for GET /marketplace/outdated.
export type MarketplaceOutdatedPayload = {
  dcc?: string | null;
  count: number;
  packages: MarketplaceOutdatedPackage[];
};

/// Request for POST /marketplace/update.
export type MarketplaceUpdateRequest = {
  name?: string;
  dcc?: string;
  all?: boolean;
};

/// A single update result item (POST /update).
export type MarketplaceUpdateResultItem = {
  updated: boolean;
  name: string;
  dcc: string;
  previous_version?: string | null;
  new_version?: string | null;
  path: string;
  install_type: string;
  source_name: string;
  source_url: string;
  reload_required: boolean;
};

/// Payload for POST /marketplace/update.
export type MarketplaceUpdatePayload = {
  updated: number;
  results: MarketplaceUpdateResultItem[];
};

/// Structured error envelope returned by marketplace API endpoints.
/// Backend sends `{ error: { kind, message } }` on failures.
export type MarketplaceErrorKind =
  | 'not_found'
  | 'already_installed'
  | 'dcc_mismatch'
  | 'ambiguous_dcc'
  | 'missing_install'
  | 'unsupported_install_type'
  | 'missing_skill'
  | 'command_failed'
  | 'hash_mismatch'
  | 'archive_error'
  | 'invalid_path_component'
  | 'admin_api_html'
  | 'internal_error';

/// Structured error envelope from marketplace API.
export type MarketplaceErrorEnvelope = {
  kind: MarketplaceErrorKind;
  message: string;
};

/// Integration kind — the integration types managed by the Integrations panel.
export type IntegrationKind = 'sentry' | 'webhooks' | 'wecom' | 'otlp';

/// Integration status — lifecycle state of a single integration.
export type IntegrationStatus = 'active' | 'inactive' | 'pending_restart';

/// Per-field env lock descriptor — whether a config field is locked to an env var.
export type EnvLockedField = {
  key: string;
  locked: boolean;
  env_var: string;
};

/// A single integration entry returned by GET /admin/api/integrations.
export type IntegrationEntry = {
  kind: IntegrationKind;
  label: string;
  description: string;
  status: IntegrationStatus;
  config: Record<string, unknown>;
  env_locked_fields: EnvLockedField[];
  error?: string;
};

/// Payload for GET /admin/api/integrations.
export type IntegrationsPayload = {
  integrations: IntegrationEntry[];
};

/// Request body for PUT /admin/api/integrations.
export type UpdateIntegrationRequest = {
  kind: IntegrationKind;
  config: Record<string, unknown>;
};

/// Response from PUT /admin/api/integrations.
export type UpdateIntegrationResult = IntegrationEntry;

/// Request body for POST /admin/api/integrations/test.
export type TestIntegrationRequest = {
  kind: IntegrationKind;
  config: Record<string, unknown>;
};

/// Response from POST /admin/api/integrations/test.
export type TestIntegrationResult = {
  kind: IntegrationKind;
  status: 'sent';
  message: string;
  sent_at_ms?: number;
  webhook_url?: string;
  wecom?: Record<string, unknown>;
};

/// DCC-type → icon URL (local SVGs, bundled by Vite + vite-plugin-singlefile).
/// Unknown/missing types fall back to a generic puzzle-piece icon.
