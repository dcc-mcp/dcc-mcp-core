import { useCallback, useEffect, useMemo, useState } from 'react';
import { LanguageSelector } from './components/LanguageSelector';
import { ThemeSelector } from './components/ThemeSelector';
import { LogsPanel } from './components/LogsPanel';
import { SkillsPanel } from './features/skills';
import { createTranslator, detectBrowserLocale, type SupportedLocale } from './i18n';
import { readLocaleOverride, storeLocaleOverride } from './locale';
import { applyTheme, readThemeMode, resolveTheme, storeThemeMode, type ThemeMode } from './theme';
import { filterLogs, isProblemLog, normalizeLogRow, summarizeLogSeverity, type LogRow, type LogSeverityFilter } from './logs';
import { CRITICAL_LATENCY_MS, SLOW_LATENCY_MS, type ActivityEvent, type CallRow, type ClientPlatform, type DebugSignal, type FailureSignal, type GovernancePayload, type HealthPayload, type IdeTarget, type InstanceRow, type InstanceSummary, type OpenApiSource, type OpenApiSpec, type Panel, type SetupUrlMode, type StatsPayload, type TaskRow, type TokenBreakdownEntry, type ToolRow, type TraceDetailPayload, type TraceRow, type TrafficPayload, type WorkflowRow } from './admin-types';
import { actorLabel, agentLabel, apiJson, API_BASE, AttributionFacetList, BackendAccessUrl, backendAccessUrls, BackendOpenApiLinks, callGroupLabel, compactId, compactInstanceId, compactList, configPathFileUrl, configPathForTarget, detectClientPlatform, DocsIcon, downloadJsonText, EmptyRow, errorRateTone, fetchOpenApiSpecText, firstTrust, flattenOpenApiOperations, formatBytes, formatDurationMs, formatSavingsPct, formatTokenCount, formatTraceDate, formatUptime, gatewayLabel, gatewayMcpUrl, gatewayOpenApiSource, GovernanceControlCard, groupRows, haystack, HealthCard, HeroMetric, HourlyChart, hrefForAdmin, IDE_TARGETS, ideConfigText, IdeIcon, instanceGroupLabel, instanceSetupLabel, isErrStatus, isOkStatus, isProblemActivity, isSlowLatency, issueReportFilename, issueReportJsonText, isWarnStatus, lanGatewayMcpUrl, latencyClass, latencyTone, LatencyValue, matchesListFilter, McpBackendLinks, MetricTile, MiniSparkline, NavIcon, OpenApiInspectorPanel, openApiSpecFilename, PanelHeader, PANELS, platformLabel, projectDocsHref, readOpenApiSourceFromUrl, readPanelFromUrl, readStatsRangeFromUrl, readTraceIdFromUrl, resolveDccIcon, responseFormatLabel, returnedTokensLabel, savedTokensLabel, sourceIpLabel, StatBarList, STATS_RANGE_IDS, StatusBadge, statusClass, StatusLine, taskActorLabel, taskOutcomeText, taskPrimaryRequestId, taskRequestCount, taskWorkflowLabel, TimeValue, TokenBreakdownList, toolGroupLabel, toolInstanceLabel, totalTraceTokens, TraceDetailPanel, traceGroupLabel, traceLatency, traceLinks, trafficBodyBytes, trafficEmptyKey, trafficFrameDetail, trafficMethod, trafficRedactedPaths, trafficRequestId, trafficSessionId, trafficStatusDetailKey, trafficStatusLabelKey, trafficStatusTone, trafficTimestamp, trustChip, trustFor, WorkflowCard, WorkflowGraphDetail } from './admin-ui-core';

