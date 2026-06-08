/**
 * TanStack Query hooks for all admin API endpoints.
 *
 * Replaces the manual fetch + useState + useCallback + setInterval pattern
 * formerly in App.tsx. Each hook maps 1:1 to a backend endpoint.
 *
 * Polling is driven by `refetchInterval` — enabled only when a panel is active.
 * Inactive panels stay in cache (gcTime: 30s) for instant tab-switch.
 *
 * ## Usage
 *
 * ```tsx
 * const { data: health } = useHealthQuery(activePanel === 'health');
 * ```
 *
 * The `enabled` param gates both the initial fetch and the polling interval,
 * so only the visible panel's data refreshes every 5s.
 */

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ADMIN_FETCH_TIMEOUT_MS,
  API_BASE,
  apiJson,
  downloadJsonText,
  fetchOpenApiSpecText,
  issueReportFilename,
  issueReportJsonText,
} from '../admin-ui-core';
import type {
  ActivityEvent,
  AnalyticsHeatmapCell,
  AnalyticsOverview,
  AnalyticsTimeseriesPoint,
  CallRow,
  GovernancePayload,
  HealthPayload,
  InstanceRow,
  InstanceSummary,
  InstalledMarketplacePackage,
  MarketplaceEntry,
  MarketplaceErrorEnvelope,
  MarketplaceInstallResult,
  MarketplaceOutdatedPackage,
  MarketplaceSourceEntry,
  MarketplaceUninstallResult,
  MarketplaceUpdatePayload,
  IntegrationsPayload,
  UpdateIntegrationRequest,
  UpdateIntegrationResult,
  StatsPayload,
  TaskRow,
  ToolRow,
  TraceRow,
  TrafficPayload,
  WorkflowRow,
} from '../admin-types';

// ── query key factory ──────────────────────────────────────────────────────

export const adminKeys = {
  all: ['admin'] as const,
  activity: () => [...adminKeys.all, 'activity'] as const,
  health: () => [...adminKeys.all, 'health'] as const,
  workers: () => [...adminKeys.all, 'workers'] as const,
  tools: () => [...adminKeys.all, 'tools'] as const,
  calls: () => [...adminKeys.all, 'calls'] as const,
  traces: (limit?: number) => [...adminKeys.all, 'traces', { limit }] as const,
  traffic: (limit?: number) => [...adminKeys.all, 'traffic', { limit }] as const,
  tasks: (limit?: number) => [...adminKeys.all, 'tasks', { limit }] as const,
  workflows: (limit?: number) => [...adminKeys.all, 'workflows', { limit }] as const,
  stats: (range: string) => [...adminKeys.all, 'stats', { range }] as const,
  analyticsOverview: (range: string) => [...adminKeys.all, 'analytics', 'overview', { range }] as const,
  analyticsTimeseries: (range: string, granularity: string) => [...adminKeys.all, 'analytics', 'timeseries', { range, granularity }] as const,
  analyticsHeatmap: (range: string) => [...adminKeys.all, 'analytics', 'heatmap', { range }] as const,
  governance: (limit?: number) => [...adminKeys.all, 'governance', { limit }] as const,
  logs: () => [...adminKeys.all, 'logs'] as const,
  skills: () => [...adminKeys.all, 'skills'] as const,
  skillDetail: (name: string, dccType: string) => [...adminKeys.all, 'skill-detail', name, dccType] as const,
  skillPaths: () => [...adminKeys.all, 'skill-paths'] as const,
  traceDetail: (requestId: string) => [...adminKeys.all, 'trace-detail', requestId] as const,
  openApiSpec: (specUrl: string) => [...adminKeys.all, 'openapi-spec', specUrl] as const,
  marketplaceCatalog: () => [...adminKeys.all, 'marketplace', 'catalog'] as const,
  marketplaceInstalled: () => [...adminKeys.all, 'marketplace', 'installed'] as const,
  marketplaceSources: () => [...adminKeys.all, 'marketplace', 'sources'] as const,
  marketplaceOutdated: () => [...adminKeys.all, 'marketplace', 'outdated'] as const,
  integrations: () => [...adminKeys.all, 'integrations'] as const,
};

