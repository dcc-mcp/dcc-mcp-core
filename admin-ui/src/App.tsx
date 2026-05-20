import { type ReactNode, useCallback, useEffect, useMemo, useState } from 'react';
import mayaIcon from './assets/icons/autodesk.svg';
import blenderIcon from './assets/icons/blender.svg';
import gimpIcon from './assets/icons/gimp.svg';
import inkscapeIcon from './assets/icons/inkscape.svg';
import kritaIcon from './assets/icons/krita.svg';
import unityIcon from './assets/icons/unity.svg';
import unrealIcon from './assets/icons/unrealengine.svg';
import substancePainterIcon from './assets/icons/photoshop.svg';
import puzzleIcon from './assets/icons/puzzle.svg';

type Panel = 'debug' | 'activity' | 'health' | 'instances' | 'tools' | 'tasks' | 'openapi' | 'calls' | 'traces' | 'stats' | 'logs' | 'skill-paths';

type HealthPayload = {
  status: string;
  instances_ready: number;
  instances_total: number;
  uptime_secs: number;
  version: string;
  rss_bytes?: number | null;
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
  parent_request_id?: string | null;
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
  span_count?: number | null;
  input_bytes?: number | null;
  output_bytes?: number | null;
  slowest_span_name?: string | null;
  slowest_span_ms?: number | null;
  links?: AdminLinks;
};

type AdminLinks = {
  admin_trace_url?: string;
  trace_api_url?: string;
  debug_bundle_url?: string;
  issue_report_url?: string;
  openapi_inspector_url?: string;
  openapi_spec_url?: string;
  openapi_docs_url?: string;
  stats_url?: string;
  admin_traces_url?: string;
};

type AgentContext = {
  agent_id?: string | null;
  agent_name?: string | null;
  agent_kind?: string | null;
  model?: string | null;
  task?: string | null;
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
    parent_request_id?: string;
  };
};

