import { type ReactNode, useEffect, useMemo, useState } from 'react';
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
import { SkillMarkdownPreview } from './components/SkillMarkdownPreview';
import { type MessageKey } from './i18n';
import { formatTime, timestampTitle } from './time';
import { CRITICAL_LATENCY_MS, recordOrNull, SLOW_LATENCY_MS, type ActivityEvent, type AdminLinks, type AgentContext, type AttributionFacet, type AttributionTrust, type CallRow, type ClientPlatform, type GatewaySentinel, type GovernanceMiddlewareControl, type HealthPayload, type IdeTarget, type InstanceRow, type LatencySeverity, type OpenApiOperationObject, type OpenApiOperationRow, type OpenApiSource, type OpenApiSpec, type Panel, type SkillDetailInstance, type SkillDetailPayload, type SkillRow, type StatsPayload, type TaskRow, type TokenAccounting, type TokenBreakdownEntry, type TokenCarrier, type ToolRow, type TopEntry, type TraceDetailPayload, type TracePayload, type TraceRow, type TraceSpan, type TrafficCaptureStatus, type TrafficFrameEnvelope, type Translator, type WorkflowAgent, type WorkflowGraphNode, type WorkflowGraphStage, type WorkflowRow, type WorkflowSearchSignal, type WorkflowStep } from './admin-types';

export const DCC_ICON_MAP: Record<string, string> = {
  maya: mayaIcon,
  blender: blenderIcon,
  gimp: gimpIcon,
  inkscape: inkscapeIcon,
  krita: kritaIcon,
  unity: unityIcon,
  unreal: unrealIcon,
  substance_painter: substancePainterIcon,
};
export const DCC_ICON_FALLBACK = puzzleIcon;

