import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  RiFileCopyLine,
  RiFolderOpenLine,
  RiRefreshLine,
} from '@remixicon/react';
import { LanguageSelector } from './components/LanguageSelector';
import { ThemeSelector } from './components/ThemeSelector';
import { BrandLogo } from './components/BrandLogo';
import { LogsPanel } from './components/LogsPanel';
import { PanelSearchBar } from './components/PanelSearchBar';
import { Button } from './components/ui/button';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './components/ui/select';
import { DiscoverPanel, type DiscoverTab } from './features/discover';
import { OverviewPanel, type OverviewTab } from './features/overview';
import { CommandCenterPanel } from './features/setup';
import { AnalyticsPanel } from './features/analytics/AnalyticsPanel';
import { DebugPanel } from './features/debug';
import { GovernancePanel } from './features/governance';
import { InstancesPanel, type InstanceUpdateNotice } from './features/instances';
import { MemoryPanel } from './features/agent-memory/MemoryPanel';
import { OpenApiPanel } from './features/openapi';
import { ActivityPanel } from './features/activity';
import { HealthPanel } from './features/health';
import { ReliabilityPanel } from './features/reliability';
import { SessionsPanel } from './features/sessions';
import { ToolsPanel } from './features/tools';
import { WorkflowsPanel } from './features/workflows';
import { TasksPanel } from './features/tasks';
import { TracesPanel } from './features/traces';
import { SessionsPanel } from './features/sessions';
import { ReliabilityPanel } from './features/reliability';
import { canonicalAdminPanelTarget, readDiscoverTabFromUrl, readOverviewTabFromUrl, readTracesTabFromUrl } from './navigation';
import { createTranslator, detectBrowserLocale, type SupportedLocale } from './i18n';
import { readLocaleOverride, storeLocaleOverride } from './locale';
import { applyTheme, readThemeMode, resolveTheme, storeThemeMode, type ThemeMode } from './theme';
import { filterLogs, isProblemLog, normalizeLogRow, summarizeLogSeverity, type LogSeverityFilter } from './logs';
import { CRITICAL_LATENCY_MS, SLOW_LATENCY_MS, type ClientPlatform, type DebugSignal, type FailureSignal, type IdeTarget, type InstanceRow, type InstanceSummary, type InstanceUpdatePayload, type OpenApiSource, type Panel, type SetupUrlMode, type TokenBreakdownEntry, type TraceDetailPayload } from './admin-types';
import { apiJson, backendAccessUrls, compactId, configPathFileUrl, configPathForTarget, detectClientPlatform, DocsIcon, downloadJsonText, flattenOpenApiOperations, gatewayMcpUrl, gatewayOpenApiSource, hrefForAdmin, IDE_TARGETS, ideConfigText, IdeIcon, instanceSetupLabel, isErrStatus, isOkStatus, isProblemActivity, isSlowLatency, issueReportFilename, issueReportJsonText, isWarnStatus, lanGatewayMcpUrl, matchesListFilter, NavIcon, NAVIGATION, openApiSpecFilename, PanelHeader, projectDocsHref, readOpenApiSourceFromUrl, readPanelFromUrl, readStatsRangeFromUrl, readTraceIdFromUrl, STATS_RANGE_IDS, statusClass, StatusLine, totalTraceTokens, traceLatency, haystack, trafficRequestId, trafficSessionId, trafficMethod, trafficRedactedPaths, agentLabel, formatDurationMs, formatTokenCount } from './admin-ui-core';
import {
  useActivityQuery,
  useCallsQuery,
  useGovernanceQuery,
  useHealthQuery,
  useInstanceServerUpdate,
  useLogsQuery,
  useOpenApiSpecQuery,
  useStatsQuery,
  useTasksQuery,
  useToolsQuery,
  useTraceDetailQuery,
  useTracesQuery,
  useTrafficQuery,
  useWorkersQuery,
  useWorkflowsQuery,
} from './hooks/queries';

function instanceUpdateTone(payload: InstanceUpdatePayload): InstanceUpdateNotice['tone'] {
  if (payload.status === 'staged' || payload.status === 'up_to_date') return 'ok';
  if (
    payload.status === 'available'
    || payload.status === 'binary_not_found'
    || payload.status === 'manifest_error'
    || payload.status === 'not_configured'
  ) {
    return 'warn';
  }
  return 'err';
}

