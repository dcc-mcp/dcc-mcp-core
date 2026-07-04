import { useMemo, useCallback } from 'react';
import { RiDownloadCloudLine, RiRefreshLine, RiFileCopyLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../components/ui/select';
import {
  PanelHeader,
  StatusLine,
  HeroMetric,
  MetricTile,
  StatBarList,
  AttributionFacetList,
  HourlyChart,
  TokenBreakdownList,
  TimeValue,
  compactId,
  compactList,
  formatBytes,
  formatDurationMs,
  formatSavingsPct,
  formatTokenCount,
  isSlowLatency,
  latencyTone,
  trafficBodyBytes,
  trafficEmptyKey,
  trafficFrameDetail,
  trafficMethod,
  trafficRedactedPaths,
  trafficRequestId,
  trafficSessionId,
  trafficStatusDetailKey,
  trafficStatusLabelKey,
  trafficStatusTone,
  trafficTimestamp,
  isOkStatus,
  isErrStatus,
  StatusBadge,
  matchesListFilter,
  haystack,
  totalTraceTokens,
  agentLabel,
  traceLatency,
} from '../../admin-ui-core';
import { API_BASE } from '../../platform';
import type {
  StatsPayload,
  TrafficPayload,
  HealthPayload,
  TraceRow,
  CallRow,
  TopEntry,
  AttributionFacet,
  TokenBreakdownEntry,
  NavigateOptions,
  Panel,
  Translator,
} from '../../admin-types';

export type OverviewTab = 'stats' | 'traffic';

export type OverviewPanelProps = {
  active: boolean;
  overviewTab: OverviewTab;
  onTabChange: (tab: OverviewTab) => void;
  stats: StatsPayload | null;
  statsRange: string;
  onStatsRangeChange: (range: string) => void;
  onStatsRefresh: () => void;
  health: HealthPayload | null;
  traces: TraceRow[];
  calls: CallRow[];
  traffic: TrafficPayload | null;
  search: string;
  trafficDetail: string;
  onSetTrafficDetail: (detail: string) => void;
  onGoToPanel: (panel: Panel, opts?: NavigateOptions) => void;
  onCopyText: (text: string, label: string) => void;
  onTrafficRefresh: () => void;
  copiedNotice: string;
  updatedAt: { stats: string; traffic: string };
  errors: { stats?: string; traffic?: string };
  t: Translator;
};

const TABS: { id: OverviewTab; labelKey: string }[] = [
  { id: 'stats', labelKey: 'navigation.overviewTab.stats' },
  { id: 'traffic', labelKey: 'navigation.overviewTab.traffic' },
];

type OverviewIssue = {
  key: string;
  tone: 'err' | 'warn' | 'ok';
  status: string;
  title: string;
  detail: string;
  action: string;
  panel: Panel;
  opts?: NavigateOptions;
};

export function OverviewPanel({
  active,
  overviewTab,
  onTabChange,
  stats,
  statsRange,
  onStatsRangeChange,
  onStatsRefresh,
  health,
  traces,
  calls,
  traffic,
  search,
  trafficDetail,
  onSetTrafficDetail,
  onGoToPanel,
  onCopyText,
  onTrafficRefresh,
  copiedNotice,
  updatedAt,
  errors,
  t,
}: OverviewPanelProps) {
  const slowTraces = useMemo(
    () =>
      [...traces]
        .filter((trace) => trace.total_ms != null)
        .sort((a, b) => traceLatency(b) - traceLatency(a))
        .slice(0, 8),
    [traces]
  );

  const slowTraceCount = useMemo(
    () => traces.filter((trace) => isSlowLatency(trace.total_ms)).length,
    [traces]
  );

  const slowCallCount = useMemo(
    () => calls.filter((call) => isSlowLatency(call.duration_ms)).length,
    [calls]
  );

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
    const failed =
      stats?.failed_calls ??
      Math.max(0, (stats?.total_calls ?? 0) - (stats?.successful_calls ?? 0));
    const success = stats?.successful_calls ?? Math.max(0, (stats?.total_calls ?? 0) - failed);
    return {
      success,
      failed,
      totalTokens: stats?.total_tokens ?? traceSummary.totalTokens,
      totalInputTokens: stats?.total_input_tokens ?? traceSummary.totalInputTokens,
      totalOutputTokens: stats?.total_output_tokens ?? traceSummary.totalOutputTokens,
      avgTokens:
        stats?.avg_tokens_per_call ?? stats?.avg_total_tokens_per_call ?? traceSummary.avgTokens,
    };
  }, [stats, traceSummary]);

  const heroTokens = useMemo(() => {
    const payload = stats?.payload_token_usage;
    const input =
      payload?.total_input_tokens ??
      stats?.total_input_tokens ??
      statsSummary.totalInputTokens ??
      0;
    const output =
      payload?.total_output_tokens ??
      stats?.total_output_tokens ??
      statsSummary.totalOutputTokens ??
      0;
    const total =
      payload?.total_tokens ??
      stats?.total_tokens ??
      (input || output ? input + output : statsSummary.totalTokens) ??
      0;
    return {
      total,
      input,
      output,
      avg:
        payload?.avg_total_tokens_per_call ??
        stats?.avg_tokens_per_call ??
        stats?.avg_total_tokens_per_call ??
        statsSummary.avgTokens ??
        0,
      saved: stats?.token_usage?.total_saved_tokens ?? 0,
      savedPct: stats?.token_usage?.average_savings_pct ?? 0,
      estimator:
        payload?.token_estimator ??
        stats?.payload_token_estimator ??
        health?.response_format?.token_estimator ??
        '-',
    };
  }, [health, stats, statsSummary]);

  const slowLatencyDetail = useMemo(() => {
    const slowest = slowTraces[0];
    if (!slowest) {
      return t('stats.detail.slowTraces', { count: slowTraceCount });
    }
    const span = slowest.slowest_span_name
      ? t('traces.detail.slowestSpan', {
          name: slowest.slowest_span_name,
          duration: formatDurationMs(slowest.slowest_span_ms),
        })
      : t('stats.detail.noSlowestSpan');
    return t('stats.detail.slowestTrace', {
      id: compactId(slowest.request_id),
      latency: formatDurationMs(slowest.total_ms),
      span,
    });
  }, [slowTraceCount, slowTraces, t]);

  const latencyThresholdDetail = useMemo(
    () =>
      t('common.detail.slowThreshold', {
        slow: formatDurationMs(1000),
        tail: formatDurationMs(5000),
      }),
    [t]
  );

  const topIssues = useMemo<OverviewIssue[]>(() => {
    const issues: OverviewIssue[] = [];
    const failedCalls = calls.filter((call) => isErrStatus(call.status));
    const slowestTrace = slowTraces[0];
    const tokenHeavyTrace = [...traces]
      .filter((trace) => totalTraceTokens(trace) != null)
      .sort((a, b) => (totalTraceTokens(b) ?? 0) - (totalTraceTokens(a) ?? 0))[0];
    const missingPayloadTokens = stats?.payload_token_usage?.calls_missing_payload_tokens ?? 0;

    if (failedCalls.length) {
      const failed = failedCalls[0];
      issues.push({
        key: 'failed-calls',
        tone: 'err',
        status: failed.status,
        title: t('stats.issue.failedCalls', { count: failedCalls.length }),
        detail: t('stats.issue.failedCallsDetail', {
          tool: failed.tool,
          id: compactId(failed.request_id),
          error: failed.error || failed.status,
        }),
        action: t('stats.issue.openCalls'),
        panel: 'traces',
        opts: { tracesTab: 'calls', traceId: failed.request_id },
      });
    }

    if (slowestTrace && isSlowLatency(slowestTrace.total_ms)) {
      issues.push({
        key: 'slowest-trace',
        tone: 'warn',
        status: slowestTrace.status,
        title: t('stats.issue.slowestTrace'),
        detail: t('stats.issue.slowestTraceDetail', {
          tool: slowestTrace.tool,
          id: compactId(slowestTrace.request_id),
          latency: formatDurationMs(slowestTrace.total_ms),
        }),
        action: t('stats.issue.openTimeline'),
        panel: 'traces',
        opts: { traceId: slowestTrace.request_id, tracesTab: 'traces' },
      });
    }

    if (tokenHeavyTrace && (totalTraceTokens(tokenHeavyTrace) ?? 0) > 0) {
      issues.push({
        key: 'token-heavy',
        tone: 'warn',
        status: 'token',
        title: t('stats.issue.tokenHeavy'),
        detail: t('stats.issue.tokenHeavyDetail', {
          tool: tokenHeavyTrace.tool,
          id: compactId(tokenHeavyTrace.request_id),
          tokens: formatTokenCount(totalTraceTokens(tokenHeavyTrace)),
          agent: agentLabel(tokenHeavyTrace),
        }),
        action: t('stats.issue.openTimeline'),
        panel: 'traces',
        opts: { traceId: tokenHeavyTrace.request_id, tracesTab: 'traces' },
      });
    }

    if (missingPayloadTokens > 0) {
      issues.push({
        key: 'missing-payload-tokens',
        tone: 'warn',
        status: 'coverage',
        title: t('stats.issue.missingPayloadTokens', { count: missingPayloadTokens }),
        detail: t('stats.issue.missingPayloadTokensDetail'),
        action: t('stats.issue.openCalls'),
        panel: 'traces',
        opts: { tracesTab: 'calls' },
      });
    }

    if (issues.length === 0) {
      issues.push({
        key: 'healthy',
        tone: 'ok',
        status: 'ok',
        title: t('stats.issue.healthy'),
        detail: t('stats.issue.healthyDetail'),
        action: t('stats.issue.openTraces'),
        panel: 'traces',
      });
    }

    return issues.slice(0, 4);
  }, [calls, slowTraces, stats, t, traces]);

  const filterTopEntries = useCallback(
    (rows: TopEntry[] | undefined) => {
      const q = search.trim().toLowerCase();
      const safeRows = rows ?? [];
      if (!q) {
        return safeRows;
      }
      return safeRows.filter((r) => r.name.toLowerCase().includes(q));
    },
    [search]
  );

  const filterAttributionFacets = useCallback(
    (rows: AttributionFacet[] | undefined) => {
      const q = search.trim().toLowerCase();
      const safeRows = rows ?? [];
      if (!q) {
        return safeRows;
      }
      return safeRows.filter((r) => r.name.toLowerCase().includes(q));
    },
    [search]
  );

  const filterTokenBreakdowns = useCallback(
    (rows: TokenBreakdownEntry[] | undefined) => {
      const q = search.trim().toLowerCase();
      const safeRows = rows ?? [];
      if (!q) {
        return safeRows;
      }
      return safeRows.filter((r) => r.name.toLowerCase().includes(q));
    },
    [search]
  );

  const filteredTopAppTypes = useMemo(
    () => filterTopEntries(stats?.top_app_types),
    [filterTopEntries, stats]
  );
  const filteredTopTools = useMemo(
    () => filterTopEntries(stats?.top_tools),
    [filterTopEntries, stats]
  );
  const filteredTopInstances = useMemo(
    () => filterTopEntries(stats?.top_instances),
    [filterTopEntries, stats]
  );
  const filteredTopAgents = useMemo(
    () => filterTopEntries(stats?.top_agents),
    [filterTopEntries, stats]
  );

  const filteredTopActors = useMemo(
    () => filterAttributionFacets(stats?.top_actors),
    [filterAttributionFacets, stats]
  );
  const filteredTopClientPlatforms = useMemo(
    () => filterAttributionFacets(stats?.top_client_platforms),
    [filterAttributionFacets, stats]
  );
  const filteredTopSourceIps = useMemo(
    () => filterAttributionFacets(stats?.top_source_ips),
    [filterAttributionFacets, stats]
  );

  const filteredTokenByTool = useMemo(
    () => filterTokenBreakdowns(stats?.token_usage?.by_tool),
    [filterTokenBreakdowns, stats]
  );
  const filteredTokenByInstance = useMemo(
    () => filterTokenBreakdowns(stats?.token_usage?.by_instance),
    [filterTokenBreakdowns, stats]
  );
  const filteredTokenByAgent = useMemo(
    () => filterTokenBreakdowns(stats?.token_usage?.by_agent),
    [filterTokenBreakdowns, stats]
  );
  const filteredTokenByTransport = useMemo(
    () => filterTokenBreakdowns(stats?.token_usage?.by_transport),
    [filterTokenBreakdowns, stats]
  );
  const filteredTokenByFormat = useMemo(
    () => filterTokenBreakdowns(stats?.token_usage?.by_response_format),
    [filterTokenBreakdowns, stats]
  );

  const trafficFrames = useMemo(() => traffic?.frames ?? [], [traffic]);
  const filteredTrafficFrames = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) {
      return trafficFrames;
    }
    return trafficFrames.filter((frame) =>
      matchesListFilter(
        q,
        haystack(
          trafficTimestamp(frame),
          trafficMethod(frame),
          frame.attributes?.leg ?? '',
          frame.attributes?.transport ?? '',
          frame.attributes?.http?.method ?? '',
          frame.attributes?.http?.url ?? '',
          String(frame.attributes?.http?.status ?? ''),
          trafficSessionId(frame),
          trafficRequestId(frame),
          (frame.correlation as any)?.trace_id ?? '',
          (frame.correlation as any)?.dcc_type ?? '',
          (frame.correlation as any)?.workflow_id ?? '',
          (frame.correlation as any)?.job_id ?? '',
          (frame.correlation as any)?.agent_id ?? '',
          (frame.correlation as any)?.actor_id ?? '',
          (frame.correlation as any)?.actor_name ?? '',
          (frame.correlation as any)?.client_platform ?? '',
          (frame.correlation as any)?.source_ip ?? ''
        )
      )
    );
  }, [trafficFrames, search]);

  const trafficSummary = useMemo(() => {
    const sessions = new Set(trafficFrames.map(trafficSessionId).filter(Boolean)).size;
    const redacted = trafficFrames.reduce(
      (sum, frame) => sum + trafficRedactedPaths(frame).length,
      0
    );
    const bytes = trafficFrames.reduce((sum, frame) => sum + (trafficBodyBytes(frame) ?? 0), 0);
    const transports = new Set(
      trafficFrames.map((frame) => frame.attributes?.transport).filter(Boolean)
    ).size;
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

  if (!active) return null;

  return (
    <section className="panel active overview-panel" data-panel="overview">
      <PanelHeader
        title={t('navigation.panel.overview')}
        meta={t('navigation.overviewTab.meta')}
        action={
          <div className="table-actions">
            <nav className="overview-tabs" role="tablist" aria-label={t('navigation.overviewTab.meta')}>
              {TABS.map((tab) => (
                <button
                  key={tab.id}
                  className={overviewTab === tab.id ? 'discover-tab active' : 'discover-tab'}
                  role="tab"
                  aria-selected={overviewTab === tab.id}
                  type="button"
                  onClick={() => onTabChange(tab.id)}
                >
                  {t(tab.labelKey as any)}
                </button>
              ))}
            </nav>
            {overviewTab === 'stats' ? (
              <div className="stats-actions">
                <div className="range-control">
                  <span className="range-label" id="overview-stats-range-label">
                    {t('stats.label.range')}
                  </span>
                  <Select value={statsRange} onValueChange={onStatsRangeChange}>
                    <SelectTrigger
                      className="admin-select-trigger range-select-trigger"
                      id="overview-stats-range-select"
                      size="sm"
                      aria-labelledby="overview-stats-range-label"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent className="admin-select-content" position="popper" align="start">
                      <SelectGroup>
                        <SelectItem value="1h">1h</SelectItem>
                        <SelectItem value="24h">24h</SelectItem>
                        <SelectItem value="7d">7d</SelectItem>
                        <SelectItem value="all">All</SelectItem>
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </div>
                <Button type="button" size="sm" onClick={onStatsRefresh}>
                  <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
                  {t('action.refresh')}
                </Button>
              </div>
            ) : (
              <div className="table-actions">
                <Button asChild variant="outline" size="sm">
                  <a
                    href={traffic?.links?.traffic_export_jsonl_url ?? `${API_BASE}/traffic/export?limit=1000`}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    <RiDownloadCloudLine data-icon="inline-start" aria-hidden="true" />
                    {t('action.exportJsonl')}
                  </a>
                </Button>
                <Button type="button" size="sm" onClick={onTrafficRefresh}>
                  <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
                  {t('action.refresh')}
                </Button>
              </div>
            )}
          </div>
        }
      />

      {overviewTab === 'stats' ? (
        <>
          <StatusLine text={updatedAt.stats} error={errors.stats} />
          {stats?.error ? <p className="empty">{stats.error}</p> : null}
          <div className="stats-hero">
            <HeroMetric
              accent
              label={t('stats.hero.totalTokens')}
              value={formatTokenCount(heroTokens.total)}
              detail={
                <>
                  {t('stats.hero.perCall', { value: formatTokenCount(heroTokens.avg) })}
                  {' · '}
                  {t('stats.hero.estimator', { name: heroTokens.estimator })}
                </>
              }
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
            <MetricTile tone={latencyTone(stats?.latency_ms?.p50_ms ?? stats?.p50_ms) ? 'warn' : 'ok'} label={t('stats.metric.success')} value={stats ? `${stats.success_rate.toFixed(1)}%` : '0.0%'} detail={t('stats.detail.okFailed', { ok: statsSummary.success, failed: statsSummary.failed })} />
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
          <section className="overview-issues" aria-label={t('stats.issue.title')}>
            <div className="trace-card-head">
              <h3>{t('stats.issue.title')}</h3>
              <span>{t('stats.issue.meta')}</span>
            </div>
            <div className="overview-issue-grid">
              {topIssues.map((issue) => (
                <article className={`overview-issue ${issue.tone}`} key={issue.key}>
                  <div className="overview-issue-main">
                    <StatusBadge value={issue.status} />
                    <div>
                      <h4>{issue.title}</h4>
                      <p>{issue.detail}</p>
                    </div>
                  </div>
                  <Button
                    variant={issue.tone === 'ok' ? 'outline' : 'secondary'}
                    size="sm"
                    type="button"
                    onClick={() => onGoToPanel(issue.panel, issue.opts)}
                  >
                    {issue.action}
                  </Button>
                </article>
              ))}
            </div>
          </section>
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
        </>
      ) : (
        <>
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
          {trafficFrames.length === 0 ? (
            <p className="empty">{t(trafficEmptyKey(trafficCaptureStatus))}</p>
          ) : filteredTrafficFrames.length === 0 ? (
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
                          <td>
                            <TimeValue value={trafficTimestamp(frame)} />
                          </td>
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
                          <td className="mono-path">
                            {compactList(trafficRedactedPaths(frame), t('governance.privacy.none'))}
                          </td>
                          <td>
                            <div className="table-actions">
                              <Button
                                variant="secondary"
                                size="xs"
                                type="button"
                                onClick={() => onSetTrafficDetail(trafficFrameDetail(frame))}
                              >
                                {t('action.view')}
                              </Button>
                              {requestId ? (
                                <Button
                                  variant="secondary"
                                  size="xs"
                                  type="button"
                                  onClick={() => onGoToPanel('traces', { traceId: requestId })}
                                >
                                  {t('action.trace')}
                                </Button>
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
                  <Button
                    variant="outline"
                    size="sm"
                    type="button"
                    onClick={() => void onCopyText(trafficDetail, 'traffic frame JSON')}
                  >
                    <RiFileCopyLine data-icon="inline-start" aria-hidden="true" />
                    {t('action.copy')}
                  </Button>
                </div>
                <pre className="payload-pre">{trafficDetail}</pre>
              </div>
            </div>
          )}
        </>
      )}
    </section>
  );
}