function App() {
  const [localeOverride, setLocaleOverride] = useState<SupportedLocale | null>(() => readLocaleOverride());
  const [themeMode, setThemeMode] = useState<ThemeMode>(() => readThemeMode());
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
  /// Filtered counts from the SkillsPanel for the cross-panel search-meta line.
  const [skillCounts, setSkillCounts] = useState({ skills: 0, paths: 0 });
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
    const total = tasks.length;
    const settled = completed + failed;
    const successRate = settled > 0 ? (completed / settled) * 100 : 0;
    const durations = tasks.map((task) => task.duration_ms).filter((ms): ms is number => typeof ms === 'number' && ms >= 0);
    const avgDurationMs = durations.length > 0 ? durations.reduce((sum, ms) => sum + ms, 0) / durations.length : null;
    return { completed, failed, active, total, successRate, avgDurationMs };
  }, [tasks]);

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

  /// Headline token figures for the stats hero cards. Prefers the precise
  /// payload-token accounting when present and falls back to the aggregate
  /// stats / trace-derived totals so the hero never renders blank.
  const heroTokens = useMemo(() => {
    const payload = stats?.payload_token_usage;
    const input = payload?.total_input_tokens ?? stats?.total_input_tokens ?? statsSummary.totalInputTokens ?? 0;
    const output = payload?.total_output_tokens ?? stats?.total_output_tokens ?? statsSummary.totalOutputTokens ?? 0;
    const total = payload?.total_tokens
      ?? stats?.total_tokens
      ?? ((input || output) ? input + output : statsSummary.totalTokens)
      ?? 0;
    return {
      total,
      input,
      output,
      avg: payload?.avg_total_tokens_per_call ?? stats?.avg_tokens_per_call ?? stats?.avg_total_tokens_per_call ?? statsSummary.avgTokens ?? 0,
      saved: stats?.token_usage?.total_saved_tokens ?? 0,
      savedPct: stats?.token_usage?.average_savings_pct ?? 0,
      estimator: payload?.token_estimator ?? stats?.payload_token_estimator ?? health?.response_format?.token_estimator ?? '-',
    };
  }, [health, stats, statsSummary]);

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

  /// Skills / skill-paths fetching lives inside features/skills/.



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
    // `skill-paths` refreshes itself from the SkillsPanel orchestrator.
    if (panel === 'logs') void fetchLogs();
  }, [fetchActivity, fetchCalls, fetchDebug, fetchGovernance, fetchHealth, fetchInstanceBackends, fetchLogs, fetchOpenApi, fetchSetup, fetchStats, fetchTasks, fetchTools, fetchTraces, fetchTraffic, fetchWorkflows]);

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
        <ThemeSelector mode={themeMode} onChange={changeTheme} t={t} />
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
                {activePanel === 'skill-paths' ? t('search.meta.skillsPaths', { skills: skillCounts.skills, paths: skillCounts.paths }) : ''}
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
              action={<button className="refresh-btn" type="button" onClick={fetchSetup}>{t('action.refresh')}</button>}
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
                  {t('action.copy')}
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
            <button className="refresh-btn" type="button" onClick={fetchActivity}>{t('action.refresh')}</button>
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
            <button className="refresh-btn" type="button" onClick={fetchHealth}>{t('action.refresh')}</button>
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
            <button className="refresh-btn" type="button" onClick={fetchInstanceBackends}>{t('action.refresh')}</button>
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
            <button className="refresh-btn" type="button" onClick={fetchTools}>{t('action.refresh')}</button>
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
                  <button className="refresh-btn" type="button" onClick={fetchOpenApi}>{t('action.refresh')}</button>
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
              action={<button className="refresh-btn" type="button" onClick={fetchWorkflows}>{t('action.refresh')}</button>}
            />
            <StatusLine text={copiedNotice || updatedAt.workflows} error={errors.workflows} />
            <div className="metric-grid compact">
              <MetricTile label={t('common.metric.total')} value={workflowSummary.total} />
              <MetricTile tone={workflowSummary.successRate >= 80 || workflowSummary.total === 0 ? 'ok' : 'warn'} label={t('common.metric.successRate')} value={`${workflowSummary.successRate.toFixed(1)}%`} detail={t('stats.detail.okFailed', { ok: workflowSummary.completed, failed: workflowSummary.failed })} />
              <MetricTile tone="ok" label={t('workflows.metric.completed')} value={workflowSummary.completed} />
              <MetricTile tone={workflowSummary.warning > 0 ? 'warn' : undefined} label={t('workflows.metric.warnings')} value={workflowSummary.warning} />
              <MetricTile tone={workflowSummary.failed > 0 ? 'err' : undefined} label={t('workflows.metric.failed')} value={workflowSummary.failed} />
              <MetricTile tone={workflowSummary.zeroResults > 0 ? 'warn' : undefined} label={t('workflows.metric.zeroResult')} value={workflowSummary.zeroResults} />
              <MetricTile label={t('workflows.metric.searches')} value={workflowSummary.searches} detail={t('workflows.metric.avgSteps', { value: workflowSummary.avgSteps.toFixed(1) })} />
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
              action={<button className="refresh-btn" type="button" onClick={fetchTasks}>{t('action.refresh')}</button>}
            />
            <StatusLine text={updatedAt.tasks} error={errors.tasks} />
            <div className="metric-grid compact">
              <MetricTile label={t('common.metric.total')} value={taskSummary.total} />
              <MetricTile tone={taskSummary.successRate >= 80 || taskSummary.total === 0 ? 'ok' : 'warn'} label={t('common.metric.successRate')} value={`${taskSummary.successRate.toFixed(1)}%`} detail={t('stats.detail.okFailed', { ok: taskSummary.completed, failed: taskSummary.failed })} />
              <MetricTile tone="ok" label={t('tasks.metric.completed')} value={taskSummary.completed} />
              <MetricTile tone={taskSummary.failed > 0 ? 'err' : undefined} label={t('tasks.metric.failed')} value={taskSummary.failed} />
              <MetricTile tone={taskSummary.active > 0 ? 'warn' : undefined} label={t('tasks.metric.activeWaiting')} value={taskSummary.active} />
              <MetricTile label={t('common.metric.avgDuration')} value={formatDurationMs(taskSummary.avgDurationMs)} />
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
            <button className="refresh-btn" type="button" onClick={fetchCalls}>{t('action.refresh')}</button>
          </section>
        )}

        {activePanel === 'traces' && (
          <section className="panel active traces-panel" data-panel="traces">
            <PanelHeader
              title={t('traces.title')}
              meta={t('traces.meta')}
              action={<button className="refresh-btn" type="button" onClick={fetchTraces}>{t('action.refresh')}</button>}
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
                    {t('action.exportJsonl')}
                  </a>
                  <button className="refresh-btn" type="button" onClick={fetchTraffic}>{t('action.refresh')}</button>
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
                                <button className="refresh-btn" type="button" onClick={() => setTrafficDetail(trafficFrameDetail(frame))}>{t('action.view')}</button>
                                {requestId ? (
                                  <button className="refresh-btn" type="button" onClick={() => goToPanel('traces', { traceId: requestId })}>{t('action.trace')}</button>
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
                    <button className="refresh-btn" type="button" onClick={() => void copyText(trafficDetail, 'traffic frame JSON')}>{t('action.copy')}</button>
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
                  <button className="refresh-btn" type="button" onClick={fetchStats}>{t('action.refresh')}</button>
                </div>
              )}
            />
            <StatusLine text={updatedAt.stats} error={errors.stats} />
            {stats?.error ? <p className="empty">{stats.error}</p> : null}
            <div className="stats-hero">
              <HeroMetric
                accent
                label={t('stats.hero.totalTokens')}
                value={formatTokenCount(heroTokens.total)}
                detail={(
                  <>
                    {t('stats.hero.perCall', { value: formatTokenCount(heroTokens.avg) })}
                    {' · '}
                    {t('stats.hero.estimator', { name: heroTokens.estimator })}
                  </>
                )}
              />
              <HeroMetric
                label={t('stats.hero.inputTokens')}
                value={formatTokenCount(heroTokens.input)}
                detail={t('stats.hero.outputTokens') + ': ' + formatTokenCount(heroTokens.output)}
              />
              <HeroMetric
                label={t('stats.hero.tokensSaved')}
                value={formatTokenCount(heroTokens.saved)}
                detail={<strong>{t('stats.hero.savings', { value: formatSavingsPct(heroTokens.savedPct) })}</strong>}
              />
              <HeroMetric
                label={t('stats.hero.totalCalls')}
                value={(stats?.total_calls ?? 0).toLocaleString()}
                detail={t('stats.hero.successRate', { value: stats ? `${stats.success_rate.toFixed(1)}%` : '0.0%' })}
              />
            </div>
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
              action={<button className="refresh-btn" type="button" onClick={fetchGovernance}>{t('action.refresh')}</button>}
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

        <SkillsPanel
          active={activePanel === 'skill-paths'}
          search={listSearch}
          updatedAt={updatedAt['skill-paths']}
          error={errors['skill-paths']}
          onUpdated={(text) => markUpdated('skill-paths', text)}
          onError={(err) => markError('skill-paths', err)}
          onCountsChange={setSkillCounts}
          t={t}
        />

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
