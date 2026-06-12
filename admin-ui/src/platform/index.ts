import type {
  ClientPlatform,
  GatewaySentinel,
  HealthPayload,
  IdeTarget,
  InstanceRow,
  OpenApiOperationRow,
  OpenApiSource,
  OpenApiSpec,
} from '../admin-types';

import mayaIcon from '../assets/icons/autodesk.svg';
import blenderIcon from '../assets/icons/blender.svg';
import gimpIcon from '../assets/icons/gimp.svg';
import inkscapeIcon from '../assets/icons/inkscape.svg';
import kritaIcon from '../assets/icons/krita.svg';
import unityIcon from '../assets/icons/unity.svg';
import unrealIcon from '../assets/icons/unrealengine.svg';
import substancePainterIcon from '../assets/icons/photoshop.svg';
import puzzleIcon from '../assets/icons/puzzle.svg';
import claudeIcon from '../assets/icons/claude.svg';
import clineIcon from '../assets/icons/cline.svg';
import codebuddyIcon from '../assets/icons/codebuddy.svg';
import cursorIcon from '../assets/icons/cursor.svg';
import openaiIcon from '../assets/icons/openai.svg';
import vscodeIcon from '../assets/icons/vscode.svg';

// ── DCC icons ────────────────────────────────────────────────────────────────

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

export function resolveDccIcon(dccType: string): string {
  const key = dccType.toLowerCase();
  if (DCC_ICON_MAP[key]) return DCC_ICON_MAP[key];
  for (const [k, url] of Object.entries(DCC_ICON_MAP)) {
    if (key.includes(k)) return url;
  }
  return DCC_ICON_FALLBACK;
}

// ── API base ──────────────────────────────────────────────────────────────────

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
export const ADMIN_FETCH_TIMEOUT_MS = 25_000;
export const DEFAULT_LOCAL_GATEWAY_PORT = '9765';

function responsePreview(text: string): string {
  return text.replace(/\s+/g, ' ').trim().slice(0, 120);
}

function looksLikeHtml(text: string, contentType: string): boolean {
  return contentType.toLowerCase().includes('text/html')
    || /^<!doctype\b/i.test(text.trim())
    || /^<html\b/i.test(text.trim());
}

function recordOrNull(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' ? value as Record<string, unknown> : null;
}

function payloadErrorMessage(payload: unknown): string | null {
  const root = recordOrNull(payload);
  if (!root) return null;
  if (typeof root.message === 'string' && root.message.trim()) {
    return root.message.trim();
  }
  if (typeof root.error === 'string' && root.error.trim()) {
    return root.error.trim();
  }
  const nested = recordOrNull(root.error);
  if (typeof nested?.message === 'string' && nested.message.trim()) {
    return nested.message.trim();
  }
  return null;
}

export class AdminApiError extends Error {
  status: number;
  statusText: string;
  endpoint: string;
  payload: unknown;
  preview: string;
  requestUrl: string;

  constructor(
    response: Response,
    endpoint: string,
    message: string,
    payload: unknown,
    preview: string,
  ) {
    super(message);
    this.name = 'AdminApiError';
    this.status = response.status;
    this.statusText = response.statusText;
    this.endpoint = endpoint;
    this.payload = payload;
    this.preview = preview;
    this.requestUrl = response.url;
  }
}

function htmlFallbackMessage(response: Response, endpoint: string): string {
  const requested = response.url ? ` (requested ${response.url})` : '';
  return `Admin API returned HTML for ${endpoint}${requested}; check that /admin/api is served by the gateway or dev mock.`;
}

export async function adminJsonResponse<T>(response: Response, endpoint: string): Promise<T> {
  const contentType = response.headers.get('content-type') ?? '';
  const text = await response.text();
  const preview = responsePreview(text);

  if (looksLikeHtml(text, contentType)) {
    throw new AdminApiError(
      response,
      endpoint,
      htmlFallbackMessage(response, endpoint),
      undefined,
      preview,
    );
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    if (!response.ok) {
      throw new AdminApiError(
        response,
        endpoint,
        `${response.status} ${response.statusText}${preview ? `: ${preview}` : ''}`,
        undefined,
        preview,
      );
    }
    throw new Error(`Invalid JSON from Admin API ${endpoint}: ${message}${preview ? ` (${preview})` : ''}`);
  }

  if (!response.ok) {
    const payloadMessage = payloadErrorMessage(parsed);
    throw new AdminApiError(
      response,
      endpoint,
      `${response.status} ${response.statusText}${payloadMessage ? `: ${payloadMessage}` : preview ? `: ${preview}` : ''}`,
      parsed,
      preview,
    );
  }

  return parsed as T;
}