type TaskRow = {
  task_id: string;
  task_type: string;
  status: string;
  title: string;
  started_at: string;
  duration_ms?: number | null;
  correlation?: ActivityEvent['correlation'];
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

type StatsPayload = {
  range: string;
  total_calls: number;
  successful_calls?: number;
  failed_calls?: number;
  success_rate: number;
  p50_ms?: number | null;
  p95_ms?: number | null;
  latency_ms?: LatencyBlock;
  top_tools?: TopEntry[];
  top_instances?: TopEntry[];
  top_agents?: TopEntry[];
  hourly_distribution?: number[];
  error?: string;
};

type WorkerRow = {
  instance_id: string;
  display_name: string;
  dcc_type: string;
  status: string;
  stale: boolean;
  pid: number | null;
  uptime_secs: number | null;
  version: string | null;
  adapter_version: string | null;
  cpu_percent: number | null;
  memory_bytes: number | null;
  mcp_url: string;
  scene?: string | null;
};

type WorkerSummary = {
  live: number;
  stale: number;
  unhealthy: number;
};

type LogRow = {
  timestamp: string;
  level: string;
  message: string;
  source?: string;
  event?: string;
  dcc_type?: string;
  instance_id?: string | null;
  request_id?: string;
  tool?: string;
  success?: boolean;
  detail?: string;
  reason?: string | null;
};

type RequestLogGroup = {
  requestId: string;
  timestamp: string;
  tool: string;
  dccType: string;
  status: string;
  success?: boolean;
  steps: LogRow[];
};

type SkillPathRow = {
  path: string;
  source: string;
  id?: number;
};

function normalizeLogRow(raw: unknown): LogRow {
  if (!raw || typeof raw !== 'object') {
    return { timestamp: '', level: '', message: '' };
  }
  const o = raw as Record<string, unknown>;
  return {
    timestamp: String(o.timestamp ?? ''),
    level: String(o.level ?? ''),
    message: String(o.message ?? ''),
    source: o.source != null ? String(o.source) : undefined,
    event: o.event != null ? String(o.event) : undefined,
    dcc_type: o.dcc_type != null ? String(o.dcc_type) : undefined,
    instance_id:
      o.instance_id === null || o.instance_id === undefined
        ? null
        : String(o.instance_id),
    request_id: o.request_id != null ? String(o.request_id) : undefined,
    tool: o.tool != null ? String(o.tool) : undefined,
    success: typeof o.success === 'boolean' ? o.success : undefined,
    detail: o.detail != null ? String(o.detail) : undefined,
    reason: o.reason == null ? null : String(o.reason),
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
const OPENAPI_METHODS = new Set(['get', 'put', 'post', 'delete', 'patch', 'options', 'head', 'trace']);
const PANELS: { id: Panel; label: string; group: string }[] = [
  { id: 'debug', label: 'Debug', group: 'Operations' },
  { id: 'instances', label: 'Instances', group: 'Operations' },
  { id: 'activity', label: 'Activity', group: 'Operations' },
  { id: 'health', label: 'Health', group: 'Operations' },
  { id: 'tasks', label: 'Tasks', group: 'Workspace' },
  { id: 'tools', label: 'Tools', group: 'Workspace' },
  { id: 'openapi', label: 'OpenAPI Inspector', group: 'Contracts' },
  { id: 'stats', label: 'Stats', group: 'Observability' },
  { id: 'traces', label: 'Traces', group: 'Observability' },
  { id: 'calls', label: 'Calls', group: 'Observability' },
  { id: 'logs', label: 'Logs', group: 'Observability' },
  { id: 'skill-paths', label: 'Skill paths', group: 'Configuration' },
];

const PANEL_ID_SET = new Set<Panel>(PANELS.map((p) => p.id));

const STATS_RANGE_IDS = new Set(['1h', '24h', '7d', 'all']);

function gatewayDocsHref(): string {
  return `${window.location.origin}/docs`;
}

function gatewayOpenApiHref(): string {
  return `${window.location.origin}/v1/openapi.json`;
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

function traceLinks(requestId: string, provided?: AdminLinks): AdminLinks {
  const encoded = encodeURIComponent(requestId);
  return {
    admin_trace_url: provided?.admin_trace_url ?? fullHrefForAdmin('traces', { trace: requestId }),
    trace_api_url: provided?.trace_api_url ?? `${API_BASE}/traces/${encoded}`,
    debug_bundle_url: provided?.debug_bundle_url ?? `${API_BASE}/debug-bundle/${encoded}`,
    issue_report_url: provided?.issue_report_url ?? `${API_BASE}/issue-report/${encoded}`,
    openapi_inspector_url: provided?.openapi_inspector_url ?? fullHrefForAdmin('openapi'),
    openapi_spec_url: provided?.openapi_spec_url ?? gatewayOpenApiHref(),
    openapi_docs_url: provided?.openapi_docs_url ?? gatewayDocsHref(),
    stats_url: provided?.stats_url ?? fullHrefForAdmin('stats', { range: readStatsRangeFromUrl() }),
    admin_traces_url: provided?.admin_traces_url ?? fullHrefForAdmin('traces'),
  };
}

function readPanelFromUrl(): Panel {
  const u = new URL(window.location.href);
  const raw = u.searchParams.get('panel') ?? u.searchParams.get('tab');
  if (raw === 'workers') {
    return 'instances';
  }
  return isPanelId(raw) ? raw : 'debug';
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

function backendAccessUrls(mcpUrl: string): { origin: string; mcp: string; docs: string } {
  const u = new URL(mcpUrl);
  if (isLoopbackHost(u.hostname)) {
    u.hostname = window.location.hostname;
  }
  const origin = u.origin;
  return { origin, mcp: u.toString(), docs: `${origin}/docs` };
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

async function issueReportJsonText(requestId: string): Promise<string> {
  const payload = await apiJson<unknown>(`/issue-report/${encodeURIComponent(requestId)}`);
  return JSON.stringify(payload, null, 2);
}

function issueReportFilename(requestId: string): string {
  const safe = requestId.replace(/[^A-Za-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'request';
  return `dcc-mcp-issue-report-${safe}.json`;
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

async function fetchOpenApiSpecText(): Promise<{ spec: OpenApiSpec; raw: string }> {
  const ctrl = new AbortController();
  const tid = window.setTimeout(() => ctrl.abort(), ADMIN_FETCH_TIMEOUT_MS);
  try {
    const response = await fetch(gatewayOpenApiHref(), { signal: ctrl.signal });
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

function formatTime(value: string | null | undefined): string {
  if (!value) {
    return '-';
  }
  return new Date(value).toLocaleTimeString();
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

function statusClass(value: string): string {
  const status = value.toLowerCase();
  if (status.includes('fail') || status.includes('error') || status.includes('err') || status.includes('rejected') || status.includes('cancel')) {
    return 'badge badge-err';
  }
  if (status.includes('ok') || status.includes('success') || status.includes('complete') || status.includes('completed') || status.includes('done') || status.includes('ready') || status.includes('available')) {
    return 'badge badge-ok';
  }
  if (status.includes('stale') || status.includes('booting') || status.includes('warn') || status.includes('pending') || status.includes('running') || status.includes('busy') || status.includes('queued')) {
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
    debug: ['M6 6h12v12H6z', 'M9 9h6v6H9z'],
    activity: ['M4 14h4l2-5 4 9 2-4h4'],
    health: ['M12 4l7 4v5c0 4-3 7-7 8-4-1-7-4-7-8V8z'],
    instances: ['M6 7h12v10H6z', 'M9 4h6', 'M9 20h6'],
    tools: ['M5 19l5-5', 'M14 5l5 5', 'M12 7l5 5-5 5-5-5z'],
    tasks: ['M6 7h12', 'M6 12h12', 'M6 17h12', 'M4 7h.01', 'M4 12h.01', 'M4 17h.01'],
    calls: ['M7 7h10v10H7z', 'M10 10h4v4h-4z'],
    traces: ['M5 7h4v4H5z', 'M15 13h4v4h-4z', 'M9 9l6 6'],
    stats: ['M5 18V9', 'M12 18V5', 'M19 18v-6', 'M4 18h16'],
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

function toolGroupLabel(tool: ToolRow): string {
  const p = tool.instance_prefix ?? '—';
  return `${tool.dcc_type} · instance ${p}`;
}

function callGroupLabel(call: CallRow): string {
  const id = call.instance_id;
  if (typeof id === 'string' && id.length > 0) {
    return `${call.dcc_type} · ${id.length > 8 ? id.slice(0, 8) : id}`;
  }
  return `${call.dcc_type} · unrouted`;
}

function traceGroupLabel(trace: TraceRow): string {
  const id = trace.instance_id;
  if (typeof id === 'string' && id.length > 0) {
    const dcc = trace.dcc_type ?? '?';
    return `${dcc} · ${id.length > 8 ? id.slice(0, 8) : id}`;
  }
  return `${trace.dcc_type ?? '?'} · unrouted`;
}

function gatewayLogGroupLabel(log: LogRow): string {
  const dcc = log.dcc_type ?? '?';
  const raw = log.instance_id;
  if (typeof raw === 'string' && raw.length > 0) {
    return `${dcc} · ${raw.length > 8 ? raw.slice(0, 8) : raw}`;
  }
  return `${dcc} · gateway`;
}

function logStepTitle(log: LogRow): string {
  if (log.event) {
    return String(log.event);
  }
  if (log.tool) {
    return log.tool;
  }
  return log.source ?? 'event';
}

function logStepDetail(log: LogRow): string {
  const parts = [log.message];
  if (log.detail) parts.push(log.detail);
  if (log.reason) parts.push(log.reason);
  return parts.filter(Boolean).join(' — ');
}

function buildRequestLogGroups(rows: LogRow[]): RequestLogGroup[] {
  const map = new Map<string, LogRow[]>();
  for (const row of rows) {
    if (!row.request_id) {
      continue;
    }
    const bucket = map.get(row.request_id) ?? [];
    bucket.push(row);
    map.set(row.request_id, bucket);
  }
  return Array.from(map.entries())
    .map(([requestId, steps]) => {
      const sorted = [...steps].sort((a, b) => (a.timestamp || '').localeCompare(b.timestamp || ''));
      const newest = sorted[sorted.length - 1] ?? steps[0];
      return {
        requestId,
        timestamp: newest?.timestamp ?? '',
        tool: newest?.tool ?? newest?.message ?? 'unknown tool',
        dccType: newest?.dcc_type ?? '?',
        status: newest?.success === false ? 'failed' : 'ok',
        success: newest?.success,
        steps: sorted,
      };
    })
    .sort((a, b) => (b.timestamp || '').localeCompare(a.timestamp || ''));
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
  return value > 5_000 ? 'warn' : 'ok';
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

function agentLabel(row: { agent_name?: string | null; agent_id?: string | null; agent_model?: string | null }): string {
  return row.agent_name || row.agent_id || row.agent_model || '-';
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

function isProblemLog(log: LogRow): boolean {
  const text = haystack(log.level, log.message, log.event ?? '', log.reason ?? '', log.detail ?? '');
  return log.success === false || text.includes('error') || text.includes('warn') || text.includes('timeout') || text.includes('failed') || text.includes('stale');
}

function MiniSparkline({ buckets }: { buckets: number[] }) {
  const values = buckets.length ? buckets : Array.from({ length: 24 }, () => 0);
  const max = Math.max(1, ...values);
  return (
    <div className="mini-sparkline" role="img" aria-label="Call distribution sparkline">
      {values.map((value, index) => (
        <span key={index} style={{ height: `${Math.max(5, (value / max) * 100)}%` }} title={`${index}:00 UTC — ${value} call(s)`} />
      ))}
    </div>
  );
}

function StatBarList({ title, items }: { title: string; items: TopEntry[] }) {
  const max = maxTopCount(items);
  return (
    <div className="chart-card">
      <h3 className="chart-title">{title}</h3>
      {!items.length ? <p className="empty">No data in this range.</p> : items.map((row) => (
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

function HourlyChart({ buckets }: { buckets: number[] }) {
  if (!buckets.length) {
    return null;
  }
  const max = Math.max(1, ...buckets);
  return (
    <div className="chart-card">
      <h3 className="chart-title">Calls by hour (UTC)</h3>
      <div className="hourly-chart" role="img" aria-label="Hourly call distribution">
        {buckets.map((v, h) => (
          <div key={h} className="hour-col" title={`${h}:00 UTC — ${v} call(s)`}>
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

function payloadPreview(payload: TracePayload | null | undefined): string {
  if (!payload) {
    return 'No payload captured.';
  }
  const suffix = payload.truncated ? `\n\n[truncated from ${formatBytes(payload.original_size)}]` : '';
  return `${payload.content}${suffix}`;
}

function buildAgentPacket(trace: TraceDetailPayload): string {
  const links = traceLinks(trace.request_id, trace.links);
  const agent = trace.agent_context;
  return JSON.stringify({
    purpose: 'dcc-mcp admin trace packet for LLM evaluation and code optimization',
    request_id: trace.request_id,
    method: trace.method,
    tool: trace.tool_slug ?? trace.method,
    dcc_type: trace.dcc_type,
    instance_id: trace.instance_id,
    transport: trace.transport,
    status: trace.ok ? 'ok' : 'err',
    total_ms: trace.total_ms,
    agent_context: agent ? {
      agent_id: agent.agent_id,
      agent_name: agent.agent_name,
      model: agent.model,
      task: agent.task,
      reasoning_summary: agent.reasoning_summary,
      plan: agent.plan ?? [],
      observations: agent.observations ?? [],
      parent_request_id: agent.parent_request_id,
      tags: agent.tags ?? [],
    } : null,
    slowest_span: [...(trace.spans ?? [])]
      .sort((a, b) => (b.duration_ns ?? 0) - (a.duration_ns ?? 0))
      .slice(0, 1)
      .map((span) => ({ name: span.name, duration_ms: Math.round((span.duration_ns ?? 0) / 1_000_000), ok: span.ok, attributes: span.attributes }))
      [0] ?? null,
    links,
  }, null, 2);
}

function TraceLinks({ links }: { links: AdminLinks }) {
  const rows = [
    ['Admin trace', links.admin_trace_url],
    ['Trace API', links.trace_api_url],
    ['Debug bundle', links.debug_bundle_url],
    ['Issue report JSON', links.issue_report_url],
    ['OpenAPI Inspector', links.openapi_inspector_url],
    ['OpenAPI spec', links.openapi_spec_url],
    ['OpenAPI docs', links.openapi_docs_url],
    ['Stats', links.stats_url],
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

function TraceDetailPanel({
  trace,
  fallback,
  onCopy,
  onCopyIssueReport,
  onDownloadIssueReport,
}: {
  trace: TraceDetailPayload | null;
  fallback: string;
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
  const agentTitle = agent?.agent_name || agent?.agent_id || agent?.agent_kind || 'Caller context';
  const links = traceLinks(trace.request_id, trace.links);
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
          <span className="trace-kicker">Request</span>
          <h3 title={trace.request_id}>{compactId(trace.request_id)}</h3>
        </div>
        <div className="trace-copy-actions">
          <button className="refresh-btn" type="button" onClick={() => onCopy(links.admin_trace_url ?? '', 'trace URL')}>
            Copy URL
          </button>
          <button className="refresh-btn" type="button" onClick={() => onCopy(buildAgentPacket(trace), 'agent packet')}>
            Copy agent packet
          </button>
          <button className="refresh-btn" type="button" onClick={() => onCopyIssueReport(trace.request_id)}>
            Copy issue JSON
          </button>
          <button className="refresh-btn" type="button" onClick={() => onDownloadIssueReport(trace.request_id)}>
            Download JSON
          </button>
        </div>
        <div className="trace-summary-grid">
          <span><strong>Tool</strong>{trace.tool_slug ?? trace.method}</span>
          <span><strong>Status</strong>{trace.ok ? 'ok' : 'err'}</span>
          <span><strong>Latency</strong>{formatDurationMs(trace.total_ms)}</span>
          <span><strong>Transport</strong>{trace.transport ?? '-'}</span>
          <span><strong>Started</strong>{formatTraceDate(trace.started_at)}</span>
          <span><strong>Spans</strong>{spans.length}</span>
        </div>
        <TraceLinks links={links} />
      </div>

      {agent ? (
        <div className="trace-detail-card agent-context-card">
          <div className="trace-card-head">
            <h3>{agentTitle}</h3>
            {agent.model ? <span>{agent.model}</span> : null}
          </div>
          {agent.task ? <p className="agent-task">{agent.task}</p> : null}
          {agent.reasoning_summary ? <p className="agent-summary">{agent.reasoning_summary}</p> : null}
          {agent.plan?.length ? (
            <div className="agent-list">
              <strong>Plan</strong>
              {agent.plan.map((step, index) => <span key={`${step}-${index}`}>{step}</span>)}
            </div>
          ) : null}
          {agent.observations?.length ? (
            <div className="agent-list">
              <strong>Observations</strong>
              {agent.observations.map((step, index) => <span key={`${step}-${index}`}>{step}</span>)}
            </div>
          ) : null}
          <div className="agent-meta">
            {agent.parent_request_id ? <span>parent {compactId(agent.parent_request_id)}</span> : null}
            {agent.trace_id ? <span>trace {compactId(agent.trace_id)}</span> : null}
            {agent.tags?.map((tag) => <span key={tag}>{tag}</span>)}
          </div>
        </div>
      ) : null}

      <div className="trace-detail-card">
        <div className="trace-card-head">
          <h3>Span waterfall</h3>
          <span>{formatDurationMs(trace.total_ms)}</span>
        </div>
        <div className="span-waterfall">
          {spans.length === 0 ? <p className="empty">No spans captured.</p> : spans.map((span, index) => (
            <div className={`span-row ${span.ok ? 'ok' : 'err'}`} key={`${span.name}-${index}`}>
              <div className="span-row-label">
                <strong>{span.name}</strong>
                <span>{formatDurationMs(Math.round((span.duration_ns ?? 0) / 1_000_000))}</span>
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
            <h3>Input</h3>
            <span>{formatBytes(trace.input?.original_size)}</span>
          </div>
          <pre className="payload-pre">{payloadPreview(trace.input)}</pre>
        </div>
        <div className="trace-detail-card">
          <div className="trace-card-head">
            <h3>Output</h3>
            <span>{formatBytes(trace.output?.original_size)}</span>
          </div>
          <pre className="payload-pre">{payloadPreview(trace.output)}</pre>
        </div>
      </div>
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
}: {
  spec: OpenApiSpec | null;
  raw: string;
  operations: OpenApiOperationRow[];
}) {
  if (!spec) {
    return <p className="empty">No OpenAPI document loaded.</p>;
  }
  const pathsCount = Object.keys(spec.paths ?? {}).length;
  const tagCount = new Set(operations.flatMap((operation) => operation.tags)).size || (spec.tags?.length ?? 0);
  const methods = Array.from(new Set(operations.map((operation) => operation.method))).sort();
  const specLinks: AdminLinks = {
    openapi_inspector_url: fullHrefForAdmin('openapi'),
    openapi_spec_url: gatewayOpenApiHref(),
    openapi_docs_url: gatewayDocsHref(),
  };

  return (
    <div className="openapi-inspector">
      <div className="metric-grid compact">
        <MetricTile label="OpenAPI" value={spec.openapi ?? '-'} detail={spec.info?.title ?? 'Gateway REST contract'} />
        <MetricTile label="Version" value={spec.info?.version ?? '-'} />
        <MetricTile label="Paths" value={pathsCount} />
        <MetricTile label="Operations" value={operations.length} />
        <MetricTile label="Schemas" value={componentSchemaCount(spec)} />
        <MetricTile label="Tags" value={tagCount} />
      </div>

      <div className="openapi-layout">
        <div className="openapi-operation-list">
          <div className="trace-group-head">
            <h3>Operations</h3>
            <span>{operations.length} · {methods.join(', ') || '-'}</span>
          </div>
          {operations.length === 0 ? <p className="empty">No operations match your search.</p> : operations.map((operation) => (
            <article className="openapi-operation-card" key={operation.key}>
              <div className="openapi-operation-head">
                <span className={`method-pill ${operation.method.toLowerCase()}`}>{operation.method}</span>
                <span className="openapi-path">{operation.path}</span>
              </div>
              <h3>{operation.operationId}</h3>
              {operation.summary ? <p>{operation.summary}</p> : null}
              <div className="openapi-meta-row">
                <span>{operation.hasRequestBody ? 'body' : 'no body'}</span>
                <span>{operation.parameterCount} params</span>
                <span>{operation.responseCodes.length ? `responses ${operation.responseCodes.join(', ')}` : 'no responses'}</span>
                {operation.tags.map((tag) => <span key={tag}>{tag}</span>)}
              </div>
            </article>
          ))}
        </div>

        <div className="trace-detail-card openapi-spec-card">
          <div className="trace-card-head">
            <h3>Contract Links</h3>
            <span>{formatBytes(raw.length)}</span>
          </div>
          <TraceLinks links={specLinks} />
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
  const [activePanel, setActivePanel] = useState<Panel>(() => readPanelFromUrl());
  const [health, setHealth] = useState<HealthPayload | null>(null);
  const [activity, setActivity] = useState<ActivityEvent[]>([]);
  const [tools, setTools] = useState<ToolRow[]>([]);
  const [tasks, setTasks] = useState<TaskRow[]>([]);
  const [calls, setCalls] = useState<CallRow[]>([]);
  const [traces, setTraces] = useState<TraceRow[]>([]);
  const [stats, setStats] = useState<StatsPayload | null>(null);
  const [statsRange, setStatsRange] = useState(() => readStatsRangeFromUrl());
  const [openApiSpec, setOpenApiSpec] = useState<OpenApiSpec | null>(null);
  const [openApiRaw, setOpenApiRaw] = useState('');
  const [workers, setWorkers] = useState<WorkerRow[]>([]);
  const [workerSummary, setWorkerSummary] = useState<WorkerSummary>({ live: 0, stale: 0, unhealthy: 0 });
  const [logs, setLogs] = useState<LogRow[]>([]);
  const [skillPaths, setSkillPaths] = useState<SkillPathRow[]>([]);
  const [skillPathInput, setSkillPathInput] = useState('');
  const [skillPathBusy, setSkillPathBusy] = useState(false);
  const [traceDetail, setTraceDetail] = useState<string>('Select a trace row for detail.');
  const [traceDetailPayload, setTraceDetailPayload] = useState<TraceDetailPayload | null>(null);
  const [callDetail, setCallDetail] = useState<string>('Select a call row for trace detail.');
  const [copiedNotice, setCopiedNotice] = useState<string>('');
  const [updatedAt, setUpdatedAt] = useState<Record<Panel, string>>({
    debug: 'Loading…',
    activity: 'Loading…',
    health: 'Loading…',
    instances: 'Loading…',
    tools: 'Loading…',
    tasks: 'Loading…',
    openapi: 'Loading…',
    calls: 'Loading…',
    traces: 'Loading…',
    stats: 'Loading…',
    logs: 'Loading…',
    'skill-paths': 'Loading…',
  });
  const [errors, setErrors] = useState<Partial<Record<Panel, string>>>({});
  const [listSearch, setListSearch] = useState('');

  useEffect(() => {
    const u = new URL(window.location.href);
    if (u.searchParams.get('panel') === 'workers') {
      u.searchParams.set('panel', 'instances');
      window.history.replaceState({}, '', `${u.pathname}${u.search}`);
    }
  }, []);

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
    if (!q) {
      return calls;
    }
    return calls.filter((c) =>
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
          c.parent_request_id ?? '',
        ),
      ),
    );
  }, [calls, listSearch]);

  const filteredTraces = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return traces;
    }
    return traces.filter((t) =>
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
          t.slowest_span_name ?? '',
        ),
      ),
    );
  }, [traces, listSearch]);

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
          task.started_at,
          String(task.duration_ms ?? ''),
          task.correlation?.request_id ?? '',
          task.correlation?.instance_id ?? '',
          task.correlation?.dcc_type ?? '',
          task.correlation?.workflow_id ?? '',
          task.correlation?.job_id ?? '',
        ),
      ),
    );
  }, [tasks, listSearch]);

  const filteredWorkers = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return workers;
    }
    return workers.filter((w) =>
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
  }, [workers, listSearch]);

  const filteredLogs = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return logs;
    }
    return logs.filter((l) =>
      matchesListFilter(
        q,
        haystack(
          l.timestamp,
          l.level,
          l.message,
          l.source ?? '',
          l.event != null ? String(l.event) : '',
          l.dcc_type ?? '',
          l.instance_id != null ? String(l.instance_id) : '',
          l.request_id ?? '',
          l.tool ?? '',
          l.detail ?? '',
          l.reason ?? '',
        ),
      ),
    );
  }, [logs, listSearch]);

  const requestLogGroups = useMemo(() => buildRequestLogGroups(filteredLogs), [filteredLogs]);

  const gatewayLogs = useMemo(
    () => filteredLogs.filter((log) => !log.request_id),
    [filteredLogs],
  );

  const filteredSkillPaths = useMemo(() => {
    const q = listSearch.trim().toLowerCase();
    if (!q) {
      return skillPaths;
    }
    return skillPaths.filter((r) =>
      matchesListFilter(q, haystack(r.path, r.source, r.id != null ? String(r.id) : '')),
    );
  }, [skillPaths, listSearch]);

  const failedCalls = useMemo(
    () => calls.filter((call) => call.success === false || call.status.toLowerCase().includes('err') || call.status.toLowerCase().includes('fail')).slice(0, 8),
    [calls],
  );

  const slowTraces = useMemo(
    () => [...traces].filter((trace) => trace.total_ms != null).sort((a, b) => traceLatency(b) - traceLatency(a)).slice(0, 8),
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

  const unhealthyWorkers = useMemo(
    () => workers.filter((worker) => worker.stale || !statusClass(worker.status).includes('ok')),
    [workers],
  );

  const debugIssues = failedCalls.length + problemActivity.length + problemLogs.length + unhealthyWorkers.length;

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

  const taskSummary = useMemo(() => {
    const completed = tasks.filter((task) => isOkStatus(task.status)).length;
    const failed = tasks.filter((task) => isErrStatus(task.status)).length;
    const active = tasks.filter((task) => isWarnStatus(task.status)).length;
    return { completed, failed, active };
  }, [tasks]);

  const traceSummary = useMemo(() => {
    const ok = traces.filter((trace) => isOkStatus(trace.status)).length;
    const failed = traces.filter((trace) => isErrStatus(trace.status)).length;
    const p95 = stats?.latency_ms?.p95_ms ?? stats?.p95_ms ?? null;
    const agentContext = traces.filter((trace) => agentLabel(trace) !== '-').length;
    const spans = traces.reduce((sum, trace) => sum + (trace.span_count ?? 0), 0);
    return { ok, failed, p95, agentContext, spans };
  }, [stats, traces]);

  const statsSummary = useMemo(() => {
    const failed = stats?.failed_calls ?? Math.max(0, (stats?.total_calls ?? 0) - (stats?.successful_calls ?? 0));
    const success = stats?.successful_calls ?? Math.max(0, (stats?.total_calls ?? 0) - failed);
    return { success, failed };
  }, [stats]);

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
      setCopiedNotice(`Copied ${label}`);
      window.setTimeout(() => setCopiedNotice(''), 1800);
    } catch (error) {
      setCopiedNotice(`Copy failed: ${error instanceof Error ? error.message : String(error)}`);
      window.setTimeout(() => setCopiedNotice(''), 2400);
    }
  }, []);

  const copyIssueReport = useCallback(async (requestId: string) => {
    try {
      const text = await issueReportJsonText(requestId);
      await copyText(text, 'issue report JSON');
    } catch (error) {
      setCopiedNotice(`Issue report export failed: ${error instanceof Error ? error.message : String(error)}`);
      window.setTimeout(() => setCopiedNotice(''), 2400);
    }
  }, [copyText]);

  const downloadIssueReport = useCallback(async (requestId: string) => {
    try {
      const text = await issueReportJsonText(requestId);
      downloadJsonText(issueReportFilename(requestId), text);
      setCopiedNotice(`Downloaded issue report JSON`);
      window.setTimeout(() => setCopiedNotice(''), 1800);
    } catch (error) {
      setCopiedNotice(`Issue report export failed: ${error instanceof Error ? error.message : String(error)}`);
      window.setTimeout(() => setCopiedNotice(''), 2400);
    }
  }, []);

  const fetchActivity = useCallback(async () => {
    try {
      const payload = await apiJson<{ events: ActivityEvent[] }>('/activity?limit=300');
      setActivity(Array.isArray(payload.events) ? payload.events : []);
      markUpdated('activity', `${payload.events?.length ?? 0} event(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('activity', error);
    }
  }, [markError, markUpdated]);

  const fetchHealth = useCallback(async () => {
    try {
      const payload = await apiJson<HealthPayload>('/health');
      setHealth(payload);
      markUpdated('health', `Last updated: ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('health', error);
    }
  }, [markError, markUpdated]);

  const fetchInstanceBackends = useCallback(async () => {
    try {
      const payload = await apiJson<{ workers: WorkerRow[]; summary: WorkerSummary }>('/workers');
      setWorkers(payload.workers);
      setWorkerSummary(payload.summary);
      markUpdated(
        'instances',
        `${payload.workers.length} instance(s) (live ${payload.summary.live}, stale ${payload.summary.stale}, unhealthy ${payload.summary.unhealthy}) — ${new Date().toLocaleTimeString()}`,
      );
    } catch (error) {
      markError('instances', error);
    }
  }, [markError, markUpdated]);

  const fetchTools = useCallback(async () => {
    try {
      const payload = await apiJson<{ tools: ToolRow[] }>('/tools');
      setTools(payload.tools);
      markUpdated('tools', `${payload.tools.length} tool(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('tools', error);
    }
  }, [markError, markUpdated]);

  const fetchOpenApi = useCallback(async () => {
    try {
      const { spec, raw } = await fetchOpenApiSpecText();
      const operations = flattenOpenApiOperations(spec);
      setOpenApiSpec(spec);
      setOpenApiRaw(raw);
      markUpdated('openapi', `${operations.length} operation(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('openapi', error);
    }
  }, [markError, markUpdated]);

  const fetchCalls = useCallback(async () => {
    try {
      const payload = await apiJson<{ calls: CallRow[] }>('/calls');
      const rows = Array.isArray(payload.calls) ? payload.calls : [];
      setCalls(rows);
      markUpdated('calls', `${rows.length} call(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('calls', error);
    }
  }, [markError, markUpdated]);

  const fetchTraces = useCallback(async () => {
    try {
      const payload = await apiJson<{ traces: TraceRow[] }>('/traces?limit=200');
      const rows = Array.isArray(payload.traces) ? payload.traces : [];
      setTraces(rows);
      markUpdated('traces', `${rows.length} trace(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('traces', error);
    }
  }, [markError, markUpdated]);

  const fetchTasks = useCallback(async () => {
    try {
      const payload = await apiJson<{ tasks: TaskRow[] }>('/tasks?limit=300');
      setTasks(Array.isArray(payload.tasks) ? payload.tasks : []);
      markUpdated('tasks', `${payload.tasks?.length ?? 0} task(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('tasks', error);
    }
  }, [markError, markUpdated]);

  const fetchStats = useCallback(async () => {
    try {
      const payload = await apiJson<StatsPayload>(`/stats?range=${encodeURIComponent(statsRange)}`);
      setStats(payload);
      markUpdated('stats', `Range ${payload.range} — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('stats', error);
    }
  }, [markError, markUpdated, statsRange]);

  const fetchLogs = useCallback(async () => {
    try {
      const payload = await apiJson<{ logs?: unknown[] }>('/logs');
      const raw = Array.isArray(payload.logs) ? payload.logs : [];
      setLogs(raw.map(normalizeLogRow));
      markUpdated('logs', `${raw.length} event(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('logs', error);
    }
  }, [markError, markUpdated]);

  const fetchSkillPaths = useCallback(async () => {
    try {
      const payload = await apiJson<{ paths: SkillPathRow[] }>('/skill-paths');
      setSkillPaths(Array.isArray(payload.paths) ? payload.paths : []);
      markUpdated(
        'skill-paths',
        `${payload.paths?.length ?? 0} path(s) — ${new Date().toLocaleTimeString()}`,
      );
    } catch (error) {
      markError('skill-paths', error);
    }
  }, [markError, markUpdated]);

  const fetchDebug = useCallback(async () => {
    await Promise.allSettled([
      fetchHealth(),
      fetchInstanceBackends(),
      fetchActivity(),
      fetchCalls(),
      fetchTraces(),
      fetchStats(),
      fetchLogs(),
    ]);
    markUpdated('debug', `Debug snapshot — ${new Date().toLocaleTimeString()}`);
  }, [fetchActivity, fetchCalls, fetchHealth, fetchInstanceBackends, fetchLogs, fetchStats, fetchTraces, markUpdated]);

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
      await fetchSkillPaths();
    } catch (error) {
      markError('skill-paths', error);
    } finally {
      setSkillPathBusy(false);
    }
  }, [fetchSkillPaths, markError, skillPathInput]);

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
        await fetchSkillPaths();
      } catch (error) {
        markError('skill-paths', error);
      } finally {
        setSkillPathBusy(false);
      }
    },
    [fetchSkillPaths, markError],
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
    (panel: Panel, opts?: { traceId?: string | null; range?: string | null; replace?: boolean }) => {
      const u = new URL(window.location.href);
      u.searchParams.set('panel', panel);
      u.searchParams.delete('range');
      u.searchParams.delete('trace');
      if (panel === 'stats') {
        const r = opts?.range;
        if (r && STATS_RANGE_IDS.has(r)) {
          u.searchParams.set('range', r);
        }
      }
      if (panel === 'traces' && opts?.traceId) {
        u.searchParams.set('trace', opts.traceId);
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
    (panel: Panel, opts?: { traceId?: string; range?: string; replace?: boolean }) => {
      let effectiveRange = statsRange;
      if (opts?.range && STATS_RANGE_IDS.has(opts.range)) {
        effectiveRange = opts.range;
        setStatsRange(opts.range);
      }
      setActivePanel(panel);
      pushAdminUrl(panel, {
        traceId: opts?.traceId,
        range: panel === 'stats' ? effectiveRange : null,
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
    const tid = readTraceIdFromUrl();
    if (panel === 'traces' && tid) {
      void fetchTraceInto(tid, 'trace');
    }
  }, [fetchTraceInto]);

  const fetchPanel = useCallback((panel: Panel) => {
    if (panel === 'debug') void fetchDebug();
    if (panel === 'activity') void fetchActivity();
    if (panel === 'health') void fetchHealth();
    if (panel === 'instances') void fetchInstanceBackends();
    if (panel === 'tools') void fetchTools();
    if (panel === 'openapi') void fetchOpenApi();
    if (panel === 'tasks') void fetchTasks();
    if (panel === 'calls') void fetchCalls();
    if (panel === 'traces') void fetchTraces();
    if (panel === 'stats') void fetchStats();
    if (panel === 'skill-paths') void fetchSkillPaths();
    if (panel === 'logs') void fetchLogs();
  }, [fetchActivity, fetchCalls, fetchDebug, fetchHealth, fetchInstanceBackends, fetchLogs, fetchOpenApi, fetchSkillPaths, fetchStats, fetchTasks, fetchTools, fetchTraces]);

  useEffect(() => {
    fetchPanel(activePanel);
    const timer = window.setInterval(() => fetchPanel(activePanel), 5000);
    return () => window.clearInterval(timer);
  }, [activePanel, fetchPanel]);

  return (
    <div className="app-shell">
      <nav className="side-rail" aria-label="Admin navigation">
        <div className="brand-lockup">
          <div className="brand-accent" aria-hidden="true" />
          <div className="brand-text">
            <h1>Admin Dashboard</h1>
            <p className="brand-tag">DCC-MCP Gateway</p>
          </div>
        </div>
        <div className="nav-links">
          {PANELS.map((panel, index) => {
            const showGroup = index === 0 || PANELS[index - 1].group !== panel.group;
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
              href={gatewayDocsHref()}
              className="nav-link"
              target="_blank"
              rel="noopener noreferrer"
              title="Open gateway OpenAPI docs"
            >
              <DocsIcon />
              <span>Docs</span>
            </a>
          </div>
        </div>
      </nav>
      <main className="main-stage">
        {activePanel !== 'health' && activePanel !== 'debug' && (
          <div className="list-search-wrap">
            <input
              type="search"
              className="list-search-input"
              placeholder={activePanel === 'stats' ? 'Filter top tools / instances…' : activePanel === 'openapi' ? 'Filter operations, paths, tags…' : 'Search this panel…'}
              value={listSearch}
              onChange={(e) => setListSearch(e.target.value)}
              aria-label="Filter current panel"
            />
            {listSearch.trim() ? (
              <span className="list-search-meta">
                {activePanel === 'activity' ? `${filteredActivity.length} / ${activity.length}` : ''}
                {activePanel === 'instances' ? `${filteredWorkers.length} / ${workers.length}` : ''}
                {activePanel === 'tools' ? `${filteredTools.length} / ${tools.length}` : ''}
                {activePanel === 'openapi' ? `${filteredOpenApiOperations.length} / ${openApiOperations.length}` : ''}
                {activePanel === 'tasks' ? `${filteredTasks.length} / ${tasks.length}` : ''}
                {activePanel === 'calls' ? `${filteredCalls.length} / ${calls.length}` : ''}
                {activePanel === 'traces' ? `${filteredTraces.length} / ${traces.length}` : ''}
                {activePanel === 'skill-paths' ? `${filteredSkillPaths.length} / ${skillPaths.length}` : ''}
                {activePanel === 'logs' ? `${filteredLogs.length} / ${logs.length}` : ''}
                {activePanel === 'stats' ? `charts: ${filteredTopTools.length} tools / ${filteredTopInstances.length} instances / ${filteredTopAgents.length} agents` : ''}
              </span>
            ) : null}
          </div>
        )}
        {activePanel === 'debug' && (
          <section className="panel active debug-panel">
            <div className="debug-hero">
              <div>
                <h2>Debug Workbench</h2>
                <StatusLine text={updatedAt.debug} error={errors.debug} />
              </div>
              <div className="debug-pulse">
                <span className={debugIssues > 0 ? 'pulse-dot warn' : 'pulse-dot ok'} />
                {debugIssues > 0 ? `${debugIssues} signals need attention` : 'No active warning signals'}
              </div>
            </div>
            <div className="debug-grid">
              <HealthCard tone={health?.status === 'ok' ? 'ok' : 'warn'} label="Gateway" value={gatewayLabel(health)} />
              <HealthCard tone={unhealthyWorkers.length ? 'warn' : 'ok'} label="Instances" value={`${workerSummary.live} live / ${unhealthyWorkers.length} flagged`} />
              <HealthCard tone={errorRateTone(stats)} label="Success" value={stats ? `${stats.success_rate.toFixed(1)}%` : '?'} />
              <HealthCard tone={latencyTone(stats?.latency_ms?.p95_ms ?? stats?.p95_ms)} label="p95 latency" value={stats?.latency_ms?.p95_ms ?? stats?.p95_ms ?? '-'} />
            </div>
            <div className="debug-map">
              <div className="debug-card debug-wide">
                <div className="debug-card-head">
                  <h3>Traffic Shape</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('stats')}>Open stats</button>
                </div>
                <MiniSparkline buckets={stats?.hourly_distribution ?? []} />
                <div className="debug-metrics">
                  <span>{stats?.total_calls ?? 0} calls</span>
                  <span>{stats?.latency_ms?.p50_ms ?? stats?.p50_ms ?? '-'} ms p50</span>
                  <span>{stats?.latency_ms?.p99_ms ?? '-'} ms p99</span>
                </div>
              </div>

              <div className="debug-card">
                <div className="debug-card-head">
                  <h3>Failed Calls</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('calls')}>Open calls</button>
                </div>
                {failedCalls.length === 0 ? <p className="empty">No failed calls in the retained window.</p> : failedCalls.map((call) => (
                  <button key={call.request_id} className="debug-row" type="button" onClick={() => goToPanel('traces', { traceId: call.request_id })}>
                    <span><StatusBadge value={call.status} /></span>
                    <span>{compactId(call.request_id)}</span>
                    <span title={call.error ?? call.tool}>{call.error ?? call.tool}</span>
                  </button>
                ))}
              </div>

              <div className="debug-card">
                <div className="debug-card-head">
                  <h3>Slowest Traces</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('traces')}>Open traces</button>
                </div>
                {slowTraces.length === 0 ? <p className="empty">No latency samples yet.</p> : slowTraces.map((trace) => (
                  <button key={trace.request_id} className="debug-row" type="button" onClick={() => goToPanel('traces', { traceId: trace.request_id })}>
                    <span>{trace.total_ms ?? '-'} ms</span>
                    <span>{compactId(trace.request_id)}</span>
                    <span title={trace.tool}>{trace.tool}</span>
                  </button>
                ))}
              </div>

              <div className="debug-card">
                <div className="debug-card-head">
                  <h3>Instance Signals</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('instances')}>Open instances</button>
                </div>
                {unhealthyWorkers.length === 0 ? <p className="empty">All retained instances look healthy.</p> : unhealthyWorkers.slice(0, 8).map((worker) => (
                  <div key={worker.instance_id} className="debug-row static">
                    <span><StatusBadge value={worker.stale ? 'stale' : worker.status} /></span>
                    <span>{worker.dcc_type}</span>
                    <span>{worker.display_name} · {compactId(worker.instance_id)}</span>
                  </div>
                ))}
              </div>

              <div className="debug-card">
                <div className="debug-card-head">
                  <h3>Event Warnings</h3>
                  <button className="linkish" type="button" onClick={() => goToPanel('logs')}>Open logs</button>
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
                    <span>{formatTime(row.timestamp)}</span>
                    <span>{row.source ?? row.level}</span>
                    <span title={row.message}>{row.message}</span>
                  </button>
                ))}
                {problemLogs.length === 0 && problemActivity.length === 0 ? <p className="empty">No warning events in the retained window.</p> : null}
              </div>
            </div>
            <button className="refresh-btn" type="button" onClick={fetchDebug}>Refresh snapshot</button>
          </section>
        )}
        {activePanel === 'activity' && (
          <section className="panel active activity-panel">
            <h2>Activity</h2>
            <StatusLine text={updatedAt.activity} error={errors.activity} />
            {activity.length === 0 ? <p className="empty">No activity recorded yet.</p> : filteredActivity.length === 0 ? (
              <p className="empty">No activity events match your search.</p>
            ) : (
              <table>
                <thead><tr><th>Time</th><th>Status</th><th>Kind</th><th>Message</th><th>DCC</th><th>Request</th><th>ms</th></tr></thead>
                <tbody>
                  {filteredActivity.map((event) => {
                    const requestId = event.correlation?.request_id;
                    return (
                      <tr key={event.event_id}>
                        <td>{formatTime(event.timestamp)}</td>
                        <td><StatusBadge value={event.status} /></td>
                        <td>{event.kind}</td>
                        <td title={event.message}>{event.message}</td>
                        <td>{event.correlation?.dcc_type ?? '-'}</td>
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
            <button className="refresh-btn" type="button" onClick={fetchActivity}>Refresh</button>
          </section>
        )}

        {activePanel === 'health' && (
          <section className="panel active health-panel">
            <h2>Health</h2>
            <StatusLine text={updatedAt.health} error={errors.health} />
            <div className="health-grid">
              <HealthCard tone={health?.status === 'ok' ? 'ok' : 'warn'} label="Status" value={health?.status ?? '?'} />
              <HealthCard label="Uptime" value={formatUptime(health?.uptime_secs)} />
              <HealthCard tone={health && health.instances_ready > 0 ? 'ok' : 'warn'} label="Ready" value={`${health?.instances_ready ?? 0} / ${health?.instances_total ?? 0}`} />
              <HealthCard label="Version" value={health?.version ?? '?'} />
              <HealthCard label="Gateway owner" value={gatewayLabel(health)} />
              <HealthCard label="Gateway candidates" value={String(health?.gateway?.candidates?.length ?? 0)} />
              <HealthCard label="RSS" value={formatBytes(health?.rss_bytes ?? undefined)} />
              <HealthCard label="Body limit" value={health?.limits ? formatBytes(health.limits.body_max_bytes) : '?'} />
              <HealthCard
                label="Rate / min·IP"
                value={health?.limits ? (health.limits.rate_limit_per_minute_per_ip === 0 ? 'off' : String(health.limits.rate_limit_per_minute_per_ip)) : '?'}
              />
              <HealthCard
                label="XFF trusted depth"
                value={health?.limits ? String(health.limits.xff_trusted_depth) : '?'}
              />
              <HealthCard label="Read retries (max)" value={health?.limits ? String(health.limits.read_retry_max) : '?'} />
              <HealthCard label="Circuit limit / open" value={health?.limits ? `${health.limits.circuit_failure_threshold} / ${health.limits.circuit_open_secs}s` : '?'} />
              <HealthCard
                tone={health?.circuits && health.circuits.circuits_open > 0 ? 'warn' : undefined}
                label="Circuits open / tracked"
                value={health?.circuits ? `${health.circuits.circuits_open} / ${health.circuits.tracked_backends}` : '?'}
              />
            </div>
            <button className="refresh-btn" type="button" onClick={fetchHealth}>Refresh</button>
          </section>
        )}

        {activePanel === 'instances' && (
          <section className="panel active instances-panel">
            <h2>Instances</h2>
            <p className="empty log-hint">
              One row per registered DCC backend (same data as the former Workers tab). Use the links to open the adapter HTTP host, MCP streamable endpoint, or <code>/docs</code> when the host exposes it.
            </p>
            <StatusLine text={updatedAt.instances} error={errors.instances} />
            <div className="workers-grid">
              {workers.length === 0 ? (
                <p className="empty">No instances registered.</p>
              ) : filteredWorkers.length === 0 ? (
                <p className="empty">No instances match your search.</p>
              ) : (
                filteredWorkers.map((worker) => (
                  <div key={worker.instance_id} className={`worker-card ${worker.stale ? 'stale' : statusClass(worker.status).replace('badge badge-', '')}`}>
                    <div className="wname">
                      <img src={resolveDccIcon(worker.dcc_type)} alt="" className="dcc-icon" aria-hidden />
                      {worker.display_name} <span>{worker.instance_id.slice(0, 8)}</span>
                    </div>
                    <div className="wkv">
                      <span>DCC</span><span>{worker.dcc_type}</span>
                      <span>Status</span><span><StatusBadge value={worker.status} /></span>
                      <span>PID</span><span>{worker.pid ?? '-'}</span>
                      <span>Uptime</span><span>{formatUptime(worker.uptime_secs)}</span>
                      <span>Version</span><span>{worker.version ?? '-'}</span>
                      <span>Adapter</span><span>{worker.adapter_version ?? '-'}</span>
                      <span>Scene</span><span>{worker.scene ?? '-'}</span>
                      <span>CPU%</span><span>{worker.cpu_percent == null ? '-' : worker.cpu_percent.toFixed(1)}</span>
                      <span>Memory</span><span>{formatBytes(worker.memory_bytes)}</span>
                      <span>Access URL</span><span><BackendAccessUrl mcpUrl={worker.mcp_url} /></span>
                      <span>Endpoints</span><span><McpBackendLinks mcpUrl={worker.mcp_url} /></span>
                    </div>
                  </div>
                ))
              )}
            </div>
            <div className="status-bar">Summary: live {workerSummary.live}, stale {workerSummary.stale}, unhealthy {workerSummary.unhealthy}</div>
            <button className="refresh-btn" type="button" onClick={fetchInstanceBackends}>Refresh</button>
          </section>
        )}

        {activePanel === 'tools' && (
          <section className="panel active tools-panel">
            <h2>Tools</h2>
            <StatusLine text={updatedAt.tools} error={errors.tools} />
            {tools.length === 0 ? <p className="empty">No tools registered.</p> : filteredTools.length === 0 ? (
              <p className="empty">No tools match your search.</p>
            ) : (
              Array.from(groupRows(filteredTools, toolGroupLabel).entries())
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([group, groupTools]) => (
                <div key={group} className="group-block">
                  <h3 className="group-title">{group}</h3>
                  <p className="group-meta">{groupTools.length} tool(s)</p>
                  <table>
                    <thead><tr><th>Slug</th><th>DCC</th><th>Summary</th></tr></thead>
                    <tbody>
                      {groupTools.map((tool) => (
                        <tr key={tool.slug}>
                          <td>{tool.slug}</td>
                          <td>{tool.dcc_type}</td>
                          <td>{tool.summary.slice(0, 120)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )))}
            <button className="refresh-btn" type="button" onClick={fetchTools}>Refresh</button>
          </section>
        )}

        {activePanel === 'openapi' && (
          <section className="panel active openapi-panel" data-panel="openapi">
            <PanelHeader
              title="OpenAPI Inspector"
              meta="Gateway REST contract behind the MCP tool surface."
              action={(
                <>
                  <a className="refresh-btn" href={gatewayDocsHref()} target="_blank" rel="noopener noreferrer">Open Reference</a>
                  <a className="refresh-btn" href={gatewayOpenApiHref()} target="_blank" rel="noopener noreferrer">Spec JSON</a>
                  <button className="refresh-btn" type="button" disabled={!openApiRaw} onClick={() => void copyText(openApiRaw, 'OpenAPI spec JSON')}>Copy JSON</button>
                  <button className="refresh-btn" type="button" disabled={!openApiRaw} onClick={() => {
                    downloadJsonText('dcc-mcp-gateway-openapi.json', openApiRaw);
                    setCopiedNotice('Downloaded OpenAPI spec JSON');
                    window.setTimeout(() => setCopiedNotice(''), 1800);
                  }}>Download JSON</button>
                  <button className="refresh-btn" type="button" onClick={fetchOpenApi}>Refresh</button>
                </>
              )}
            />
            <StatusLine text={copiedNotice || updatedAt.openapi} error={errors.openapi} />
            <OpenApiInspectorPanel
              spec={openApiSpec}
              raw={openApiRaw}
              operations={filteredOpenApiOperations}
            />
          </section>
        )}

        {activePanel === 'tasks' && (
          <section className="panel active tasks-panel">
            <PanelHeader
              title="Tasks"
              meta="Trace-derived work items, grouped for quick operator scanning."
              action={<button className="refresh-btn" type="button" onClick={fetchTasks}>Refresh</button>}
            />
            <StatusLine text={updatedAt.tasks} error={errors.tasks} />
            <div className="metric-grid compact">
              <MetricTile tone="ok" label="Completed" value={taskSummary.completed} />
              <MetricTile tone={taskSummary.failed > 0 ? 'err' : undefined} label="Failed" value={taskSummary.failed} />
              <MetricTile tone={taskSummary.active > 0 ? 'warn' : undefined} label="Active / waiting" value={taskSummary.active} />
              <MetricTile label="Visible" value={`${filteredTasks.length} / ${tasks.length}`} />
            </div>
            {tasks.length === 0 ? <p className="empty">No tasks reconstructed from traces yet.</p> : filteredTasks.length === 0 ? (
              <p className="empty">No tasks match your search.</p>
            ) : (
              <div className="task-board">
                {filteredTasks.map((task) => {
                  const requestId = task.correlation?.request_id;
                  const tone = isErrStatus(task.status) ? 'err' : isWarnStatus(task.status) ? 'warn' : isOkStatus(task.status) ? 'ok' : 'muted';
                  return (
                    <article key={task.task_id} className={`task-card ${tone}`}>
                      <div className="task-main">
                        <div className="task-title-row">
                          <StatusBadge value={task.status} />
                          <span className="task-type">{task.task_type}</span>
                          <span className="task-time">{formatTime(task.started_at)}</span>
                        </div>
                        <h3 title={task.title}>{task.title}</h3>
                        <div className="task-meta">
                          <span>{task.correlation?.dcc_type ?? 'gateway'}</span>
                          <span>{formatDurationMs(task.duration_ms)}</span>
                          <span>{compactId(task.correlation?.instance_id)}</span>
                        </div>
                      </div>
                      <div className="task-side">
                        {requestId ? (
                          <button className="link-chip" type="button" title={requestId} onClick={() => goToPanel('traces', { traceId: requestId })}>
                            trace {requestId.slice(0, 12)}
                          </button>
                        ) : (
                          <span className="mono-path">{task.task_id.slice(0, 12)}</span>
                        )}
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
            <h2>Recent Calls</h2>
            <StatusLine text={updatedAt.calls} error={errors.calls} />
            {calls.length === 0 ? <p className="empty">No recent calls. AuditMiddleware may not be active.</p> : filteredCalls.length === 0 ? (
              <p className="empty">No calls match your search.</p>
            ) : (
              Array.from(groupRows(filteredCalls, callGroupLabel).entries())
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([group, groupCalls]) => (
                <div key={group} className="group-block">
                  <h3 className="group-title">{group}</h3>
                  <table>
                    <thead><tr><th>Time</th><th>Request</th><th>Tool</th><th>DCC</th><th>Agent</th><th>Transport</th><th>Status</th><th>Error</th><th>ms</th><th>Detail</th></tr></thead>
                    <tbody>
                      {groupCalls.map((call) => (
                        <tr key={call.request_id}>
                          <td>{formatTime(call.timestamp)}</td>
                          <td>
                            <button className="refresh-btn" type="button" title={call.request_id} onClick={() => goToPanel('traces', { traceId: call.request_id })}>
                              {call.request_id.slice(0, 12)}
                            </button>
                          </td>
                          <td>{call.tool}</td>
                          <td>{call.dcc_type}</td>
                          <td title={call.agent_id ?? call.agent_name ?? ''}>{agentLabel(call)}</td>
                          <td>{call.transport ?? '-'}</td>
                          <td><StatusBadge value={call.status} /></td>
                          <td title={call.error ?? ''}>{call.error ? call.error.slice(0, 80) : '-'}</td>
                          <td>{call.duration_ms ?? '-'}</td>
                          <td>
                            <div className="table-actions">
                              <button className="refresh-btn" type="button" onClick={() => void fetchTraceInto(call.request_id, 'call')}>Expand</button>
                              <button className="refresh-btn" type="button" onClick={() => void copyText(traceLinks(call.request_id, call.links).admin_trace_url ?? '', 'trace URL')}>Copy URL</button>
                              <button className="refresh-btn" type="button" onClick={() => void copyIssueReport(call.request_id)}>Copy issue JSON</button>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )))}
            <pre className="empty">{callDetail}</pre>
            <button className="refresh-btn" type="button" onClick={fetchCalls}>Refresh</button>
          </section>
        )}

        {activePanel === 'traces' && (
          <section className="panel active traces-panel" data-panel="traces">
            <PanelHeader
              title="Traces"
              meta="Request timeline and latency drill-down for gateway fan-out."
              action={<button className="refresh-btn" type="button" onClick={fetchTraces}>Refresh</button>}
            />
            <StatusLine text={copiedNotice || updatedAt.traces} error={errors.traces} />
            <div className="metric-grid compact">
              <MetricTile tone="ok" label="OK" value={traceSummary.ok} />
              <MetricTile tone={traceSummary.failed > 0 ? 'err' : undefined} label="Failed" value={traceSummary.failed} />
              <MetricTile tone={latencyTone(traceSummary.p95)} label="p95 latency" value={formatDurationMs(traceSummary.p95)} />
              <MetricTile label="Agent ctx" value={traceSummary.agentContext} />
              <MetricTile label="Spans" value={traceSummary.spans} />
              <MetricTile label="Visible" value={`${filteredTraces.length} / ${traces.length}`} />
            </div>
            {traces.length === 0 ? <p className="empty">No traces recorded.</p> : filteredTraces.length === 0 ? (
              <p className="empty">No traces match your search.</p>
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
                          className={`trace-item ${isErrStatus(trace.status) ? 'err' : isWarnStatus(trace.status) ? 'warn' : isOkStatus(trace.status) ? 'ok' : ''}`}
                          type="button"
                          onClick={() => goToPanel('traces', { traceId: trace.request_id, replace: true })}
                        >
                          <span className="trace-item-main">
                            <strong>{trace.tool}</strong>
                            <span>{compactId(trace.request_id)} - {formatTime(trace.timestamp)} - {trace.transport ?? '?'}</span>
                            <span>{agentLabel(trace)}{trace.slowest_span_name ? ` - slowest ${trace.slowest_span_name} ${formatDurationMs(trace.slowest_span_ms)}` : ''}</span>
                          </span>
                          <span className="trace-item-side">
                            <StatusBadge value={trace.status} />
                            <span>{formatDurationMs(trace.total_ms)}</span>
                            <span>{trace.span_count ?? 0} spans</span>
                          </span>
                        </button>
                      ))}
                    </div>
                  ))}
                </div>
                <TraceDetailPanel
                  trace={traceDetailPayload}
                  fallback={traceDetail}
                  onCopy={copyText}
                  onCopyIssueReport={(requestId) => void copyIssueReport(requestId)}
                  onDownloadIssueReport={(requestId) => void downloadIssueReport(requestId)}
                />
              </div>
            )}
          </section>
        )}

        {activePanel === 'stats' && (
          <section className="panel active stats-panel" data-panel="stats">
            <PanelHeader
              title="Stats"
              meta="Gateway call volume, success rate, latency, and hot paths."
              action={(
                <div className="stats-actions">
                  <label className="range-label" htmlFor="stats-range-select">
                    Range
                    <select
                      id="stats-range-select"
                      aria-label="Range"
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
                  <button className="refresh-btn" type="button" onClick={fetchStats}>Refresh</button>
                </div>
              )}
            />
            <StatusLine text={updatedAt.stats} error={errors.stats} />
            {stats?.error ? <p className="empty">{stats.error}</p> : null}
            <div className="metric-grid">
              <MetricTile label="Calls" value={stats?.total_calls ?? 0} detail={`${statsRange} window`} />
              <MetricTile tone={errorRateTone(stats)} label="Success" value={stats ? `${stats.success_rate.toFixed(1)}%` : '0.0%'} detail={`${statsSummary.success} ok / ${statsSummary.failed} failed`} />
              <MetricTile tone={latencyTone(stats?.latency_ms?.p50_ms ?? stats?.p50_ms)} label="p50 latency" value={formatDurationMs(stats?.latency_ms?.p50_ms ?? stats?.p50_ms)} />
              <MetricTile tone={latencyTone(stats?.latency_ms?.p95_ms ?? stats?.p95_ms)} label="p95 latency" value={formatDurationMs(stats?.latency_ms?.p95_ms ?? stats?.p95_ms)} />
            </div>
            <div className="stats-charts">
              <StatBarList title="Top tools" items={filteredTopTools} />
              <StatBarList title="Top instances" items={filteredTopInstances} />
              <StatBarList title="Top agents" items={filteredTopAgents} />
              {stats?.hourly_distribution?.length ? <HourlyChart buckets={stats.hourly_distribution} /> : null}
            </div>
          </section>
        )}

        {activePanel === 'skill-paths' && (
          <section className="panel active skill-paths-panel">
            <h2>Skill search paths</h2>
            <StatusLine text={updatedAt['skill-paths']} error={errors['skill-paths']} />
            <p className="empty log-hint">
              Paths used for skill discovery (CLI, environment variables, bundled data dir, and optional SQLite-backed custom entries). Adding or removing a custom path persists to SQLite, re-runs disk catalog discovery in-process, and refreshes gateway capability data; Event Log records each change.
            </p>
            <div className="skill-path-add">
              <input
                type="text"
                className="list-search-input"
                placeholder="Add directory path…"
                value={skillPathInput}
                onChange={(e) => setSkillPathInput(e.target.value)}
                aria-label="New skill path"
              />
              <button className="refresh-btn" type="button" disabled={skillPathBusy} onClick={() => void addSkillPath()}>
                Add path
              </button>
            </div>
            <table>
              <thead>
                <tr>
                  <th>Source</th>
                  <th>Path</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {skillPaths.length === 0 ? (
                  <EmptyRow columns={3}>No paths reported.</EmptyRow>
                ) : filteredSkillPaths.length === 0 ? (
                  <EmptyRow columns={3}>No rows match your search.</EmptyRow>
                ) : (
                  filteredSkillPaths.map((row) => (
                    <tr key={`${row.source}-${row.path}-${row.id ?? 'x'}`}>
                      <td>
                        <span className="source-pill" data-source={row.source}>
                          {row.source}
                        </span>
                      </td>
                      <td className="mono-path">{row.path}</td>
                      <td>
                        {row.id != null ? (
                          <button type="button" className="linkish" disabled={skillPathBusy} onClick={() => void deleteSkillPath(row.id!)}>
                            Remove
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
            <button className="refresh-btn" type="button" onClick={fetchSkillPaths}>
              Refresh
            </button>
          </section>
        )}

        {activePanel === 'logs' && (
          <section className="panel active logs-panel">
            <h2>Event Log</h2>
            <StatusLine text={updatedAt.logs} error={errors.logs} />
            <p className="empty log-hint">
              Live request stream, refreshed every 5s. Rows with a request id are grouped like a run with ordered steps; gateway events without a request id stay in their own event lane.
            </p>
            {logs.length === 0 ? <p className="empty">No events in buffer yet. Use the gateway (tool calls) or wait for registry / probe activity.</p> : filteredLogs.length === 0 ? (
              <p className="empty">No log lines match your search.</p>
            ) : (
              <div className="live-log-board">
                {requestLogGroups.map((run) => (
                  <div key={run.requestId} className="request-run">
                    <div className="run-header">
                      <div>
                        <div className="run-title">
                          Request <span className="mono-path">{run.requestId}</span>
                        </div>
                        <div className="run-meta">
                          {formatTime(run.timestamp)} · {run.dccType} · {run.tool}
                        </div>
                      </div>
                      <StatusBadge value={run.status} />
                    </div>
                    <div className="run-steps">
                      {run.steps.map((log, idx) => (
                        <div key={`${log.timestamp}-${log.source ?? ''}-${idx}`} className="run-step">
                          <span className={`step-dot ${log.success === false ? 'err' : 'ok'}`} aria-hidden="true" />
                          <div className="step-body">
                            <div className="step-head">
                              <span className="step-name">Step {idx + 1}: {logStepTitle(log)}</span>
                              <span className="muted">{formatTime(log.timestamp)}</span>
                              <span className="source-pill" data-source={log.source ?? 'contention'}>{log.source ?? 'contention'}</span>
                            </div>
                            <div className="step-detail">{logStepDetail(log)}</div>
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                ))}
                {gatewayLogs.length > 0 ? (
                  <div className="group-block">
                    <h3 className="group-title">Gateway events</h3>
                    {Array.from(groupRows(gatewayLogs, gatewayLogGroupLabel).entries())
                      .sort(([a], [b]) => a.localeCompare(b))
                      .map(([group, groupLogs]) => (
                        <div key={group} className="gateway-event-group">
                          <p className="group-meta">{group}</p>
                          {groupLogs.map((log, idx) => (
                            <div key={`${log.timestamp}-${log.request_id ?? ''}-${idx}`} className="log-line">
                              <span className="source-pill" data-source={log.source ?? 'contention'}>{log.source ?? 'contention'}</span>
                              {' '}
                              <span className="muted">{formatTime(log.timestamp)}</span>
                              {' '}
                              <span className="warn-text">[{log.level}]</span>
                              {' '}
                              {log.event ? <span className="log-event">{String(log.event)}</span> : null}
                              {' '}
                              {log.message}
                              {log.detail ? <span className="muted"> — {log.detail}</span> : null}
                            </div>
                          ))}
                        </div>
                      ))}
                  </div>
                ) : null}
              </div>
            )}
            <button className="refresh-btn" type="button" onClick={fetchLogs}>Refresh</button>
          </section>
        )}
      </main>
    </div>
  );
}

export default App;