/// Resolve icon URL for a dcc_type, supporting prefix matching
/// (e.g. "autodesk_maya" → maya icon).
export function resolveDccIcon(dccType: string): string {
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
export function adminApiBase(): string {
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

export const API_BASE = adminApiBase();
/** Abort hung admin fetches so the UI does not wait indefinitely on a wedged gateway. */
export const ADMIN_FETCH_TIMEOUT_MS = 25_000;
export const DEFAULT_LOCAL_GATEWAY_PORT = '9765';
export const OPENAPI_METHODS = new Set(['get', 'put', 'post', 'delete', 'patch', 'options', 'head', 'trace']);
export const IDE_SERVER_NAME = 'dcc-mcp-gateway';
export const buildMcpServersConfig = (url: string) => JSON.stringify({
  mcpServers: {
    [IDE_SERVER_NAME]: { url },
  },
}, null, 2);
export const tomlString = (value: string) => JSON.stringify(value);
export const buildCodexConfig = (url: string) => [
  `[mcp_servers.${IDE_SERVER_NAME}]`,
  `url = ${tomlString(url)}`,
].join('\n');
export const IDE_TARGETS: IdeTarget[] = [
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
export type PanelDefinition = { id: Panel; labelKey: MessageKey; groupKey: MessageKey };

export const PANELS: PanelDefinition[] = [
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

export const PANEL_ID_SET = new Set<Panel>(PANELS.map((p) => p.id));

export const STATS_RANGE_IDS = new Set(['1h', '24h', '7d', 'all']);

export function gatewayDocsHref(): string {
  return `${window.location.origin}/docs`;
}

export function projectDocsHref(): string {
  return 'https://github.com/dcc-mcp/dcc-mcp-core/tree/main/docs';
}

export function gatewayOpenApiHref(): string {
  return `${window.location.origin}/v1/openapi.json`;
}

export function gatewayOpenApiSource(): OpenApiSource {
  return {
    label: 'Gateway REST API',
    specUrl: gatewayOpenApiHref(),
    docsUrl: gatewayDocsHref(),
    inspectorUrl: fullHrefForAdmin('openapi'),
    kind: 'gateway',
  };
}

export function isPanelId(value: string | null | undefined): value is Panel {
  return value != null && value !== '' && PANEL_ID_SET.has(value as Panel);
}

/** Admin HTML path without `/api` (honours custom `--admin-path`). */
export function adminShellPath(): string {
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
export function hrefForAdmin(panel: Panel, extra?: Record<string, string | undefined>): string {
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

export function fullHrefForAdmin(panel: Panel, extra?: Record<string, string | undefined>): string {
  return new URL(hrefForAdmin(panel, extra), window.location.origin).toString();
}

export function openApiInspectorHref(specUrl: string, docsUrl: string, label: string): string {
  return hrefForAdmin('openapi', { spec: specUrl, docs: docsUrl, label });
}

export function readOpenApiSourceFromUrl(): OpenApiSource {
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

export function traceLinks(requestId: string, provided?: AdminLinks): AdminLinks {
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

export function adminPathBases(): { adminBase: string; apiBase: string } {
  try {
    const apiBase = new URL(API_BASE).pathname.replace(/\/+$/, '') || '/admin/api';
    const adminBase = apiBase.endsWith('/api') ? apiBase.slice(0, -'/api'.length) || '/admin' : '/admin';
    return { adminBase, apiBase };
  } catch {
    return { adminBase: '/admin', apiBase: '/admin/api' };
  }
}

export function publicSafeIssuePaths(requestId: string): Record<string, string> {
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

export function publicToolFamily(tool: string | null | undefined, method: string): string {
  const raw = tool || method || 'unknown';
  const lastSegment = raw.split('.').filter(Boolean).pop() || raw;
  const family = lastSegment.includes('__') ? lastSegment.split('__').pop() || lastSegment : lastSegment;
  return family.replace(/[^A-Za-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'unknown';
}

export function readPanelFromUrl(): Panel {
  const u = new URL(window.location.href);
  const raw = u.searchParams.get('panel');
  return isPanelId(raw) ? raw : 'setup';
}

export function readStatsRangeFromUrl(): string {
  const u = new URL(window.location.href);
  const r = u.searchParams.get('range');
  return r && STATS_RANGE_IDS.has(r) ? r : '24h';
}

export function readTraceIdFromUrl(): string | null {
  const u = new URL(window.location.href);
  const t = u.searchParams.get('trace');
  return t != null && t.trim() !== '' ? t.trim() : null;
}

export function haystack(...parts: (string | number | null | undefined)[]): string {
  return parts
    .filter((p) => p != null && p !== '')
    .map((p) => String(p))
    .join(' ')
    .toLowerCase();
}

export function matchesListFilter(query: string, hay: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) {
    return true;
  }
  return hay.includes(q);
}

export function isLoopbackHost(hostname: string): boolean {
  const h = hostname.toLowerCase();
  return h === 'localhost' || h === '127.0.0.1' || h === '::1' || h === '[::1]';
}

export function backendAccessUrls(mcpUrl: string): { origin: string; mcp: string; docs: string; openapi: string } {
  const u = new URL(mcpUrl);
  if (isLoopbackHost(u.hostname)) {
    u.hostname = window.location.hostname;
  }
  const origin = u.origin;
  return { origin, mcp: u.toString(), docs: `${origin}/docs`, openapi: `${origin}/v1/openapi.json` };
}

export function urlHost(host: string): string {
  const trimmed = host.trim();
  if (trimmed === '0.0.0.0' || trimmed === '::') {
    return window.location.hostname;
  }
  if (trimmed.includes(':') && !trimmed.startsWith('[') && !trimmed.endsWith(']')) {
    return `[${trimmed}]`;
  }
  return trimmed;
}

export function gatewaySentinelMcpUrl(sentinel: GatewaySentinel | null | undefined): string | null {
  if (!sentinel || !sentinel.host || !Number.isFinite(sentinel.port) || sentinel.port <= 0) {
    return null;
  }
  try {
    return new URL('/mcp', `http://${urlHost(sentinel.host)}:${sentinel.port}`).toString();
  } catch {
    return null;
  }
}

export function configuredDevGatewayMcpUrl(): string | null {
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

export function gatewayMcpUrl(health: HealthPayload | null): string {
  return gatewaySentinelMcpUrl(health?.gateway?.current)
    ?? configuredDevGatewayMcpUrl()
    ?? new URL('/mcp', window.location.origin).toString();
}

export function gatewayMcpUrlFromPage(): string {
  return new URL('/mcp', window.location.origin).toString();
}

export function lanGatewayMcpUrl(): string | null {
  if (isLoopbackHost(window.location.hostname)) {
    return null;
  }
  return gatewayMcpUrlFromPage();
}

export function instanceSetupLabel(instance: InstanceRow): string {
  return `${instance.display_name || instance.dcc_type} (${instance.instance_id.slice(0, 8)})`;
}

export function detectClientPlatform(): ClientPlatform {
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

export function configPathForTarget(target: IdeTarget, platform: ClientPlatform): string {
  if (typeof target.configPath === 'string') {
    return target.configPath;
  }
  return target.configPath[platform] ?? target.configPath.linux;
}

export function ideConfigText(target: IdeTarget, url: string): string {
  return target.build(url);
}

export function configPathFileUrl(path: string): string | null {
  if (path.startsWith('%') || path.startsWith('~') || path.includes('->')) {
    return null;
  }
  const normalized = path.replace(/\\/g, '/');
  return normalized.match(/^[A-Za-z]:\//) ? `file:///${normalized}` : null;
}

export function instanceOpenApiSource(instance: InstanceRow): OpenApiSource {
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

export function BackendAccessUrl({ mcpUrl }: { mcpUrl: string }) {
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
export function McpBackendLinks({ mcpUrl }: { mcpUrl: string }) {
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

export async function apiJson<T>(path: string): Promise<T> {
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

export function BackendOpenApiLinks({ instance }: { instance: InstanceRow }) {
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

export async function issueReportJsonText(requestId: string): Promise<string> {
  const payload = await apiJson<unknown>(`/issue-report/${encodeURIComponent(requestId)}`);
  return JSON.stringify(payload, null, 2);
}

export function issueReportFilename(requestId: string): string {
  const safe = requestId.replace(/[^A-Za-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'request';
  return `dcc-mcp-issue-report-${safe}.json`;
}

export function openApiSpecFilename(label: string): string {
  const safe = label.replace(/[^A-Za-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'gateway';
  return `dcc-mcp-openapi-${safe}.json`;
}

export function downloadJsonText(filename: string, text: string): void {
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

export async function fetchOpenApiSpecText(specUrl: string): Promise<{ spec: OpenApiSpec; raw: string }> {
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

export function flattenOpenApiOperations(spec: OpenApiSpec | null): OpenApiOperationRow[] {
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

export function formatUptime(value: number | null | undefined): string {
  if (value == null) {
    return '-';
  }
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const seconds = value % 60;
  return `${hours}h ${minutes}m ${seconds}s`;
}

export function formatBytes(value: number | null | undefined): string {
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

export function totalTraceTokens(row: TraceRow): number | null {
  if (row.total_tokens != null) {
    return row.total_tokens;
  }
  if (row.input_tokens == null && row.output_tokens == null) {
    return null;
  }
  return (row.input_tokens ?? 0) + (row.output_tokens ?? 0);
}

export function detailTraceTokens(trace: TraceDetailPayload): {
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

export function statusClass(value: string): string {
  const status = value.toLowerCase();
  if (status.includes('fail') || status.includes('error') || status.includes('err') || status.includes('rejected') || status.includes('cancel') || status.includes('unavailable') || status.includes('unreachable')) {
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

export function isOkStatus(value: string | null | undefined): boolean {
  return statusClass(value ?? '').includes('badge-ok');
}

export function isErrStatus(value: string | null | undefined): boolean {
  return statusClass(value ?? '').includes('badge-err');
}

export function isWarnStatus(value: string | null | undefined): boolean {
  return statusClass(value ?? '').includes('badge-warn');
}

export function StatusBadge({ value }: { value: string }) {
  return <span className={statusClass(value)}>{value}</span>;
}

export function TimeValue({ value, className }: { value: string | null | undefined; className?: string }) {
  const title = timestampTitle(value);
  const text = formatTime(value);
  if (!title) {
    return <span className={className}>{text}</span>;
  }
  return <time className={className} dateTime={title} title={title}>{text}</time>;
}

export function StatusLine({ text, error }: { text: string; error?: string }) {
  return <div className="status-bar">{error ? `Error: ${error}` : text}</div>;
}

export function HealthCard({ tone, label, value }: { tone?: 'ok' | 'warn'; label: string; value: string | number }) {
  return (
    <div className={`health-card ${tone ?? ''}`}>
      <div className="label">{label}</div>
      <div className="value">{value}</div>
    </div>
  );
}

export function MetricTile({ tone, label, value, detail }: { tone?: 'ok' | 'warn' | 'err'; label: string; value: string | number; detail?: string }) {
  return (
    <div className={`metric-tile ${tone ?? ''}`}>
      <div className="metric-label">{label}</div>
      <div className="metric-value">{value}</div>
      {detail ? <div className="metric-detail">{detail}</div> : null}
    </div>
  );
}

/// TokenTracker-style hero metric: a large display number for a headline KPI.
/// `accent` highlights the card with the brand bar + green value (use for the
/// single most important number on the panel, e.g. total tokens).
export function HeroMetric({ label, value, detail, accent }: { label: string; value: string | number; detail?: ReactNode; accent?: boolean }) {
  return (
    <div className={`hero-metric ${accent ? 'accent' : ''}`}>
      <div className="hero-label">{label}</div>
      <div className="hero-value">{value}</div>
      {detail != null ? <div className="hero-detail">{detail}</div> : null}
    </div>
  );
}

export function PanelHeader({ title, meta, action }: { title: string; meta?: string; action?: ReactNode }) {
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

export function NavIcon({ panel }: { panel: Panel }) {
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

export function IdeIcon({ target }: { target: IdeTarget }) {
  return (
    <img className={`ide-icon ide-icon-${target.id}`} src={target.icon} alt="" aria-hidden="true" />
  );
}

export function DocsIcon() {
  return (
    <svg className="nav-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M6 4h9l3 3v13H6z" />
      <path d="M15 4v4h4" />
      <path d="M9 13h6" />
      <path d="M9 16h5" />
    </svg>
  );
}

export function EmptyRow({ columns, children }: { columns: number; children: string }) {
  return (
    <tr>
      <td colSpan={columns} className="empty">{children}</td>
    </tr>
  );
}

export type SkillDetailToolSummary = {
  name: string;
  summary?: string;
  annotations: string[];
};

export function skillDetailTools(detail: SkillDetailInstance | null | undefined): SkillDetailToolSummary[] {
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

export function SkillDetailPanel({
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
            {busy ? t('common.status.loading') : t('action.reload')}
          </button>
          <button className="linkish" type="button" onClick={onClose}>{t('action.close')}</button>
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
        copyLabel={t('action.copy')}
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

export function appTypeLabel(value: string | null | undefined): string {
  const app = (value ?? 'unknown').trim() || 'unknown';
  return `app-type: ${app}`;
}

export function compactInstanceId(value: string | null | undefined): string {
  if (typeof value !== 'string' || value.length === 0) {
    return 'unrouted';
  }
  return value.length > 8 ? value.slice(0, 8) : value;
}

export function toolInstanceLabel(tool: ToolRow): string {
  return tool.instance_prefix ?? compactInstanceId(tool.instance_id);
}

export function toolGroupLabel(tool: ToolRow): string {
  return appTypeLabel(tool.dcc_type);
}

export function instanceGroupLabel(instance: InstanceRow): string {
  return appTypeLabel(instance.dcc_type);
}

export function callGroupLabel(call: CallRow): string {
  return appTypeLabel(call.dcc_type);
}

export function traceGroupLabel(trace: TraceRow): string {
  return appTypeLabel(trace.dcc_type);
}

export function maxTopCount(items: TopEntry[]): number {
  if (!items.length) {
    return 1;
  }
  return Math.max(1, ...items.map((i) => i.count));
}

export function latencyTone(value: number | null | undefined): 'ok' | 'warn' | undefined {
  if (value == null) {
    return undefined;
  }
  return latencySeverity(value) ? 'warn' : 'ok';
}

export function latencySeverity(value: number | null | undefined): LatencySeverity | null {
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

export function isSlowLatency(value: number | null | undefined): boolean {
  return latencySeverity(value) != null;
}

export function latencyClass(value: number | null | undefined): string {
  const severity = latencySeverity(value);
  return severity ? `latency-${severity}` : '';
}

export function latencyBadgeKey(severity: LatencySeverity): MessageKey {
  return severity === 'critical' ? 'common.badge.tail' : 'common.badge.slow';
}

export function LatencyBadge({ value, t }: { value: number | null | undefined; t: Translator }) {
  const severity = latencySeverity(value);
  if (!severity) {
    return null;
  }
  return <span className={`badge badge-latency badge-latency-${severity}`}>{t(latencyBadgeKey(severity))}</span>;
}

export function LatencyValue({ value, t }: { value: number | null | undefined; t: Translator }) {
  return (
    <span className="latency-value">
      <span>{formatDurationMs(value)}</span>
      <LatencyBadge value={value} t={t} />
    </span>
  );
}

export function errorRateTone(stats: StatsPayload | null): 'ok' | 'warn' | undefined {
  if (!stats || stats.total_calls === 0) {
    return undefined;
  }
  return stats.success_rate < 95 ? 'warn' : 'ok';
}

export function traceLatency(trace: TraceRow): number {
  return trace.total_ms ?? -1;
}

export function spanDurationMs(span: TraceSpan): number {
  return Math.round((span.duration_ns ?? 0) / 1_000_000);
}

export function agentLabel(row: { agent_name?: string | null; agent_id?: string | null; agent_model?: string | null }): string {
  return row.agent_name || row.agent_id || row.agent_model || '-';
}

export function actorLabel(row: {
  actor?: string | null;
  actor_name?: string | null;
  actor_id?: string | null;
  auth_subject?: string | null;
  actor_email_hash?: string | null;
}): string {
  return row.actor || row.actor_name || row.actor_id || row.auth_subject || row.actor_email_hash || '-';
}

export function platformLabel(row: { client_platform?: string | null; client_os?: string | null; client_host?: string | null }): string {
  return [row.client_platform, row.client_os, row.client_host].filter(Boolean).join(' / ') || '-';
}

export function sourceIpLabel(row: { source_ip?: string | null }): string {
  return row.source_ip || '-';
}

export function trustFor(row: { attribution_trust?: AttributionTrust | null; trust?: AttributionTrust | null }, field: keyof AttributionTrust): string | null {
  return row.attribution_trust?.[field] ?? row.trust?.[field] ?? null;
}

export function firstTrust(row: { attribution_trust?: AttributionTrust | null; trust?: AttributionTrust | null }, fields: (keyof AttributionTrust)[]): string | null {
  for (const field of fields) {
    const value = trustFor(row, field);
    if (value) {
      return value;
    }
  }
  return null;
}

export function trustChip(source: string | null | undefined): ReactNode {
  return source ? <span className="trust-chip" title={`trust: ${source}`}>{source}</span> : null;
}

export function safeCallerContext(agent: AgentContext | null | undefined): Record<string, unknown> | null {
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

export function publicSafeCallerContext(agent: AgentContext | null | undefined): Record<string, unknown> | null {
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

export function tokenAccounting(row: TokenCarrier | null | undefined): TokenAccounting | null {
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

export function numericValue(value: number | string | null | undefined): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

export function formatTokenCount(value: number | string | null | undefined): string {
  const n = numericValue(value);
  if (n == null) {
    return '-';
  }
  return Math.round(n).toLocaleString();
}

export function formatSavingsPct(value: number | string | null | undefined): string {
  const n = numericValue(value);
  if (n == null) {
    return '-';
  }
  return `${n.toFixed(1)}%`;
}

export function responseFormatLabel(row: TokenCarrier | null | undefined): string {
  return tokenAccounting(row)?.response_format || '-';
}

export function returnedTokensLabel(row: TokenCarrier | null | undefined): string {
  return formatTokenCount(tokenAccounting(row)?.returned_tokens);
}

export function savedTokensLabel(row: TokenCarrier | null | undefined): string {
  const tokens = tokenAccounting(row);
  if (!tokens) {
    return '-';
  }
  return `${formatTokenCount(tokens.saved_tokens)} (${formatSavingsPct(tokens.savings_pct)})`;
}

export function formatDurationMs(value: number | null | undefined): string {
  if (value == null) {
    return '-';
  }
  if (value < 1_000) {
    return `${value} ms`;
  }
  return `${(value / 1_000).toFixed(2)} s`;
}

export function compactId(value: string | null | undefined): string {
  if (!value) {
    return '-';
  }
  return value.length > 12 ? value.slice(0, 12) : value;
}

export function trafficTimestamp(frame: TrafficFrameEnvelope): string | undefined {
  if (typeof frame.timestamp_ns === 'number') {
    return new Date(frame.timestamp_ns / 1_000_000).toISOString();
  }
  return undefined;
}

export function trafficMethod(frame: TrafficFrameEnvelope): string {
  return frame.attributes?.mcp?.method ?? '-';
}

export function trafficRequestId(frame: TrafficFrameEnvelope): string | undefined {
  return frame.correlation?.request_id;
}

export function trafficSessionId(frame: TrafficFrameEnvelope): string | undefined {
  return frame.attributes?.session_id ?? frame.correlation?.session_id;
}

export function trafficBodyBytes(frame: TrafficFrameEnvelope): number | undefined {
  return frame.attributes?.body?.size_bytes;
}

export function trafficRedactedPaths(frame: TrafficFrameEnvelope): string[] {
  return frame.attributes?.body?.redacted_paths ?? [];
}

export function trafficFrameDetail(frame: TrafficFrameEnvelope): string {
  return JSON.stringify(frame, null, 2);
}

export function trafficStatusLabelKey(status: TrafficCaptureStatus | null | undefined): MessageKey {
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

export function trafficStatusDetailKey(status: TrafficCaptureStatus | null | undefined): MessageKey {
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

export function trafficEmptyKey(status: TrafficCaptureStatus | null | undefined): MessageKey {
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

export function trafficStatusTone(status: TrafficCaptureStatus | null | undefined): 'ok' | 'warn' | 'err' | undefined {
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

export function compactList(values: string[] | null | undefined, empty = 'Any'): string {
  const clean = (values ?? []).filter(Boolean);
  if (!clean.length) {
    return empty;
  }
  if (clean.length <= 3) {
    return clean.join(', ');
  }
  return `${clean.slice(0, 3).join(', ')} +${clean.length - 3}`;
}

export function taskPrimaryRequestId(task: TaskRow): string | null {
  return task.correlation?.request_id ?? task.related?.request_ids?.[0] ?? null;
}

export function taskActorLabel(task: TaskRow): string {
  return task.correlation?.actor_name
    ?? task.correlation?.actor_id
    ?? task.correlation?.agent_id
    ?? task.correlation?.client_platform
    ?? '-';
}

export function taskWorkflowLabel(task: TaskRow): string {
  const workflows = task.related?.workflow_ids ?? [];
  if (workflows.length > 0) {
    return compactList(workflows.map(compactId), '-');
  }
  return compactId(task.correlation?.workflow_id);
}

export function taskRequestCount(task: TaskRow): number {
  return task.related?.request_ids?.length ?? (task.correlation?.request_id ? 1 : 0);
}

export function taskOutcomeText(task: TaskRow): string | null {
  return task.status && isErrStatus(task.status)
    ? task.failure_reason ?? task.final_result ?? task.summary ?? null
    : task.final_result ?? task.summary ?? task.goal ?? null;
}

export function gatewayLabel(health: HealthPayload | null): string {
  const current = health?.gateway?.current;
  if (!current) {
    return health?.status ?? '?';
  }
  const pid = current.pid ? ` pid ${current.pid}` : '';
  return `${current.name}${pid}`;
}

export function isProblemActivity(event: ActivityEvent): boolean {
  const text = haystack(event.status, event.severity, event.kind, event.message);
  return text.includes('err') || text.includes('fail') || text.includes('warn') || text.includes('timeout') || text.includes('stale');
}

export function MiniSparkline({ buckets, t }: { buckets: number[]; t: Translator }) {
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

export function StatBarList({ title, items, t }: { title: string; items: TopEntry[]; t: Translator }) {
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

export function AttributionFacetList({ title, items, t }: { title: string; items: AttributionFacet[]; t: Translator }) {
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

export function TokenBreakdownList({ title, items, t }: { title: string; items: TokenBreakdownEntry[]; t: Translator }) {
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

export function TokenAccountingDetail({ row, t }: { row: TokenCarrier | null | undefined; t: Translator }) {
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

export function HourlyChart({ buckets, t }: { buckets: number[]; t: Translator }) {
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

export function formatTraceDate(value: number | string | undefined): string {
  if (typeof value === 'number') {
    return new Date(value).toLocaleString();
  }
  if (typeof value === 'string' && value) {
    return new Date(value).toLocaleString();
  }
  return '-';
}

export function payloadPreview(payload: TracePayload | null | undefined, t: Translator): string {
  if (!payload) {
    return t('traces.payload.empty');
  }
  const suffix = payload.truncated ? `\n\n[${t('traces.payload.truncated', { size: formatBytes(payload.original_size) })}]` : '';
  return `${payload.content}${suffix}`;
}

export function buildAgentPacket(trace: TraceDetailPayload): string {
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

export function TraceLinks({ links, t }: { links: AdminLinks; t: Translator }) {
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

export function workflowAgentLabel(agent: WorkflowAgent | null | undefined): string {
  return agent?.agent_name || agent?.agent_id || agent?.agent_kind || agentModelLabel(agent) || 'unknown agent';
}

export function agentModelLabel(agent: Pick<AgentContext, 'model' | 'model_provider' | 'model_version'> | null | undefined): string {
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

export function workflowMeta(workflow: WorkflowRow): string {
  const parts = [
    workflow.group_kind,
    workflow.correlation.session_id ? `session ${compactId(workflow.correlation.session_id)}` : '',
    workflow.correlation.turn_id ? `turn ${compactId(workflow.correlation.turn_id)}` : '',
    workflow.correlation.trace_id ? `trace ${compactId(workflow.correlation.trace_id)}` : '',
    `${workflow.step_count} steps`,
  ];
  return parts.filter(Boolean).join(' · ');
}

export function agentTurnChips(agent: WorkflowAgent | AgentContext | null | undefined, t: Translator): string[] {
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

export function WorkflowSearchChips({ signal, t }: { signal: WorkflowSearchSignal | null | undefined; t: Translator }) {
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

export function workflowStageLabelKey(stage: WorkflowGraphStage): MessageKey {
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

export function workflowStepText(step: WorkflowStep): string {
  return [step.kind, step.title, step.tool ?? ''].join(' ').toLowerCase();
}

export function isEscapeHatchStep(step: WorkflowStep): boolean {
  const text = workflowStepText(step);
  return ['fallback', 'script', 'python', 'eval', 'execute_code', 'execute code', 'raw command'].some((needle) => text.includes(needle));
}

export function workflowStepStage(step: WorkflowStep): WorkflowGraphStage {
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

export function workflowNodeTone(node: WorkflowGraphNode): 'ok' | 'warn' | 'err' | 'muted' {
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

export function buildWorkflowGraphNodes(workflow: WorkflowRow): WorkflowGraphNode[] {
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

export function defaultWorkflowNodeId(nodes: WorkflowGraphNode[]): string {
  return nodes.find((node) => workflowNodeTone(node) === 'err')?.node_id
    ?? nodes.find((node) => node.escape_hatch)?.node_id
    ?? nodes.find((node) => workflowNodeTone(node) === 'warn')?.node_id
    ?? nodes[1]?.node_id
    ?? nodes[0]?.node_id
    ?? '';
}

export function workflowPrimaryRequestId(workflow: WorkflowRow): string | undefined {
  return [...workflow.steps]
    .reverse()
    .find((step) => step.request_id && (step.links || step.kind === 'call' || workflowStepStage(step) === 'toolCalls'))
    ?.request_id ?? workflow.correlation.request_ids?.at(-1);
}

export function workflowUniqueValues(workflow: WorkflowRow, field: 'dcc_type' | 'transport'): string[] {
  return Array.from(new Set(workflow.steps.map((step) => step[field]).filter(Boolean) as string[]));
}

export function workflowNodeRows(node: WorkflowGraphNode, workflow: WorkflowRow, t: Translator): [string, string][] {
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

export function WorkflowStageStrip({ workflow, t }: { workflow: WorkflowRow; t: Translator }) {
  const stages = workflow.steps.map(workflowStepStage);
  const uniqueStages = stages.filter((stage, index) => stages.indexOf(stage) === index).slice(0, 6);
  return (
    <div className="workflow-stage-strip" aria-label={t('workflows.label.stagePreview')}>
      {uniqueStages.map((stage) => <span key={stage}>{t(workflowStageLabelKey(stage))}</span>)}
      {stages.length > uniqueStages.length ? <span>{`+${stages.length - uniqueStages.length}`}</span> : null}
    </div>
  );
}

export function WorkflowStepCard({
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
              {t('action.trace')}
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

export function WorkflowGraphDetail({
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

export function WorkflowCard({
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
            {t('action.trace')}
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

export function TraceDetailPanel({
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

export function GovernanceControlCard({ control, t }: { control: GovernanceMiddlewareControl; t: Translator }) {
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

export function componentSchemaCount(spec: OpenApiSpec | null): number {
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

export function OpenApiInspectorPanel({
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

export function groupRows<T>(rows: T[], keyFn: (row: T) => string): Map<string, T[]> {
  const map = new Map<string, T[]>();
  for (const row of rows) {
    const key = keyFn(row);
    const bucket = map.get(key) ?? [];
    bucket.push(row);
    map.set(key, bucket);
  }
  return map;
}
