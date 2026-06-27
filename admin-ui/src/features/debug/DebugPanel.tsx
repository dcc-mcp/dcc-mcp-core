import {
  RiArrowRightLine,
  RiRefreshLine,
} from '@remixicon/react';
import { Button } from '../../components/ui/button';
import type {
  ActivityEvent,
  DebugSignal,
  FailureSignal,
  HealthPayload,
  InstanceRow,
  InstanceSummary,
  NavigateOptions,
  Panel,
  StatsPayload,
  TraceRow,
  Translator,
} from '../../admin-types';
import type { LogRow } from '../../logs';
import {
  BackendOpenApiLinks,
  compactId,
  compactInstanceId,
  errorRateTone,
  formatDurationMs,
  formatTokenCount,
  gatewayLabel,
  groupRows,
  HealthCard,
  instanceGroupLabel,
  latencyClass,
  LatencyValue,
  latencyTone,
  MiniSparkline,
  StatusBadge,
  StatusLine,
  TimeValue,
  totalTraceTokens,
} from '../../admin-ui-core';

type TokenPressure = {
  total: number;
  input: number;
  output: number;
  avg: number;
  saved: number;
  estimator: string;
};

export type DebugPanelProps = {
  updatedAt: string;
  error?: string;
  debugIssues: number;
  health: HealthPayload | null;
  unhealthyInstanceRows: InstanceRow[];
  instanceSummary: InstanceSummary;
  stats: StatsPayload | null;
  tokenPressure: TokenPressure;
  slowLatencyDetail: string;
  debugSignals: DebugSignal[];
  tokenHeavyTraces: TraceRow[];
  failureSignals: FailureSignal[];
  slowTraces: TraceRow[];
  instanceRows: InstanceRow[];
  problemLogs: LogRow[];
  problemActivity: ActivityEvent[];
  onGoToPanel: (panel: Panel, opts?: NavigateOptions) => void;
  onRefresh: () => void;
  t: Translator;
};

export function DebugPanel({
  updatedAt,
  error,
  debugIssues,
  health,
  unhealthyInstanceRows,
  instanceSummary,
  stats,
  tokenPressure,
  slowLatencyDetail,
  debugSignals,
  tokenHeavyTraces,
  failureSignals,
  slowTraces,
  instanceRows,
  problemLogs,
  problemActivity,
  onGoToPanel,
  onRefresh,
  t,
}: DebugPanelProps) {
  return (
    <section className="panel active debug-panel">
      <div className="debug-hero">
        <div>
          <h2>{t('debug.title.workbench')}</h2>
          <StatusLine text={updatedAt} error={error} />
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
            <Button variant="ghost" size="xs" type="button" onClick={() => onGoToPanel('traces')}>
              {t('debug.action.openEvidence')}
              <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
            </Button>
          </div>
          <div className="debug-signal-list">
            {debugSignals.map((signal) => (
              <button
                key={signal.key}
                className={`debug-signal ${signal.tone}`}
                type="button"
                onClick={() => onGoToPanel(signal.panel, signal.traceId ? { traceId: signal.traceId } : undefined)}
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
            <Button variant="ghost" size="xs" type="button" onClick={() => onGoToPanel('overview', { overviewTab: 'stats' })}>
              {t('debug.action.openStats')}
              <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
            </Button>
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
            <Button variant="ghost" size="xs" type="button" onClick={() => onGoToPanel('overview', { overviewTab: 'stats' })}>
              {t('debug.action.openStats')}
              <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
            </Button>
          </div>
          <div className="debug-metrics">
            <span>{formatTokenCount(tokenPressure.total)} total</span>
            <span>{formatTokenCount(tokenPressure.input)} in</span>
            <span>{formatTokenCount(tokenPressure.output)} out</span>
            <span>{t('debug.detail.saved', { value: formatTokenCount(tokenPressure.saved) })}</span>
            <span>{tokenPressure.estimator}</span>
          </div>
          {tokenHeavyTraces.length === 0 ? <p className="empty">{t('debug.empty.tokenPressure')}</p> : tokenHeavyTraces.map((trace) => (
            <button key={trace.request_id} className="debug-row" type="button" onClick={() => onGoToPanel('traces', { traceId: trace.request_id })}>
              <span>{formatTokenCount(totalTraceTokens(trace))} tok</span>
              <span>{compactId(trace.request_id)}</span>
              <span title={trace.tool}>{trace.tool}</span>
            </button>
          ))}
        </div>

        <div className="debug-card">
          <div className="debug-card-head">
            <h3>{t('debug.section.failures')}</h3>
            <Button variant="ghost" size="xs" type="button" onClick={() => onGoToPanel('traces', { tracesTab: 'calls' })}>
              {t('debug.action.openCalls')}
              <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
            </Button>
          </div>
          {failureSignals.length === 0 ? <p className="empty">{t('debug.empty.failures')}</p> : failureSignals.map((failure) => (
            <button key={failure.request_id} className="debug-row" type="button" onClick={() => onGoToPanel('traces', { traceId: failure.request_id })}>
              <span><StatusBadge value={failure.status} /></span>
              <span>{compactId(failure.request_id)}</span>
              <span title={`${failure.tool} · ${failure.detail}`}>{failure.detail}</span>
            </button>
          ))}
        </div>

        <div className="debug-card">
          <div className="debug-card-head">
            <h3>{t('debug.section.slowestTraces')}</h3>
            <Button variant="ghost" size="xs" type="button" onClick={() => onGoToPanel('traces')}>
              {t('debug.action.openTraces')}
              <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
            </Button>
          </div>
          {slowTraces.length === 0 ? <p className="empty">{t('debug.empty.latency')}</p> : slowTraces.map((trace) => (
            <button key={trace.request_id} className={`debug-row ${latencyClass(trace.total_ms)}`} type="button" onClick={() => onGoToPanel('traces', { traceId: trace.request_id })}>
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
            <Button variant="ghost" size="xs" type="button" onClick={() => onGoToPanel('instances')}>
              {t('debug.action.openInstances')}
              <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
            </Button>
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
            <Button variant="ghost" size="xs" type="button" onClick={() => onGoToPanel('openapi')}>
              {t('debug.action.gatewaySpec')}
              <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
            </Button>
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
            <Button variant="ghost" size="xs" type="button" onClick={() => onGoToPanel('logs')}>
              {t('debug.action.openLogs')}
              <RiArrowRightLine data-icon="inline-end" aria-hidden="true" />
            </Button>
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
              onClick={() => row.request_id ? onGoToPanel('traces', { traceId: row.request_id }) : onGoToPanel('logs')}
            >
              <TimeValue value={row.timestamp} />
              <span>{row.source ?? row.level}</span>
              <span title={row.message}>{row.message}</span>
            </button>
          ))}
          {problemLogs.length === 0 && problemActivity.length === 0 ? <p className="empty">{t('debug.empty.events')}</p> : null}
        </div>
      </div>
      <Button type="button" size="sm" onClick={onRefresh}>
        <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
        {t('debug.action.refreshSnapshot')}
      </Button>
    </section>
  );
}
