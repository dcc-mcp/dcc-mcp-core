import type { AdminLinks, OpenApiSource, Panel } from '../admin-types';
import type { MessageKey } from '../i18n';
import { API_BASE } from '../platform';

// ── Panel definitions ────────────────────────────────────────────────────────

export type PanelDefinition = { id: Panel; labelKey: MessageKey; groupKey: MessageKey };

export const PANELS: PanelDefinition[] = [
  { id: 'setup', labelKey: 'navigation.panel.setup', groupKey: 'navigation.group.onboarding' },
  { id: 'discover', labelKey: 'navigation.panel.discover', groupKey: 'navigation.group.discover' },
  { id: 'debug', labelKey: 'navigation.panel.debug', groupKey: 'navigation.group.operations' },
  { id: 'instances', labelKey: 'navigation.panel.instances', groupKey: 'navigation.group.operations' },
  { id: 'activity', labelKey: 'navigation.panel.activity', groupKey: 'navigation.group.operations' },
  { id: 'health', labelKey: 'navigation.panel.health', groupKey: 'navigation.group.operations' },
  { id: 'workflows', labelKey: 'navigation.panel.workflows', groupKey: 'navigation.group.workspace' },
  { id: 'tasks', labelKey: 'navigation.panel.tasks', groupKey: 'navigation.group.workspace' },
  { id: 'tools', labelKey: 'navigation.panel.tools', groupKey: 'navigation.group.workspace' },
  { id: 'openapi', labelKey: 'navigation.panel.openapi', groupKey: 'navigation.group.contracts' },
  { id: 'traces', labelKey: 'navigation.panel.traces', groupKey: 'navigation.group.observability' },
  { id: 'governance', labelKey: 'navigation.panel.governance', groupKey: 'navigation.group.observability' },
  { id: 'logs', labelKey: 'navigation.panel.logs', groupKey: 'navigation.group.observability' },
  { id: 'analytics', labelKey: 'navigation.panel.analytics', groupKey: 'navigation.group.insights' },
  { id: 'overview', labelKey: 'navigation.panel.overview', groupKey: 'navigation.group.insights' },
];

export const PANEL_ID_SET = new Set<Panel>(PANELS.map((p) => p.id));
export const STATS_RANGE_IDS = new Set(['1h', '24h', '7d', 'all']);

// ── URL alias map ─────────────────────────────────────────────────────────────

/**
 * Maps deprecated / legacy URL panel names to their current canonical Panel id.
 *
 * When `readPanelFromUrl()` encounters a raw panel value that is NOT a valid
 * `Panel` but IS a key in this map, it resolves to the canonical id and
 * replaces the browser history entry so bookmarked legacy URLs self-heal.
 *
 * New entries are additive — old Panel ids are never removed from the `Panel`
 * type until a major version bump; this map exists so that users who bookmarked
 * old names continue to land on the right panel without a 404.
 */
export const PANEL_ALIAS_MAP: Record<string, Panel> = {
  // Phase 1 merge (PIP-1458): consolidated top-level panels into sub-tabs.
  // Old deep links self-heal to their new parent panel.
  'skill-paths': 'discover',
  marketplace: 'discover',
  integrations: 'discover',
  stats: 'overview',
  traffic: 'overview',
  calls: 'traces',
};

export function isPanelId(value: string | null | undefined): value is Panel {
  return value != null && value !== '' && PANEL_ID_SET.has(value as Panel);
}

// ── Gateway docs / spec URLs ─────────────────────────────────────────────────

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

// ── Shell path ────────────────────────────────────────────────────────────────

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

// ── URL helpers ───────────────────────────────────────────────────────────────

export function hrefForAdmin(panel: Panel, extra?: Record<string, string | undefined>): string {
  const u = new URL(`${window.location.origin}${adminShellPath()}`);
  u.searchParams.set('panel', panel);
  if (extra) {
    for (const [k, v] of Object.entries(extra)) {
      if (v != null && v !== '') u.searchParams.set(k, v);
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
  if (!spec) return gatewayOpenApiSource();
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

// ── URL reader helpers ────────────────────────────────────────────────────────

export function readPanelFromUrl(): Panel {
  const u = new URL(window.location.href);
  const raw = u.searchParams.get('panel');

  // Fast path: valid known panel id.
  if (isPanelId(raw)) return raw;

  // Resolve legacy / deprecated panel names via the alias map.
  if (raw && raw in PANEL_ALIAS_MAP) {
    const resolved = PANEL_ALIAS_MAP[raw];
    // Self-heal the URL: redirect old name to canonical name via history replace.
    u.searchParams.set('panel', resolved);
    window.history.replaceState(null, '', `${u.pathname}${u.search}`);
    return resolved;
  }

  return 'setup';
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

/** Read the active sub-tab within the Discover panel from the URL. */
export function readDiscoverTabFromUrl(): string {
  const u = new URL(window.location.href);
  return u.searchParams.get('discoverTab')?.trim() || '';
}

/** Read the active sub-tab within the Overview panel from the URL. */
export function readOverviewTabFromUrl(): string {
  const u = new URL(window.location.href);
  return u.searchParams.get('overviewTab')?.trim() || '';
}

/** Read the active sub-tab within the Traces panel from the URL. */
export function readTracesTabFromUrl(): string {
  const u = new URL(window.location.href);
  return u.searchParams.get('tracesTab')?.trim() || '';
}