// ── polling config ─────────────────────────────────────────────────────────

/** Interval for active-panel polling (matches the old 5s setInterval). */
const POLL_INTERVAL_MS = 5_000;

// ── query hooks ────────────────────────────────────────────────────────────

export function useActivityQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.activity(),
    queryFn: () => apiJson<{ events: ActivityEvent[] }>('/activity?limit=300'),
    select: (payload) => (Array.isArray(payload.events) ? payload.events : []),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useHealthQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.health(),
    queryFn: () => apiJson<HealthPayload>('/health'),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useWorkersQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.workers(),
    queryFn: () => apiJson<{ workers: InstanceRow[]; summary: InstanceSummary }>('/workers'),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useToolsQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.tools(),
    queryFn: () => apiJson<{ tools: ToolRow[] }>('/tools'),
    select: (payload) => payload.tools,
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useCallsQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.calls(),
    queryFn: () => apiJson<{ calls: CallRow[] }>('/calls'),
    select: (payload) => (Array.isArray(payload.calls) ? payload.calls : []),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useTracesQuery(enabled: boolean, limit = 200) {
  return useQuery({
    queryKey: adminKeys.traces(limit),
    queryFn: () => apiJson<{ traces: TraceRow[] }>(`/traces?limit=${limit}`),
    select: (payload) => (Array.isArray(payload.traces) ? payload.traces : []),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useTrafficQuery(enabled: boolean, limit = 300) {
  return useQuery({
    queryKey: adminKeys.traffic(limit),
    queryFn: () => apiJson<TrafficPayload>(`/traffic?limit=${limit}`),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useTasksQuery(enabled: boolean, limit = 300) {
  return useQuery({
    queryKey: adminKeys.tasks(limit),
    queryFn: () => apiJson<{ tasks: TaskRow[] }>(`/tasks?limit=${limit}`),
    select: (payload) => (Array.isArray(payload.tasks) ? payload.tasks : []),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useWorkflowsQuery(enabled: boolean, limit = 200) {
  return useQuery({
    queryKey: adminKeys.workflows(limit),
    queryFn: () => apiJson<{ workflows: WorkflowRow[] }>(`/workflows?limit=${limit}`),
    select: (payload) => (Array.isArray(payload.workflows) ? payload.workflows : []),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useStatsQuery(enabled: boolean, range: string) {
  return useQuery({
    queryKey: adminKeys.stats(range),
    queryFn: () => apiJson<StatsPayload>(`/stats?range=${encodeURIComponent(range)}`),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useAnalyticsOverviewQuery(enabled: boolean, range: string) {
  return useQuery({
    queryKey: adminKeys.analyticsOverview(range),
    queryFn: () => apiJson<AnalyticsOverview>(`/analytics/overview?range=${encodeURIComponent(range)}`),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useAnalyticsTimeseriesQuery(enabled: boolean, range: string, granularity = 'day') {
  return useQuery({
    queryKey: adminKeys.analyticsTimeseries(range, granularity),
    queryFn: () => apiJson<{ series: AnalyticsTimeseriesPoint[] }>(`/analytics/timeseries?range=${encodeURIComponent(range)}&granularity=${encodeURIComponent(granularity)}`),
    select: (payload) => (Array.isArray(payload.series) ? payload.series : []),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useAnalyticsHeatmapQuery(enabled: boolean, range: string) {
  return useQuery({
    queryKey: adminKeys.analyticsHeatmap(range),
    queryFn: () => apiJson<{ heatmap: AnalyticsHeatmapCell[] }>(`/analytics/heatmap?range=${encodeURIComponent(range)}`),
    select: (payload) => (Array.isArray(payload.heatmap) ? payload.heatmap : []),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useGovernanceQuery(enabled: boolean, limit = 300) {
  return useQuery({
    queryKey: adminKeys.governance(limit),
    queryFn: () => apiJson<GovernancePayload>(`/governance?limit=${limit}`),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useLogsQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.logs(),
    queryFn: () => apiJson<{ logs?: unknown[] }>('/logs'),
    select: (payload) => (Array.isArray(payload.logs) ? payload.logs : []),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

/** On-demand: fetch a single trace waterfall. No polling. */
export function useTraceDetailQuery(requestId: string | null) {
  return useQuery({
    queryKey: adminKeys.traceDetail(requestId ?? ''),
    queryFn: () => apiJson<unknown>(`/traces/${encodeURIComponent(requestId!)}`),
    enabled: Boolean(requestId),
    staleTime: 10_000,
  });
}

/** On-demand: fetch OpenAPI spec. No polling. */
export function useOpenApiSpecQuery(specUrl: string, enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.openApiSpec(specUrl),
    queryFn: () => fetchOpenApiSpecText(specUrl),
    enabled: Boolean(specUrl) && enabled,
    staleTime: 60_000,
  });
}

// ── mutation hooks ─────────────────────────────────────────────────────────

export function useDownloadIssueReport() {
  return useMutation({
    mutationFn: async (requestId: string) => {
      const text = await issueReportJsonText(requestId);
      downloadJsonText(issueReportFilename(requestId), text);
    },
  });
}

export function useAddSkillPath() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (path: string) =>
      fetch(`${API_BASE}/skill-paths`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path }),
        signal: AbortSignal.timeout(ADMIN_FETCH_TIMEOUT_MS),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.skillPaths() });
      queryClient.invalidateQueries({ queryKey: adminKeys.skills() });
    },
  });
}

export function useDeleteSkillPath() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: number) =>
      fetch(`${API_BASE}/skill-paths/${id}`, {
        method: 'DELETE',
        signal: AbortSignal.timeout(ADMIN_FETCH_TIMEOUT_MS),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.skillPaths() });
      queryClient.invalidateQueries({ queryKey: adminKeys.skills() });
    },
  });
}

// ── marketplace query hooks ─────────────────────────────────────────────────

export function useMarketplaceCatalogQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.marketplaceCatalog(),
    queryFn: () => apiJson<{ entries: MarketplaceEntry[] }>('/marketplace/catalog'),
    select: (payload) => (Array.isArray(payload.entries) ? payload.entries : []),
    enabled,
    staleTime: 60_000,
  });
}

export function useInstalledMarketplaceQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.marketplaceInstalled(),
    queryFn: () => apiJson<{ packages: InstalledMarketplacePackage[] }>('/marketplace/installed'),
    select: (payload) => (Array.isArray(payload.packages) ? payload.packages : []),
    enabled,
    staleTime: 10_000,
  });
}

export function useMarketplaceSourcesQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.marketplaceSources(),
    queryFn: () => apiJson<{ sources: MarketplaceSourceEntry[] }>('/marketplace/sources'),
    select: (payload) => (Array.isArray(payload.sources) ? payload.sources : []),
    enabled,
    staleTime: 60_000,
  });
}

export function useMarketplaceOutdatedQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.marketplaceOutdated(),
    queryFn: () =>
      apiJson<{ dcc?: string | null; count: number; packages: MarketplaceOutdatedPackage[] }>(
        '/marketplace/outdated',
      ),
    select: (payload) => ({
      dcc: payload.dcc ?? null,
      count: payload.count ?? 0,
      packages: Array.isArray(payload.packages) ? payload.packages : [],
    }),
    enabled,
    staleTime: 15_000,
  });
}

// ── marketplace error helpers ───────────────────────────────────────────────

/** Try to parse a fetch error response into a structured error envelope. */
async function readMarketplaceErrorEnvelope(
  res: Response,
): Promise<MarketplaceErrorEnvelope | null> {
  try {
    const body = await res.json();
    const err = (body as Record<string, unknown>)?.error;
    if (err && typeof err === 'object' && (err as MarketplaceErrorEnvelope).kind) {
      return err as MarketplaceErrorEnvelope;
    }
  } catch {
    // ignore parse errors
  }
  return null;
}

/** Build a structured error from a failed fetch response. */
async function buildMarketplaceError(
  res: Response,
  fallback: string,
): Promise<MarketplaceError> {
  const envelope = await readMarketplaceErrorEnvelope(res);
  return new MarketplaceError(
    envelope?.kind ?? 'internal_error',
    envelope?.message ?? `${fallback}: ${res.status}`,
  );
}

/** Structured marketplace error carrying the backend error kind for UI mapping. */
export class MarketplaceError extends Error {
  kind: MarketplaceErrorEnvelope['kind'];
  constructor(kind: MarketplaceErrorEnvelope['kind'], message: string) {
    super(message);
    this.name = 'MarketplaceError';
    this.kind = kind;
  }
}

// ── marketplace mutations ───────────────────────────────────────────────────

export function useMarketplaceInstall() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (body: { name: string; dcc: string; source?: string; force?: boolean }) => {
      const res = await fetch(`${API_BASE}/marketplace/install`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(ADMIN_FETCH_TIMEOUT_MS),
      });
      if (!res.ok) throw await buildMarketplaceError(res, 'Install failed');
      return res.json() as Promise<MarketplaceInstallResult>;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.marketplaceInstalled() });
      queryClient.invalidateQueries({ queryKey: adminKeys.marketplaceCatalog() });
    },
  });
}

