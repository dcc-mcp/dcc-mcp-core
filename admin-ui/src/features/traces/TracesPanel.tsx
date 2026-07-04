import { useMemo } from 'react';
import { RiRefreshLine, RiFileCopyLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import {
  PanelHeader,
  StatusLine,
  MetricTile,
  StatusBadge,
  TimeValue,
  LatencyValue,
  TraceDetailPanel,
  groupRows,
  traceGroupLabel,
  compactId,
  compactInstanceId,
  actorLabel,
  platformLabel,
  sourceIpLabel,
  agentLabel,
  totalTraceTokens,
  isErrStatus,
  isWarnStatus,
  isOkStatus,
  latencyClass,
  latencyTone,
  firstTrust,
  trustChip,
  trustFor,
  callGroupLabel,
  responseFormatLabel,
  returnedTokensLabel,
  savedTokensLabel,
  traceLinks,
  isSlowLatency,
  traceLatency,
  formatDurationMs,
  formatTokenCount,
} from '../../admin-ui-core';
import type { TraceRow, CallRow, TraceDetailPayload, StatsPayload, Translator } from '../../admin-types';

export type TracesPanelProps = {
  updatedAt: string;
  error?: string;
  traces: TraceRow[];
  filteredTraces: TraceRow[];
  calls: CallRow[];
  filteredCalls: CallRow[];
  tracesTab: 'traces' | 'calls';
  onTracesTabChange: (tab: 'traces' | 'calls') => void;
  onSelectTraceId: (id: string | null) => void;
  traceDetailPayload: TraceDetailPayload | null;
  traceDetail: string;
  stats: StatsPayload | null;
  onCopyText: (text: string, label: string) => void;
  onCopyIssueReport: (requestId: string) => void;
  onDownloadIssueReport: (requestId: string) => void;
  onTracesRefresh: () => void;
  onCallsRefresh: () => void;
  copiedNotice: string;
  callDetail: string;
  onExpandTraceDetail: (requestId: string) => void;
  t: Translator;
};

export function TracesPanel({
  updatedAt,
  error,
  traces,
  filteredTraces,
  calls,
  filteredCalls,
  tracesTab,
  onTracesTabChange,
  onSelectTraceId,
  traceDetailPayload,
  traceDetail,
  stats,
  onCopyText,
  onCopyIssueReport,
  onDownloadIssueReport,
  onTracesRefresh,
  onCallsRefresh,
  copiedNotice,
  callDetail,
  onExpandTraceDetail,
  t,
}: TracesPanelProps) {
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

  const traceByRequest = useMemo(() => {
    const rows = new Map<string, TraceRow>();
    for (const trace of traces) {
      rows.set(trace.request_id, trace);
    }
    return rows;
  }, [traces]);

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

  return (
    <section className="panel active traces-panel" data-panel="traces">
      <PanelHeader
        title={t('traces.title')}
        meta={t('traces.meta')}
        action={
          <div className="table-actions">
            <nav className="discover-tabs traces-subnav" role="tablist" aria-label={t('navigation.tracesTab.meta')}>
              <button
                className={tracesTab === 'traces' ? 'discover-tab active' : 'discover-tab'}
                role="tab"
                aria-selected={tracesTab === 'traces'}
                type="button"
                onClick={() => onTracesTabChange('traces')}
              >
                {t('navigation.tracesTab.traces')}
              </button>
              <button
                className={tracesTab === 'calls' ? 'discover-tab active' : 'discover-tab'}
                role="tab"
                aria-selected={tracesTab === 'calls'}
                type="button"
                onClick={() => onTracesTabChange('calls')}
              >
                {t('navigation.tracesTab.calls')}
              </button>
            </nav>
            {tracesTab === 'traces' ? (
              <Button type="button" size="sm" onClick={onTracesRefresh}>
                <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
                {t('action.refresh')}
              </Button>
            ) : (
              <Button type="button" size="sm" onClick={onCallsRefresh}>
                <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
                {t('action.refresh')}
              </Button>
            )}
          </div>
        }
      />
      <StatusLine text={copiedNotice || updatedAt} error={error} />
      {tracesTab === 'traces' ? (
        <>
          <div className="metric-grid compact">
            <MetricTile tone="ok" label="OK" value={traceSummary.ok} />
            <MetricTile
              tone={traceSummary.failed > 0 ? 'err' : undefined}
              label={t('workflows.metric.failed')}
              value={traceSummary.failed}
            />
            <MetricTile
              tone={latencyTone(traceSummary.p95)}
              label={t('debug.metric.latency')}
              value={formatDurationMs(traceSummary.p95)}
            />
            <MetricTile
              tone={latencyTone(traceSummary.p99)}
              label={t('stats.metric.p99Latency')}
              value={formatDurationMs(traceSummary.p99)}
              detail={latencyThresholdDetail}
            />
            <MetricTile
              tone={traceSummary.slow > 0 ? 'warn' : undefined}
              label={t('stats.metric.slowCalls')}
              value={traceSummary.slow}
              detail={slowLatencyDetail}
            />
            <MetricTile
              label={t('traces.metric.totalTokens')}
              value={formatTokenCount(traceSummary.totalTokens)}
              detail={t('traces.detail.inputOutput', {
                input: formatTokenCount(traceSummary.totalInputTokens),
                output: formatTokenCount(traceSummary.totalOutputTokens),
              })}
            />
            <MetricTile
              label={t('traces.metric.agentContext')}
              value={traceSummary.agentContext}
              detail={t('traces.detail.agentCoverage', { count: traceSummary.agentContext, total: traces.length })}
            />
            <MetricTile label={t('traces.metric.spans')} value={traceSummary.spans} />
            <MetricTile label={t('common.metric.visible')} value={`${filteredTraces.length} / ${traces.length}`} />
          </div>
          {traces.length === 0 ? (
            <p className="empty">{t('traces.empty.none')}</p>
          ) : filteredTraces.length === 0 ? (
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
                          className={`trace-item ${
                            isErrStatus(trace.status)
                              ? 'err'
                              : isWarnStatus(trace.status)
                              ? 'warn'
                              : isOkStatus(trace.status)
                              ? 'ok'
                              : ''
                          } ${latencyClass(trace.total_ms)}`}
                          type="button"
                          onClick={() => onSelectTraceId(trace.request_id)}
                        >
                          <span className="trace-item-main">
                            <strong>{trace.tool}</strong>
                            <span>
                              {compactId(trace.request_id)} - {compactInstanceId(trace.instance_id)} -{' '}
                              <TimeValue value={trace.timestamp} /> - {trace.transport ?? '?'}
                            </span>
                            <span>
                              {actorLabel(trace)}{' '}
                              {trustChip(
                                firstTrust(trace, [
                                  'actor_name',
                                  'actor_id',
                                  'actor_email_hash',
                                  'auth_subject',
                                ])
                              )}
                              {' - '}
                              {platformLabel(trace)}{' '}
                              {trustChip(firstTrust(trace, ['client_platform', 'client_os', 'client_host']))}
                              {' - '}
                              {sourceIpLabel(trace)} {trustChip(trustFor(trace, 'source_ip'))}
                            </span>
                            <span>
                              {agentLabel(trace)}
                              {trace.slowest_span_name
                                ? ` - ${t('traces.detail.slowestSpan', {
                                    name: trace.slowest_span_name,
                                    duration: formatDurationMs(trace.slowest_span_ms),
                                  })}`
                                : ''}
                            </span>
                          </span>
                          <span className="trace-item-side">
                            <StatusBadge value={trace.status} />
                            <LatencyValue value={trace.total_ms} t={t} />
                            <span>{t('traces.detail.spanCount', { count: trace.span_count ?? 0 })}</span>
                            <span>
                              {t('traces.detail.tokenCount', {
                                count: formatTokenCount(totalTraceTokens(trace)),
                              })}
                            </span>
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
                onCopy={onCopyText}
                onCopyIssueReport={onCopyIssueReport}
                onDownloadIssueReport={onDownloadIssueReport}
              />
            </div>
          )}
        </>
      ) : (
        <>
          <h2>{t('calls.title')}</h2>
          <StatusLine text={updatedAt} error={error} />
          {calls.length === 0 ? (
            <p className="empty">{t('calls.empty.none')}</p>
          ) : filteredCalls.length === 0 ? (
            <p className="empty">{t('calls.empty.search')}</p>
          ) : (
            Array.from(groupRows(filteredCalls, callGroupLabel).entries())
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([group, groupCalls]) => (
                <div key={group} className="group-block">
                  <h3 className="group-title">{group}</h3>
                  <table>
                    <thead>
                      <tr>
                        <th>{t('common.table.time')}</th>
                        <th>{t('common.table.request')}</th>
                        <th>{t('common.table.tool')}</th>
                        <th>{t('common.table.appType')}</th>
                        <th>{t('common.table.instance')}</th>
                        <th>{t('common.table.actor')}</th>
                        <th>{t('calls.table.agent')}</th>
                        <th>{t('common.table.platform')}</th>
                        <th>{t('common.table.sourceIp')}</th>
                        <th>{t('calls.table.transport')}</th>
                        <th>{t('calls.table.format')}</th>
                        <th>{t('calls.table.returned')}</th>
                        <th>{t('calls.table.saved')}</th>
                        <th>{t('common.table.status')}</th>
                        <th>{t('calls.table.error')}</th>
                        <th>{t('common.table.ms')}</th>
                        <th>{t('calls.table.detail')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {groupCalls.map((call) => {
                        const trace = traceByRequest.get(call.request_id);
                        const slowestSpan = trace?.slowest_span_name
                          ? t('traces.detail.slowestSpan', {
                              name: trace.slowest_span_name,
                              duration: formatDurationMs(trace.slowest_span_ms),
                            })
                          : '';
                        return (
                          <tr key={call.request_id} className={`latency-row ${latencyClass(call.duration_ms)}`}>
                            <td>
                              <TimeValue value={call.timestamp} />
                            </td>
                            <td>
                              <Button
                                variant="secondary"
                                size="xs"
                                type="button"
                                title={call.request_id}
                                onClick={() => onSelectTraceId(call.request_id)}
                              >
                                {call.request_id.slice(0, 12)}
                              </Button>
                            </td>
                            <td>{call.tool}</td>
                            <td>{call.dcc_type}</td>
                            <td>{compactInstanceId(call.instance_id)}</td>
                            <td title={call.actor_id ?? call.auth_subject ?? ''}>
                              <span className="trust-cell">
                                {actorLabel(call)}
                                {trustChip(
                                  firstTrust(call, ['actor_name', 'actor_id', 'actor_email_hash', 'auth_subject'])
                                )}
                              </span>
                            </td>
                            <td title={call.agent_id ?? call.agent_name ?? ''}>{agentLabel(call)}</td>
                            <td title={[call.client_platform, call.client_os, call.client_host].filter(Boolean).join(' / ')}>
                              <span className="trust-cell">
                                {platformLabel(call)}
                                {trustChip(firstTrust(call, ['client_platform', 'client_os', 'client_host']))}
                              </span>
                            </td>
                            <td>
                              <span className="trust-cell">
                                {sourceIpLabel(call)}
                                {trustChip(trustFor(call, 'source_ip'))}
                              </span>
                            </td>
                            <td>{call.transport ?? '-'}</td>
                            <td>{responseFormatLabel(call)}</td>
                            <td>{returnedTokensLabel(call)}</td>
                            <td>{savedTokensLabel(call)}</td>
                            <td>
                              <StatusBadge value={call.status} />
                            </td>
                            <td title={call.error ?? ''}>{call.error ? call.error.slice(0, 80) : '-'}</td>
                            <td className="latency-cell">
                              <LatencyValue value={call.duration_ms} t={t} />
                              {slowestSpan ? <div className="latency-subtext">{slowestSpan}</div> : null}
                            </td>
                            <td>
                              <div className="table-actions">
                                <Button
                                  variant="secondary"
                                  size="xs"
                                  type="button"
                                  onClick={() => onExpandTraceDetail(call.request_id)}
                                >
                                  {t('calls.action.expand')}
                                </Button>
                                <Button
                                  variant="outline"
                                  size="xs"
                                  type="button"
                                  onClick={() =>
                                    void onCopyText(
                                      traceLinks(call.request_id, call.links).admin_trace_url ?? '',
                                      'trace URL'
                                    )
                                  }
                                >
                                  <RiFileCopyLine data-icon="inline-start" aria-hidden="true" />
                                  {t('traces.action.copyUrl')}
                                </Button>
                                <Button
                                  variant="outline"
                                  size="xs"
                                  type="button"
                                  onClick={() => onCopyIssueReport(call.request_id)}
                                >
                                  <RiFileCopyLine data-icon="inline-start" aria-hidden="true" />
                                  {t('traces.action.copyIssueJson')}
                                </Button>
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              ))
          )}
          <pre className="empty">{callDetail}</pre>
        </>
      )}
    </section>
  );
}