function App() {
  const [localeOverride, setLocaleOverride] = useState<SupportedLocale | null>(() => readLocaleOverride());
  const [themeMode, setThemeMode] = useState<ThemeMode>(() => readThemeMode());
  const localeDetection = useMemo(() => detectBrowserLocale(localeOverride), [localeOverride]);
  const t = useMemo(() => createTranslator(localeDetection.locale), [localeDetection.locale]);
  const [activePanel, setActivePanel] = useState<Panel>(() => readPanelFromUrl());
  const [selectedWorkflowId, setSelectedWorkflowId] = useState<string | null>(null);
  const [statsRange, setStatsRange] = useState(() => readStatsRangeFromUrl());
  const [openApiSource, setOpenApiSource] = useState<OpenApiSource>(() => readOpenApiSourceFromUrl());
  const [setupUrlMode, setSetupUrlMode] = useState<SetupUrlMode>('local');
  const [clientPlatform] = useState<ClientPlatform>(() => detectClientPlatform());
  const [directInstanceId, setDirectInstanceId] = useState<string>('');
  const [logSeverityFilter, setLogSeverityFilter] = useState<LogSeverityFilter>('all');
  /// Filtered counts from the SkillsPanel for the cross-panel search-meta line.
  const [skillCounts, setSkillCounts] = useState({ skills: 0, paths: 0 });
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(() => {
    const panel = readPanelFromUrl();
    return panel === 'traces' ? readTraceIdFromUrl() : null;
  });
  const [discoverTab, setDiscoverTab] = useState<DiscoverTab>(() => {
    const tab = readDiscoverTabFromUrl();
    return (tab === 'skills' || tab === 'marketplace' || tab === 'integrations') ? tab : 'skills';
  });
  const [overviewTab, setOverviewTab] = useState<OverviewTab>(() => {
    const tab = readOverviewTabFromUrl();
    return (tab === 'stats' || tab === 'traffic') ? tab : 'stats';
  });
  const [tracesTab, setTracesTab] = useState<'traces' | 'calls'>(() => {
    const tab = readTracesTabFromUrl();
    return tab === 'calls' ? 'calls' : 'traces';
  });
  const [trafficDetail, setTrafficDetail] = useState<string>('Select a traffic frame row for detail.');
  const [callDetail, setCallDetail] = useState<string>('Select a call row for trace detail.');
  const [slowOnly, setSlowOnly] = useState(false);
  const [copiedNotice, setCopiedNotice] = useState<string>('');
  const [listSearch, setListSearch] = useState('');
  const [instanceUpdateNotices, setInstanceUpdateNotices] = useState<Record<string, InstanceUpdateNotice>>({});
  const [pendingInstanceUpdateId, setPendingInstanceUpdateId] = useState<string | null>(null);

  // ── data queries (TanStack Query) ──────────────────────────────────────
  // Each query is enabled only when its owning panel(s) are active.
  // Polling is driven by refetchInterval (5s), replacing the old setInterval.
  // ────────────────────────────────────────────────────────────────────────

  const isActive = useCallback((...panels: Panel[]) => panels.includes(activePanel), [activePanel]);

  // Panels that need each data source (mirrors the old fetchPanel dispatch):
  // setup→health+workers | debug→all | health→health | instances→workers
  // activity→activity | tools→tools | openapi→openApiSpec | workflows→workflows
  // tasks→tasks | calls→calls | traces→traces | traffic→traffic
  // stats→stats+calls+traces | governance→governance | logs→logs

  const healthQuery = useHealthQuery(isActive('health', 'debug', 'setup'));
  const workersQuery = useWorkersQuery(isActive('instances', 'debug', 'setup'));
  const activityQuery = useActivityQuery(isActive('activity', 'debug'));
  const toolsQuery = useToolsQuery(isActive('tools', 'debug'));
  const callsQuery = useCallsQuery(isActive('traces', 'debug', 'overview'));
  const tracesQuery = useTracesQuery(isActive('traces', 'debug', 'overview'));
  const trafficQuery = useTrafficQuery(isActive('overview', 'debug'));
  const tasksQuery = useTasksQuery(isActive('tasks', 'debug'));
  const workflowsQuery = useWorkflowsQuery(isActive('workflows', 'debug'));
  const statsQuery = useStatsQuery(isActive('overview', 'debug'), statsRange);
  const governanceQuery = useGovernanceQuery(isActive('governance', 'debug'));
  const logsQuery = useLogsQuery(isActive('logs', 'debug'));
  const openApiQuery = useOpenApiSpecQuery(openApiSource.specUrl, isActive('openapi'));
  const instanceUpdateMutation = useInstanceServerUpdate();

  // Derived data (with fallbacks matching the old useState initial values)
  const health = healthQuery.data ?? null;
  const activity = activityQuery.data ?? [];
  const tools = toolsQuery.data ?? [];
  const workflows = workflowsQuery.data ?? [];
  const tasks = tasksQuery.data ?? [];
  const calls = callsQuery.data ?? [];
  const traces = tracesQuery.data ?? [];
  const traffic = trafficQuery.data ?? null;
  const stats = statsQuery.data ?? null;
  const governance = governanceQuery.data ?? null;
  const logs = useMemo(() => (logsQuery.data?.logs ?? []).map(normalizeLogRow), [logsQuery.data]);
  const instanceRows = workersQuery.data?.workers ?? [];
  const instanceSummary: InstanceSummary = workersQuery.data?.summary ?? { live: 0, stale: 0, unhealthy: 0 };
  const openApiSpec = openApiQuery.data?.spec ?? null;
  const openApiRaw = openApiQuery.data?.raw ?? '';

  // On-demand trace detail (enabled only when a trace ID is selected)
  const traceDetailQuery = useTraceDetailQuery(selectedTraceId);
  const traceDetail = useMemo(() => {
    if (!selectedTraceId) return t('traces.empty.selectTrace');
    if (traceDetailQuery.isLoading) return t('common.status.loading');
    if (traceDetailQuery.error) return t('common.status.errorPrefix', { message: traceDetailQuery.error.message });
    if (traceDetailQuery.data != null) return JSON.stringify(traceDetailQuery.data, null, 2);
    return t('common.status.noData');
  }, [selectedTraceId, traceDetailQuery, t]);
  const traceDetailPayload: TraceDetailPayload | null = useMemo(() => {
    if (!selectedTraceId || traceDetailQuery.error || traceDetailQuery.isLoading) return null;
    return traceDetailQuery.data as TraceDetailPayload | null;
  }, [selectedTraceId, traceDetailQuery]);

  // ── derived status (replaces old markUpdated / markError) ──────────────

  type QueryMeta = { dataUpdatedAt: number; error: Error | null; isLoading: boolean };
  function queryMeta(q: QueryMeta): string {
    if (q.error) return t('common.status.errorPrefix', { message: q.error.message });
    if (q.isLoading) return t('common.status.loading');
    if (q.dataUpdatedAt > 0) {
      return new Date(q.dataUpdatedAt).toLocaleTimeString();
    }
    return t('common.status.waiting');
  }

  // SkillsPanel manages its own data; we keep lightweight status state for it
  const [skillPathsUpdatedAt, setSkillPathsUpdatedAt] = useState('');
  const [skillPathsError, setSkillPathsError] = useState<string | undefined>();
  const [highlightSkillName, setHighlightSkillName] = useState<string | null>(null);
  const [marketplaceCounts, setMarketplaceCounts] = useState({ total: 0, installed: 0 });
  const [marketplaceUpdatedAt, setMarketplaceUpdatedAt] = useState('');
  const [marketplaceError, setMarketplaceError] = useState<string | undefined>();
  const [integrationsCounts, setIntegrationsCounts] = useState({ total: 0, active: 0 });
  const [integrationsUpdatedAt, setIntegrationsUpdatedAt] = useState('');
  const [integrationsError, setIntegrationsError] = useState<string | undefined>();

  const updatedAt = useMemo<Record<string, string>>(() => {
    const qm = (q: QueryMeta) => queryMeta(q);
    return {
      setup: qm(healthQuery) || qm(workersQuery),
      debug: 'auto-refreshing',
      activity: qm(activityQuery),
      health: qm(healthQuery),
      instances: qm(workersQuery),
      tools: qm(toolsQuery),
      workflows: qm(workflowsQuery),
      tasks: qm(tasksQuery),
      openapi: qm(openApiQuery),
      traces: qm(tracesQuery),
      governance: qm(governanceQuery),
      logs: qm(logsQuery),
      analytics: '',
      discover: '',
      overview: '',
      sessions: '',
      reliability: '',
    };
  }, [healthQuery, workersQuery, activityQuery, toolsQuery, callsQuery, tracesQuery, trafficQuery, tasksQuery, workflowsQuery, statsQuery, governanceQuery, logsQuery, openApiQuery]);

  const errors = useMemo<Partial<Record<string, string>>>(() => {
    const errs: Partial<Record<string, string>> = {};
    const set = (panel: string, q: QueryMeta) => { if (q.error) errs[panel] = q.error.message; };
    set('health', healthQuery);
    set('instances', workersQuery);
    set('activity', activityQuery);
    set('tools', toolsQuery);
    set('traces', tracesQuery);
    set('tasks', tasksQuery);
    set('workflows', workflowsQuery);
    set('governance', governanceQuery);
    set('logs', logsQuery);
    set('openapi', openApiQuery);
    if (skillPathsError) errs['skill-paths'] = skillPathsError;
    if (marketplaceError) errs.marketplace = marketplaceError;
    if (integrationsError) errs.integrations = integrationsError;
    return errs;
  }, [healthQuery, workersQuery, activityQuery, toolsQuery, callsQuery, tracesQuery, trafficQuery, tasksQuery, workflowsQuery, statsQuery, governanceQuery, logsQuery, openApiQuery, skillPathsError, marketplaceError, integrationsError]);

  const panels = useMemo(
    () => NAVIGATION.map((panel) => ({ ...panel, label: t(panel.labelKey), group: t(panel.groupKey) })),
    [t],
  );

  const changeLocale = useCallback((locale: SupportedLocale) => {
    storeLocaleOverride(locale);
    setLocaleOverride(locale);
  }, []);

  const changeTheme = useCallback((mode: ThemeMode) => {
    storeThemeMode(mode);
    setThemeMode(mode);
  }, []);

  useEffect(() => {
    applyTheme(resolveTheme(themeMode));
    if (themeMode !== 'system' || typeof window.matchMedia !== 'function') {
      return;
    }
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => applyTheme(resolveTheme('system'));
    media.addEventListener('change', onChange);
    return () => media.removeEventListener('change', onChange);
  }, [themeMode]);

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
          w.server_version ?? '',
          w.adapter_version ?? '',
          w.instance_type ?? '',
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

  /// `filteredSkills` / `filteredSkillPaths` are owned by the SkillsPanel
  /// feature module now; the orchestrator forwards count updates back to
  /// the cross-panel search-meta line via `skillCounts`.

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

  const workflowSummary = useMemo(() => {
    const completed = workflows.filter((workflow) => isOkStatus(workflow.status)).length;
    const failed = workflows.filter((workflow) => isErrStatus(workflow.status)).length;
    const warning = workflows.filter((workflow) => isWarnStatus(workflow.status)).length;
    const zeroResults = workflows.filter((workflow) => workflow.discovery.zero_result_count > 0).length;
    const total = workflows.length;
    const settled = completed + failed;
    const successRate = settled > 0 ? (completed / settled) * 100 : 0;
    const searches = workflows.reduce((sum, workflow) => sum + (workflow.discovery.search_count ?? 0), 0);
    const totalSteps = workflows.reduce((sum, workflow) => sum + (workflow.step_count ?? 0), 0);
    const avgSteps = total > 0 ? totalSteps / total : 0;
    return { completed, failed, warning, zeroResults, total, successRate, searches, avgSteps };
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

  /// Headline token figures for the stats hero cards. Prefers the precise
  /// payload-token accounting when present and falls back to the aggregate
  /// stats / trace-derived totals so the hero never renders blank.
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
        panel: 'overview',
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

  const copyText = useCallback(async (text: string, label: string): Promise<boolean> => {
    if (!text) {
      return false;
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
        copied = document.execCommand('copy');
        document.body.removeChild(textarea);
      }
      if (!copied) {
        throw new Error('Clipboard write was not accepted by the browser.');
      }
      setCopiedNotice(t('common.notice.copied', { label }));
      window.setTimeout(() => setCopiedNotice(''), 1800);
      return true;
    } catch (error) {
      setCopiedNotice(t('common.notice.copyFailed', { message: error instanceof Error ? error.message : String(error) }));
      window.setTimeout(() => setCopiedNotice(''), 2400);
      return false;
    }
  }, [t]);

  const instanceUpdateMessage = useCallback((payload: InstanceUpdatePayload) => {
    const latest = payload.latest_version ?? '';
    const binary = payload.binary_name ?? 'dcc-mcp-server';
    const detail = payload.message || payload.error || '';
    switch (payload.status) {
      case 'staged':
        return t('instances.update.status.staged', { version: latest });
      case 'up_to_date':
        return t('instances.update.status.upToDate', { version: latest });
      case 'available':
        return t('instances.update.status.available', { version: latest });
      case 'binary_not_found':
        return t('instances.update.status.binaryNotFound', { binary });
      case 'manifest_error':
        return t('instances.update.status.manifestError', { message: detail });
      case 'not_configured':
        return t('instances.update.status.notConfigured');
      case 'download_failed':
        return t('instances.update.status.downloadFailed', { message: detail });
      case 'stage_failed':
        return t('instances.update.status.stageFailed', { message: detail });
      default:
        return detail || t('instances.update.status.failed');
    }
  }, [t]);

  const updateInstanceServer = useCallback(async (instance: InstanceRow) => {
    setPendingInstanceUpdateId(instance.instance_id);
    setInstanceUpdateNotices((prev) => ({
      ...prev,
      [instance.instance_id]: {
        tone: 'muted',
        message: t('instances.update.status.checking'),
      },
    }));
    try {
      const payload = await instanceUpdateMutation.mutateAsync({
        instanceId: instance.instance_id,
        apply: true,
      });
      setInstanceUpdateNotices((prev) => ({
        ...prev,
        [instance.instance_id]: {
          tone: instanceUpdateTone(payload),
          message: instanceUpdateMessage(payload),
          requiresRestart: payload.requires_restart === true || payload.status === 'staged',
        },
      }));
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      const friendlyMessage = /\b404\b/.test(message)
        ? t('instances.update.status.endpointUnavailable')
        : message;
      setInstanceUpdateNotices((prev) => ({
        ...prev,
        [instance.instance_id]: {
          tone: /\b404\b/.test(message) ? 'warn' : 'err',
          message: friendlyMessage || t('instances.update.status.failed'),
        },
      }));
    } finally {
      setPendingInstanceUpdateId((current) => (current === instance.instance_id ? null : current));
    }
  }, [instanceUpdateMessage, instanceUpdateMutation, t]);

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

  /// On-demand trace detail expand (one-off, not polled — uses apiJson directly).
  const expandTraceDetail = useCallback(async (requestId: string) => {
    try {
      const payload = await apiJson<unknown>(`/traces/${encodeURIComponent(requestId)}`);
      setCallDetail(JSON.stringify(payload, null, 2));
    } catch (error) {
      setCallDetail(t('common.status.errorPrefix', { message: error instanceof Error ? error.message : String(error) }));
    }
  }, []);

  // Combo-refresh wrappers for panels that pull multiple data sources
  const refreshSetup = useCallback(() => {
    healthQuery.refetch();
    workersQuery.refetch();
  }, [healthQuery, workersQuery]);

  const refreshDebug = useCallback(() => {
    healthQuery.refetch();
    workersQuery.refetch();
    activityQuery.refetch();
    callsQuery.refetch();
    tracesQuery.refetch();
    trafficQuery.refetch();
    statsQuery.refetch();
    governanceQuery.refetch();
    logsQuery.refetch();
  }, [healthQuery, workersQuery, activityQuery, callsQuery, tracesQuery, trafficQuery, statsQuery, governanceQuery, logsQuery]);

  const refreshStats = useCallback(() => {
    statsQuery.refetch();
    callsQuery.refetch();
    tracesQuery.refetch();
  }, [statsQuery, callsQuery, tracesQuery]);

  const pushAdminUrl = useCallback(
    (panel: Panel, opts?: { traceId?: string | null; range?: string | null; openApiSource?: OpenApiSource | null; replace?: boolean; discoverTab?: string | null; overviewTab?: string | null; tracesTab?: string | null }) => {
      const target = canonicalAdminPanelTarget(panel, {
        discoverTab: opts?.discoverTab ?? undefined,
        overviewTab: opts?.overviewTab ?? undefined,
        tracesTab: opts?.tracesTab ?? undefined,
      });
      const targetPanel = target.panel;
      const targetDiscoverTab = target.extra.discoverTab;
      const targetOverviewTab = target.extra.overviewTab;
      const targetTracesTab = target.extra.tracesTab;
      const u = new URL(window.location.href);
      u.searchParams.set('panel', targetPanel);
      u.searchParams.delete('range');
      u.searchParams.delete('trace');
      u.searchParams.delete('spec');
      u.searchParams.delete('docs');
      u.searchParams.delete('label');
      u.searchParams.delete('discoverTab');
      u.searchParams.delete('overviewTab');
      u.searchParams.delete('tracesTab');
      if (targetPanel === 'overview') {
        const r = opts?.range;
        if (r && STATS_RANGE_IDS.has(r)) {
          u.searchParams.set('range', r);
        }
      }
      if (targetPanel === 'traces') {
        if (opts?.traceId) u.searchParams.set('trace', opts.traceId);
        if (targetTracesTab) u.searchParams.set('tracesTab', targetTracesTab);
      }
      if (targetPanel === 'openapi' && opts?.openApiSource && opts.openApiSource.kind === 'instance') {
        u.searchParams.set('spec', opts.openApiSource.specUrl);
        u.searchParams.set('docs', opts.openApiSource.docsUrl);
        u.searchParams.set('label', opts.openApiSource.label);
      }
      if (targetPanel === 'discover' && targetDiscoverTab) {
        u.searchParams.set('discoverTab', targetDiscoverTab);
      }
      if (targetPanel === 'overview' && targetOverviewTab) {
        u.searchParams.set('overviewTab', targetOverviewTab);
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
    (panel: Panel, opts?: { traceId?: string; range?: string; openApiSource?: OpenApiSource; replace?: boolean; discoverTab?: string; overviewTab?: string; tracesTab?: string }) => {
      const target = canonicalAdminPanelTarget(panel, {
        discoverTab: opts?.discoverTab,
        overviewTab: opts?.overviewTab,
        tracesTab: opts?.tracesTab,
      });
      const targetPanel = target.panel;
      const targetDiscoverTab = target.extra.discoverTab;
      const targetOverviewTab = target.extra.overviewTab;
      const targetTracesTab = target.extra.tracesTab;
      let effectiveRange = statsRange;
      if (opts?.range && STATS_RANGE_IDS.has(opts.range)) {
        effectiveRange = opts.range;
        setStatsRange(opts.range);
      }
      if (targetPanel === 'openapi') {
        setOpenApiSource(opts?.openApiSource ?? gatewayOpenApiSource());
      }
      setActivePanel(targetPanel);
      if (targetDiscoverTab === 'skills' || targetDiscoverTab === 'marketplace' || targetDiscoverTab === 'integrations') {
        setDiscoverTab(targetDiscoverTab);
      }
      if (targetOverviewTab === 'stats' || targetOverviewTab === 'traffic') {
        setOverviewTab(targetOverviewTab);
      }
      if (targetTracesTab === 'traces' || targetTracesTab === 'calls') {
        setTracesTab(targetTracesTab);
      }
      pushAdminUrl(targetPanel, {
        traceId: opts?.traceId,
        range: targetPanel === 'overview' && (targetOverviewTab ?? overviewTab) === 'stats' ? effectiveRange : null,
        openApiSource: targetPanel === 'openapi' ? (opts?.openApiSource ?? gatewayOpenApiSource()) : null,
        replace: opts?.replace,
        discoverTab: targetDiscoverTab ?? null,
        overviewTab: targetOverviewTab ?? null,
        tracesTab: targetTracesTab ?? null,
      });
      if (opts?.traceId) {
        setSelectedTraceId(opts.traceId);
      } else if (targetPanel === 'traces') {
        setSelectedTraceId(null);
      }
    },
    [pushAdminUrl, statsRange, overviewTab],
  );

  /// Navigate to Discover→Skills tab and highlight a freshly installed skill.
  const handleNavigateToSkills = useCallback(
    (skillName: string) => {
      setHighlightSkillName(skillName);
      setDiscoverTab('skills');
      goToPanel('discover', { discoverTab: 'skills' });
    },
    [goToPanel],
  );

  useEffect(() => {
    const onPop = () => {
      const panel = readPanelFromUrl();
      setActivePanel(panel);
      setStatsRange(readStatsRangeFromUrl());
      setOpenApiSource(readOpenApiSourceFromUrl());
      const tid = readTraceIdFromUrl();
      if (panel === 'traces') {
        setSelectedTraceId(tid);
      }
      // Restore sub-tab states from URL
      const dt = readDiscoverTabFromUrl();
      if (dt === 'skills' || dt === 'marketplace' || dt === 'integrations') {
        setDiscoverTab(dt);
      }
      const ot = readOverviewTabFromUrl();
      if (ot === 'stats' || ot === 'traffic') {
        setOverviewTab(ot);
      }
      const tt = readTracesTabFromUrl();
      if (tt === 'traces' || tt === 'calls') {
        setTracesTab(tt);
      }
    };
    window.addEventListener('popstate', onPop);
    return () => window.removeEventListener('popstate', onPop);
  }, []);

  const hasLatencyFilter = activePanel === 'traces';
  const showListSearchMeta = Boolean(listSearch.trim()) || (hasLatencyFilter && slowOnly);
  const latencyThresholdDetail = t('common.detail.slowThreshold', {
    slow: formatDurationMs(SLOW_LATENCY_MS),
    tail: formatDurationMs(CRITICAL_LATENCY_MS),
  });
  const listSearchPlaceholder =
    activePanel === 'overview'
      ? t('search.input.stats')
      : activePanel === 'openapi'
        ? t('search.input.openapi')
        : t('search.input.default');
  const listSearchMeta = showListSearchMeta
    ? [
        activePanel === 'activity' ? `${filteredActivity.length} / ${activity.length}` : '',
        activePanel === 'instances' ? `${filteredInstanceRows.length} / ${instanceRows.length}` : '',
        activePanel === 'tools' ? `${filteredTools.length} / ${tools.length}` : '',
        activePanel === 'workflows' ? `${filteredWorkflows.length} / ${workflows.length}` : '',
        activePanel === 'openapi' ? `${filteredOpenApiOperations.length} / ${openApiOperations.length}` : '',
        activePanel === 'tasks' ? `${filteredTasks.length} / ${tasks.length}` : '',
        activePanel === 'traces' && tracesTab === 'traces' ? `${filteredTraces.length} / ${traces.length}` : '',
        activePanel === 'traces' && tracesTab === 'calls' ? `${filteredCalls.length} / ${calls.length}` : '',
        activePanel === 'governance' ? `${filteredGovernanceDecisions.length} / ${governance?.recent_decisions?.length ?? 0}` : '',
        activePanel === 'discover' && discoverTab === 'skills' ? t('search.meta.skillsPaths', { skills: skillCounts.skills, paths: skillCounts.paths }) : '',
        activePanel === 'discover' && discoverTab === 'marketplace' ? t('search.meta.marketplace', { total: marketplaceCounts.total }) : '',
        activePanel === 'discover' && discoverTab === 'integrations' ? t('integrations.detail.count', { count: integrationsCounts.total }) : '',
        activePanel === 'overview' && overviewTab === 'stats' ? t('search.meta.statsCharts', {
          apps: filteredTopAppTypes.length,
          tools: filteredTopTools.length,
          instances: filteredTopInstances.length,
          agents: filteredTopAgents.length,
          actors: filteredTopActors.length,
          platforms: filteredTopClientPlatforms.length,
          sources: filteredTopSourceIps.length,
          formats: filteredTokenByFormat.length,
        }) : '',
        activePanel === 'overview' && overviewTab === 'traffic' ? `${filteredTrafficFrames.length} / ${trafficFrames.length}` : '',
        activePanel === 'logs' ? `${filteredLogs.length} / ${logs.length}` : '',
        activePanel === 'governance' ? t('search.meta.governancePressure', { denied: governanceSummary.denied, throttled: governanceSummary.throttled }) : '',
      ].filter(Boolean).join(' ')
    : '';

  return (
    <div className="app-shell">
      <nav className="side-rail" aria-label={t('common.aria.adminNavigation')}>
        <div className="brand-lockup">
          <BrandLogo />
          <div className="brand-text">
            <h1>{t('chrome.app.title')}</h1>
            <p className="brand-tag">{t('chrome.app.subtitle')}</p>
          </div>
        </div>
        <div className="sidebar-preferences" aria-label={`${t('common.language.label')} / ${t('common.theme.label')}`}>
          <LanguageSelector
            locale={localeDetection.locale}
            source={localeDetection.source}
            onChange={changeLocale}
            t={t}
          />
          <ThemeSelector mode={themeMode} onChange={changeTheme} t={t} />
        </div>
        <div className="nav-links">
          {panels.map((panel, index) => {
            const showGroup = index === 0 || panels[index - 1].group !== panel.group;
            const isActive =
              panel.panel === activePanel
              && (!panel.discoverTab || (activePanel === 'discover' && discoverTab === panel.discoverTab))
              && (!panel.overviewTab || (activePanel === 'overview' && overviewTab === panel.overviewTab))
              && (!panel.tracesTab || (activePanel === 'traces' && tracesTab === panel.tracesTab));
            const href = hrefForAdmin(panel.panel, {
              discoverTab: panel.discoverTab,
              overviewTab: panel.overviewTab,
              tracesTab: panel.tracesTab,
              range: panel.panel === 'overview' ? statsRange : undefined,
            });
            return (
              <div className="nav-entry" key={panel.id}>
                {showGroup ? <div className="nav-section-title">{panel.group}</div> : null}
                <a
                  href={href}
                  className={isActive ? 'nav-link active' : 'nav-link'}
                  aria-current={isActive ? 'page' : undefined}
                  onClick={(e) => {
                    e.preventDefault();
                    goToPanel(panel.panel, {
                      discoverTab: panel.discoverTab,
                      overviewTab: panel.overviewTab,
                      tracesTab: panel.tracesTab,
                      range: panel.panel === 'overview' ? statsRange : undefined,
                    });
                  }}
                >
                  <NavIcon panel={panel.icon} />
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
          <PanelSearchBar
            panel={activePanel}
            discoverTab={discoverTab}
            placeholder={listSearchPlaceholder}
            value={listSearch}
            ariaLabel={t('search.input.ariaLabel')}
            meta={listSearchMeta}
            showLatencyFilter={hasLatencyFilter}
            slowOnly={slowOnly}
            slowLabel={t('common.filter.slowOnly')}
            allLabel={t('common.filter.allLatency')}
            latencyTitle={latencyThresholdDetail}
            onChange={setListSearch}
            onToggleLatency={() => setSlowOnly((value) => !value)}
          />
        )}
        {activePanel === 'setup' && (
          <section className="panel active setup-panel">
            <PanelHeader
              title={t('navigation.panel.setup')}
              meta={setupMcpUrl}
              action={(
                <Button type="button" size="sm" onClick={refreshSetup}>
                  <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
                  {t('action.refresh')}
                </Button>
              )}
            />
            <StatusLine text={copiedNotice || updatedAt.setup} error={errors.setup} />
            <CommandCenterPanel
              health={health}
              instanceSummary={instanceSummary}
              mcpUrl={setupMcpUrl}
              onCopy={copyText}
              onOpenInstances={() => goToPanel('instances')}
              onOpenMarketplace={() => goToPanel('discover', { discoverTab: 'marketplace' })}
              onOpenSkills={() => goToPanel('discover', { discoverTab: 'skills' })}
              t={t}
            />
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
                <Button
                  className="setup-inline-action"
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => copyText(setupMcpUrl, 'MCP URL')}
                >
                  <RiFileCopyLine data-icon="inline-start" aria-hidden="true" />
                  {t('action.copy')}
                </Button>
              </div>
              {setupUrlMode === 'direct' ? (
                <div className="setup-instance-picker">
                  <span id="setup-instance-picker-label">{t('common.table.instance')}</span>
                  <Select
                    value={selectedDirectInstance?.instance_id ?? ''}
                    disabled={directSetupInstanceRows.length === 0}
                    onValueChange={setDirectInstanceId}
                  >
                    <SelectTrigger
                      className="admin-select-trigger setup-instance-select-trigger"
                      id="setup-instance-picker"
                      size="sm"
                      aria-labelledby="setup-instance-picker-label"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent className="admin-select-content" position="popper" align="start">
                      <SelectGroup>
                        {directSetupInstanceRows.map((instance) => (
                          <SelectItem key={instance.instance_id} value={instance.instance_id}>
                            {instanceSetupLabel(instance)}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </div>
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
                      <Button
                        className="ide-card-action"
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => copyText(config, `${target.label} config`)}
                      >
                        <RiFileCopyLine data-icon="inline-start" aria-hidden="true" />
                        {t('common.action.copy')}
                      </Button>
                      <Button
                        className="ide-card-action"
                        type="button"
                        variant="secondary"
                        size="sm"
                        onClick={() => openConfigLocation(target, configPath)}
                      >
                        <RiFolderOpenLine data-icon="inline-start" aria-hidden="true" />
                        {t('common.action.openFile')}
                      </Button>
                    </div>
                  </article>
                );
              })}
            </div>
          </section>
        )}
        {activePanel === 'debug' && (
          <DebugPanel
            updatedAt={updatedAt.debug}
            error={errors.debug}
            debugIssues={debugIssues}
            health={health}
            unhealthyInstanceRows={unhealthyInstanceRows}
            instanceSummary={instanceSummary}
            stats={stats}
            tokenPressure={tokenPressure}
            slowLatencyDetail={slowLatencyDetail}
            debugSignals={debugSignals}
            tokenHeavyTraces={tokenHeavyTraces}
            failureSignals={failureSignals}
            slowTraces={slowTraces}
            instanceRows={instanceRows}
            problemLogs={problemLogs}
            problemActivity={problemActivity}
            onGoToPanel={goToPanel}
            onRefresh={refreshDebug}
            t={t}
          />
        )}
        {activePanel === 'activity' && (
          <ActivityPanel
            updatedAt={updatedAt.activity}
            error={errors.activity}
            activity={activity}
            filteredActivity={filteredActivity}
            onGoToPanel={goToPanel}
            onRefresh={() => activityQuery.refetch()}
            t={t}
          />
        )}

        {activePanel === 'health' && (
          <HealthPanel
            updatedAt={updatedAt.health}
            error={errors.health}
            health={health}
            onRefresh={() => healthQuery.refetch()}
            t={t}
          />
        )}

        <ReliabilityPanel
          active={activePanel === 'reliability'}
          t={t}
        />

        <SessionsPanel
          active={activePanel === 'sessions'}
          t={t}
        />

        {activePanel === 'instances' && (
          <InstancesPanel
            updatedAt={updatedAt.instances}
            error={errors.instances}
            instanceRows={instanceRows}
            filteredInstanceRows={filteredInstanceRows}
            instanceSummary={instanceSummary}
            instanceUpdateNotices={instanceUpdateNotices}
            pendingInstanceUpdateId={pendingInstanceUpdateId}
            onUpdateInstance={(instance) => void updateInstanceServer(instance)}
            onRefresh={() => workersQuery.refetch()}
            t={t}
          />
        )}
        {activePanel === 'tools' && (
          <ToolsPanel
            updatedAt={updatedAt.tools}
            error={errors.tools}
            tools={tools}
            filteredTools={filteredTools}
            onRefresh={() => toolsQuery.refetch()}
            t={t}
          />
        )}

        {activePanel === 'openapi' && (
          <OpenApiPanel
            source={openApiSource}
            spec={openApiSpec}
            raw={openApiRaw}
            operations={filteredOpenApiOperations}
            notice={copiedNotice}
            updatedAt={updatedAt.openapi}
            error={errors.openapi}
            onCopy={(text, label) => void copyText(text, label)}
            onDownload={() => {
              downloadJsonText(openApiSpecFilename(openApiSource.label), openApiRaw);
              setCopiedNotice(t('openapi.notice.downloadedSpec'));
              window.setTimeout(() => setCopiedNotice(''), 1800);
            }}
            onShowGatewaySpec={() => goToPanel('openapi', { replace: true })}
            onRefresh={() => openApiQuery.refetch()}
            t={t}
          />
        )}
        {activePanel === 'workflows' && (
          <WorkflowsPanel
            updatedAt={updatedAt.workflows}
            error={errors.workflows}
            workflows={workflows}
            filteredWorkflows={filteredWorkflows}
            selectedWorkflowId={selectedWorkflowId}
            onSelectWorkflowId={setSelectedWorkflowId}
            onGoToPanel={goToPanel}
            onCopyIssueReport={(requestId) => void copyIssueReport(requestId)}
            onRefresh={() => workflowsQuery.refetch()}
            copiedNotice={copiedNotice || ''}
            t={t}
          />
        )}

        {activePanel === 'tasks' && (
          <TasksPanel
            updatedAt={updatedAt.tasks}
            error={errors.tasks}
            tasks={tasks}
            filteredTasks={filteredTasks}
            onGoToPanel={goToPanel}
            onRefresh={() => tasksQuery.refetch()}
            t={t}
          />
        )}

        {activePanel === 'traces' && (
          <TracesPanel
            updatedAt={updatedAt.traces}
            error={errors.traces}
            traces={traces}
            filteredTraces={filteredTraces}
            calls={calls}
            filteredCalls={filteredCalls}
            tracesTab={tracesTab}
            onTracesTabChange={(tab) => goToPanel('traces', { tracesTab: tab, replace: true })}
            onSelectTraceId={(id) => goToPanel('traces', { traceId: id ?? undefined, tracesTab: 'traces', replace: true })}
            traceDetailPayload={traceDetailPayload}
            traceDetail={traceDetail}
            stats={stats}
            onCopyText={copyText}
            onCopyIssueReport={(requestId) => void copyIssueReport(requestId)}
            onDownloadIssueReport={(requestId) => void downloadIssueReport(requestId)}
            onTracesRefresh={() => tracesQuery.refetch()}
            onCallsRefresh={() => callsQuery.refetch()}
            copiedNotice={copiedNotice || ''}
            callDetail={callDetail}
            onExpandTraceDetail={(requestId) => void expandTraceDetail(requestId)}
            t={t}
          />
        )}

        {activePanel === 'overview' && (
          <OverviewPanel
            active={activePanel === 'overview'}
            overviewTab={overviewTab}
            onTabChange={(tab) => goToPanel('overview', { overviewTab: tab, replace: true })}
            stats={stats}
            statsRange={statsRange}
            onStatsRangeChange={(value) => {
              setStatsRange(value);
              pushAdminUrl('overview', { range: value, replace: true, overviewTab: 'stats' });
            }}
            onStatsRefresh={refreshStats}
            health={health}
            traces={traces}
            calls={calls}
            traffic={traffic}
            search={listSearch}
            trafficDetail={trafficDetail}
            onSetTrafficDetail={setTrafficDetail}
            onGoToPanel={goToPanel}
            onCopyText={copyText}
            onTrafficRefresh={() => trafficQuery.refetch()}
            copiedNotice={copiedNotice || ''}
            updatedAt={{ stats: updatedAt.stats, traffic: updatedAt.traffic }}
            errors={{ stats: errors.stats, traffic: errors.traffic }}
            t={t}
          />
        )}

        {activePanel === 'governance' && (
          <GovernancePanel
            governance={governance}
            governanceSummary={governanceSummary}
            filteredGovernanceDecisions={filteredGovernanceDecisions}
            updatedAt={updatedAt.governance}
            error={errors.governance}
            onRefresh={() => governanceQuery.refetch()}
            t={t}
          />
        )}
        <AnalyticsPanel
          active={activePanel === 'analytics'}
          locale={localeDetection.locale}
          t={t}
        />

        <MemoryPanel
          active={activePanel === 'memory'}
          t={t}
        />

        <DiscoverPanel
          active={activePanel === 'discover'}
          discoverTab={discoverTab}
          search={listSearch}
          onTabChange={setDiscoverTab}
          skillUpdatedAt={skillPathsUpdatedAt}
          skillError={skillPathsError}
          onSkillUpdated={setSkillPathsUpdatedAt}
          onSkillError={(err: unknown) => setSkillPathsError(err instanceof Error ? err.message : String(err))}
          onSkillCountsChange={setSkillCounts}
          highlightSkillName={highlightSkillName}
          onHighlightConsumed={() => setHighlightSkillName(null)}
          marketplaceUpdatedAt={marketplaceUpdatedAt}
          marketplaceError={marketplaceError}
          onMarketplaceUpdated={setMarketplaceUpdatedAt}
          onMarketplaceError={(err: unknown) => setMarketplaceError(err instanceof Error ? err.message : String(err))}
          onMarketplaceCountsChange={setMarketplaceCounts}
          coreVersion={health?.version ?? null}
          integrationsUpdatedAt={integrationsUpdatedAt}
          integrationsError={integrationsError}
          onIntegrationsUpdated={setIntegrationsUpdatedAt}
          onIntegrationsError={(err: unknown) => setIntegrationsError(err instanceof Error ? err.message : String(err))}
          onIntegrationsCountsChange={setIntegrationsCounts}
          onNavigateToSkills={handleNavigateToSkills}
          t={t}
        />

        {activePanel === 'logs' && (
          <LogsPanel
            logs={logs}
            filteredLogs={filteredLogs}
            serverVersion={logsQuery.data?.serverVersion ?? null}
            severityCounts={logSeverityCounts}
            severityFilter={logSeverityFilter}
            updatedAt={updatedAt.logs}
            error={errors.logs}
            onSeverityFilterChange={setLogSeverityFilter}
            onRefresh={() => logsQuery.refetch()}
            t={t}
          />
        )}

        {activePanel === 'sessions' && (
          <SessionsPanel
            active={activePanel === 'sessions'}
            updatedAt={updatedAt.sessions}
            error={errors.sessions}
            onRefresh={() => {}}
            t={t}
          />
        )}

        {activePanel === 'reliability' && (
          <ReliabilityPanel
            active={activePanel === 'reliability'}
            updatedAt={updatedAt.reliability}
            error={errors.reliability}
            onRefresh={() => {}}
            t={t}
          />
        )}
      </main>
    </div>
  );
}

export default App;