export function useMarketplaceUninstall() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (body: { name: string; dcc: string }) => {
      const res = await fetch(`${API_BASE}/marketplace/uninstall`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(ADMIN_FETCH_TIMEOUT_MS),
      });
      if (!res.ok) throw await buildMarketplaceError(res, 'Uninstall failed');
      return res.json() as Promise<MarketplaceUninstallResult>;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.marketplaceInstalled() });
      queryClient.invalidateQueries({ queryKey: adminKeys.marketplaceCatalog() });
    },
  });
}

export function useAddMarketplaceSource() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (source: string) => {
      const res = await fetch(`${API_BASE}/marketplace/sources`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ source }),
        signal: AbortSignal.timeout(ADMIN_FETCH_TIMEOUT_MS),
      });
      if (!res.ok) throw await buildMarketplaceError(res, 'Failed to add source');
      return res.json() as Promise<{ sources: MarketplaceSourceEntry[] }>;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.marketplaceSources() });
      queryClient.invalidateQueries({ queryKey: adminKeys.marketplaceCatalog() });
    },
  });
}

export function useMarketplaceUpdate() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (body: { name?: string; dcc?: string; all?: boolean }) => {
      const res = await fetch(`${API_BASE}/marketplace/update`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(ADMIN_FETCH_TIMEOUT_MS),
      });
      if (!res.ok) throw await buildMarketplaceError(res, 'Update failed');
      return res.json() as Promise<MarketplaceUpdatePayload>;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.marketplaceInstalled() });
      queryClient.invalidateQueries({ queryKey: adminKeys.marketplaceOutdated() });
      queryClient.invalidateQueries({ queryKey: adminKeys.marketplaceCatalog() });
    },
  });
}

// ── integrations query hooks ─────────────────────────────────────────────────

export function useIntegrationsQuery(enabled: boolean) {
  return useQuery({
    queryKey: adminKeys.integrations(),
    queryFn: () => apiJson<IntegrationsPayload>('/integrations'),
    select: (payload) => (Array.isArray(payload.integrations) ? payload.integrations : []),
    enabled,
    refetchInterval: enabled ? POLL_INTERVAL_MS : false,
  });
}

export function useUpdateIntegration() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: UpdateIntegrationRequest) =>
      fetch(`${API_BASE}/integrations`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(ADMIN_FETCH_TIMEOUT_MS),
      }).then(async (res) => {
        if (!res.ok) {
          const text = await res.text().catch(() => '');
          throw new Error(text || `Update failed: ${res.status}`);
        }
        return res.json() as Promise<UpdateIntegrationResult>;
      }),
    onSuccess: (data) => {
      queryClient.setQueryData<IntegrationsPayload>(
        adminKeys.integrations(),
        (prev) => {
          const list = prev?.integrations ?? [];
          return {
            integrations: list.map((entry) =>
              entry.kind === data.kind ? data : entry,
            ),
          };
        },
      );
    },
  });
}