export async function adminOkResponse(response: Response, endpoint: string): Promise<void> {
  const contentType = response.headers.get('content-type') ?? '';
  const text = await response.text();
  const preview = responsePreview(text);

  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}${preview ? `: ${preview}` : ''}`);
  }

  if (looksLikeHtml(text, contentType)) {
    throw new Error(htmlFallbackMessage(response, endpoint));
  }

  if (!text.trim()) {
    return;
  }

  try {
    JSON.parse(text);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    throw new Error(`Invalid JSON from Admin API ${endpoint}: ${message}${preview ? ` (${preview})` : ''}`);
  }
}

// ── IDE targets ───────────────────────────────────────────────────────────────

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

// ── Networking helpers ────────────────────────────────────────────────────────

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

// ── Gateway MCP URL helpers ──────────────────────────────────────────────────

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

// ── Platform detection ────────────────────────────────────────────────────────

export function detectClientPlatform(): ClientPlatform {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const primaryPlatform = `${nav.userAgentData?.platform ?? navigator.platform ?? ''}`.toLowerCase();
  if (primaryPlatform.includes('win')) return 'windows';
  if (primaryPlatform.includes('mac')) return 'macos';
  if (primaryPlatform.includes('linux') || primaryPlatform.includes('x11')) return 'linux';

  const userAgent = `${navigator.userAgent ?? ''}`.toLowerCase();
  if (userAgent.includes('win')) return 'windows';
  if (userAgent.includes('mac')) return 'macos';
  return 'linux';
}

export function configPathForTarget(target: IdeTarget, platform: ClientPlatform): string {
  if (typeof target.configPath === 'string') return target.configPath;
  return target.configPath[platform] ?? target.configPath.linux;
}

export function ideConfigText(target: IdeTarget, url: string): string {
  return target.build(url);
}

export function configPathFileUrl(path: string): string | null {
  if (path.startsWith('%') || path.startsWith('~') || path.includes('->')) return null;
  const normalized = path.replace(/\\/g, '/');
  return normalized.match(/^[A-Za-z]:\//) ? `file:///${normalized}` : null;
}

// ── Instance helpers ──────────────────────────────────────────────────────────

export function instanceSetupLabel(instance: InstanceRow): string {
  return `${instance.display_name || instance.dcc_type} (${instance.instance_id.slice(0, 8)})`;
}

export function instanceOpenApiSource(instance: InstanceRow): OpenApiSource {
  const urls = backendAccessUrls(instance.mcp_url);
  const label = `${instance.display_name || instance.dcc_type} ${instance.instance_id.slice(0, 8)}`;
  return {
    label,
    specUrl: urls.openapi,
    docsUrl: urls.docs,
    inspectorUrl: new URL(
      `${window.location.origin}${window.location.pathname}?panel=openapi&spec=${encodeURIComponent(urls.openapi)}&docs=${encodeURIComponent(urls.docs)}&label=${encodeURIComponent(label)}`,
    ).toString(),
    kind: 'instance',
  };
}

// ── API fetch ─────────────────────────────────────────────────────────────────

export async function apiJson<T>(path: string): Promise<T> {
  const ctrl = new AbortController();
  const tid = window.setTimeout(() => ctrl.abort(), ADMIN_FETCH_TIMEOUT_MS);
  try {
    return await adminJsonResponse<T>(
      await fetch(`${API_BASE}${path}`, { signal: ctrl.signal }),
      path,
    );
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') {
      throw new Error(`Request timed out after ${ADMIN_FETCH_TIMEOUT_MS / 1000}s`);
    }
    throw err;
  } finally {
    clearTimeout(tid);
  }
}

// ── Issue report helpers ──────────────────────────────────────────────────────

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

// ── OpenAPI fetch ─────────────────────────────────────────────────────────────

export const OPENAPI_METHODS = new Set(['get', 'put', 'post', 'delete', 'patch', 'options', 'head', 'trace']);

export async function fetchOpenApiSpecText(specUrl: string): Promise<{ spec: OpenApiSpec; raw: string }> {
  const ctrl = new AbortController();
  const tid = window.setTimeout(() => ctrl.abort(), ADMIN_FETCH_TIMEOUT_MS);
  try {
    const response = await fetch(specUrl, { signal: ctrl.signal });
    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
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
    if (!rawPathItem || typeof rawPathItem !== 'object' || Array.isArray(rawPathItem)) continue;
    for (const [method, rawOperation] of Object.entries(rawPathItem as Record<string, unknown>)) {
      const methodKey = method.toLowerCase();
      if (!OPENAPI_METHODS.has(methodKey) || !rawOperation || typeof rawOperation !== 'object' || Array.isArray(rawOperation)) continue;
      const op = rawOperation as any;
      const responseCodes = Object.keys(op.responses ?? {});
      const tags = Array.isArray(op.tags) ? op.tags.filter((t: unknown): t is string => typeof t === 'string') : [];
      const operationId = op.operationId ?? `${methodKey}_${path.replace(/[^A-Za-z0-9]+/g, '_').replace(/^_+|_+$/g, '')}`;
      rows.push({
        key: `${methodKey.toUpperCase()} ${path}`,
        method: methodKey.toUpperCase(),
        path,
        operationId,
        summary: op.summary ?? op.description ?? '',
        tags,
        responseCodes,
        hasRequestBody: op.requestBody != null,
        parameterCount: Array.isArray(op.parameters) ? op.parameters.length : 0,
      });
    }
  }
  return rows.sort((a, b) => a.path.localeCompare(b.path) || a.method.localeCompare(b.method));
}
