import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { RiRefreshLine } from '@remixicon/react';

import {
  actorLabel,
  apiJson,
  compactId,
  formatDurationMs,
  formatTokenCount,
  formatTraceDate,
  MetricTile,
  PanelHeader,
  spanDurationMs,
  StatusLine,
  totalTraceTokens,
} from '../../admin-ui-core';
import type { TraceDetailPayload, TraceRow, TraceSpan, Translator } from '../../admin-types';
import { Badge } from '../../components/ui/badge';
import { Button } from '../../components/ui/button';
import { Input } from '../../components/ui/input';
import { Separator } from '../../components/ui/separator';
import './experiments.css';

type Experiment = {
  experiment_id: string;
  name: string;
  scenario_id: string;
};

type ExperimentRun = {
  run_id: string;
  session_id: string;
  parent_run_id?: string | null;
  parent_session_id?: string | null;
  status: string;
  parameters?: Record<string, unknown>;
  metrics?: Record<string, unknown>;
  evidence?: string[];
};

type JudgeResult = {
  run_id?: string;
  evaluator_id: string;
  status: string;
  summary?: string;
  scores?: Record<string, unknown>;
  evidence?: string[];
};

type ExperimentList = { experiments: Experiment[] };
type ExperimentDetail = {
  experiment: Experiment;
  runs: ExperimentRun[];
  judge_results: JudgeResult[];
  metrics?: {
    runs?: Record<string, number>;
    judges?: Record<string, number>;
  };
};

type PayloadSection = { role: string; content: string };

const POLL_INTERVAL_MS = 5_000;

function percentile(values: number[], ratio: number): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length * ratio) - 1] ?? null;
}

function prettyPayload(content: string): string {
  try {
    return JSON.stringify(JSON.parse(content), null, 2);
  } catch {
    return content;
  }
}

function payloadSections(trace: TraceDetailPayload): PayloadSection[] {
  const sections: PayloadSection[] = [];
  if (trace.input?.content) {
    try {
      const parsed = JSON.parse(trace.input.content) as { messages?: { role?: unknown; content?: unknown }[] };
      for (const message of parsed.messages ?? []) {
        sections.push({
          role: typeof message.role === 'string' ? message.role : 'message',
          content: typeof message.content === 'string' ? message.content : JSON.stringify(message.content, null, 2),
        });
      }
    } catch {
      sections.push({ role: 'request', content: trace.input.content });
    }
  }
  if (trace.output?.content) sections.push({ role: 'response', content: prettyPayload(trace.output.content) });
  return sections;
}

function DetailView({
  detail,
  experiment,
  judges,
  onBack,
  t,
}: {
  detail: TraceDetailPayload;
  experiment: Experiment;
  judges: JudgeResult[];
  onBack: () => void;
  t: Translator;
}) {
  const [spanQuery, setSpanQuery] = useState('');
  const [selectedSpan, setSelectedSpan] = useState(0);
  const [tab, setTab] = useState<'payload' | 'events'>('payload');
  const spans = detail.spans.filter((span) => span.name.toLowerCase().includes(spanQuery.toLowerCase()));
  const span = spans[selectedSpan] ?? spans[0] ?? detail.spans[0];
  const inputTokens = detail.input_tokens ?? detail.input?.estimated_tokens;
  const outputTokens = detail.output_tokens ?? detail.output?.estimated_tokens;
  const totalTokens = detail.total_tokens ?? ((inputTokens ?? 0) + (outputTokens ?? 0));

  return (
    <div className="experiment-trace-detail">
      <header className="experiment-detail-header">
        <Button type="button" variant="secondary" size="sm" onClick={onBack}>← {t('experiments.action.back')}</Button>
        <div className="experiment-trace-identity">
          <strong>{detail.tool_slug ?? detail.method}</strong>
          <span><b>{t('experiments.detail.traceId')}</b> <code>{detail.request_id}</code></span>
          <span><b>{t('experiments.detail.sessionId')}</b> <code>{detail.session_id ?? detail.agent_context?.session_id ?? '—'}</code></span>
          <Badge variant="outline">{experiment.scenario_id}</Badge>
          <span>{detail.agent_context?.actor_name ?? '—'}</span>
        </div>
      </header>

      <Separator />

      <div className="experiment-detail-summary">
        <span>{t('experiments.table.started')} <strong>{formatTraceDate(detail.started_at)}</strong></span>
        <span>{t('experiments.table.latency')} <strong>{formatDurationMs(detail.total_ms)}</strong></span>
        <span>{t('experiments.detail.spans')} <strong>{detail.spans.length}</strong></span>
        <span>{t('experiments.detail.inputTokens')} <strong>{formatTokenCount(inputTokens)}</strong></span>
        <span>{t('experiments.detail.outputTokens')} <strong>{formatTokenCount(outputTokens)}</strong></span>
        <span>{t('experiments.kpi.tokens')} <strong>{formatTokenCount(totalTokens)}</strong></span>
      </div>

      <div className="experiment-detail-body">
        <aside className="experiment-span-rail" aria-label={t('experiments.detail.spanList')}>
          <Input
            value={spanQuery}
            onChange={(event) => { setSpanQuery(event.target.value); setSelectedSpan(0); }}
            placeholder={t('experiments.filter.span')}
          />
          <div className="experiment-span-list">
            {spans.map((item, index) => (
              <button
                type="button"
                key={`${item.name}-${item.started_ns}`}
                className={`experiment-span-item${span === item ? ' selected' : ''}`}
                onClick={() => setSelectedSpan(index)}
              >
                <span className={`experiment-status-dot ${item.ok ? 'ok' : 'err'}`} />
                <span><strong>{item.name}</strong><small>{formatDurationMs(spanDurationMs(item))}</small></span>
              </button>
            ))}
          </div>
        </aside>

        <main className="experiment-span-detail">
          <header>
            <div>
              <Badge variant="outline" className={span?.ok ? 'tone-ok' : 'tone-err'}>{span?.ok ? 'SPAN' : 'ERROR'}</Badge>
              <strong>{span?.name ?? detail.method}</strong>
            </div>
            <span>{formatDurationMs(span ? spanDurationMs(span) : detail.total_ms)}</span>
          </header>
          <nav className="experiment-detail-tabs" role="tablist">
            <button type="button" role="tab" aria-selected={tab === 'payload'} className={tab === 'payload' ? 'active' : ''} onClick={() => setTab('payload')}>
              {t('experiments.detail.payload')}
            </button>
            <button type="button" role="tab" aria-selected={tab === 'events'} className={tab === 'events' ? 'active' : ''} onClick={() => setTab('events')}>
              {t('experiments.detail.events')} ({detail.spans.length})
            </button>
          </nav>
          {tab === 'payload' ? (
            <section className="experiment-payload-list">
              <div className="experiment-section-title">{t('experiments.detail.inputInfo')}</div>
              {payloadSections(detail).map((section, index) => (
                <details className="experiment-message-detail" key={`${section.role}-${index}`} open={index === 0}>
                  <summary><span>{index + 1}</span><strong>{section.role}</strong></summary>
                  <pre>{section.content}</pre>
                </details>
              ))}
            </section>
          ) : (
            <section className="experiment-event-list">
              {detail.spans.map((event) => (
                <article key={`${event.name}-${event.started_ns}`}>
                  <span className={`experiment-status-dot ${event.ok ? 'ok' : 'err'}`} />
                  <strong>{event.name}</strong>
                  <code>{formatDurationMs(spanDurationMs(event))}</code>
                  {event.attributes ? <pre>{JSON.stringify(event.attributes, null, 2)}</pre> : null}
                </article>
              ))}
            </section>
          )}
          {judges.length > 0 ? (
            <footer className="experiment-judge-evidence">
              <strong>{t('experiments.detail.judge')}</strong>
              {judges.map((judge) => <span key={judge.evaluator_id}><Badge variant="outline" className={judge.status === 'passed' ? 'tone-ok' : 'tone-err'}>{judge.status}</Badge>{judge.evaluator_id} · {judge.summary}</span>)}
            </footer>
          ) : null}
        </main>
      </div>
    </div>
  );
}

export function ExperimentsPanel({ active, t }: { active: boolean; t: Translator }) {
  const [selectedExperimentId, setSelectedExperimentId] = useState<string | null>(null);
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null);
  const [view, setView] = useState<'trace' | 'span'>('trace');
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [includeOk, setIncludeOk] = useState(true);
  const [includeError, setIncludeError] = useState(true);
  const [minLatency, setMinLatency] = useState('');
  const [maxLatency, setMaxLatency] = useState('');
  const [traceFilter, setTraceFilter] = useState('');
  const [sessionFilter, setSessionFilter] = useState('');
  const [actorFilter, setActorFilter] = useState('');

  const polling = active && autoRefresh ? POLL_INTERVAL_MS : false;
  const listQuery = useQuery({
    queryKey: ['admin', 'experiments'],
    queryFn: () => apiJson<ExperimentList>('/experiments'),
    enabled: active,
    refetchInterval: polling,
  });
  const experimentId = selectedExperimentId ?? listQuery.data?.experiments[0]?.experiment_id ?? null;
  const experimentQuery = useQuery({
    queryKey: ['admin', 'experiments', experimentId],
    queryFn: () => apiJson<ExperimentDetail>(`/experiments/${encodeURIComponent(experimentId!)}`),
    enabled: active && experimentId != null,
    refetchInterval: polling,
  });
  const tracesQuery = useQuery({
    queryKey: ['admin', 'traces', { limit: 500 }],
    queryFn: () => apiJson<{ traces: TraceRow[] }>('/traces?limit=500'),
    select: (payload) => payload.traces ?? [],
    enabled: active,
    refetchInterval: polling,
  });

  const runSessions = useMemo(() => new Set(experimentQuery.data?.runs.map((run) => run.session_id) ?? []), [experimentQuery.data]);
  const experimentTraces = useMemo(
    () => (tracesQuery.data ?? []).filter((trace) => trace.session_id && runSessions.has(trace.session_id)),
    [runSessions, tracesQuery.data],
  );
  const filteredTraces = useMemo(() => experimentTraces.filter((trace) => {
    const latency = trace.total_ms ?? 0;
    if (trace.success ? !includeOk : !includeError) return false;
    if (minLatency && latency < Number(minLatency)) return false;
    if (maxLatency && latency > Number(maxLatency)) return false;
    if (traceFilter && !trace.request_id.toLowerCase().includes(traceFilter.toLowerCase())) return false;
    if (sessionFilter && !trace.session_id?.toLowerCase().includes(sessionFilter.toLowerCase())) return false;
    return !actorFilter || actorLabel(trace).toLowerCase().includes(actorFilter.toLowerCase());
  }), [actorFilter, experimentTraces, includeError, includeOk, maxLatency, minLatency, sessionFilter, traceFilter]);

  const previewTraceId = selectedTraceId ?? (view === 'span' ? filteredTraces[0]?.request_id : null) ?? null;
  const traceDetailQuery = useQuery({
    queryKey: ['admin', 'trace-detail', previewTraceId],
    queryFn: () => apiJson<TraceDetailPayload>(`/traces/${encodeURIComponent(previewTraceId!)}`),
    enabled: active && previewTraceId != null,
  });

  if (!active) return null;
  const experiment = experimentQuery.data?.experiment;
  const latencies = filteredTraces.flatMap((trace) => trace.total_ms == null ? [] : [trace.total_ms]);
  const totalTokens = filteredTraces.reduce((sum, trace) => sum + (totalTraceTokens(trace) ?? 0), 0);
  const error = listQuery.error ?? experimentQuery.error ?? tracesQuery.error ?? traceDetailQuery.error;

  function resetFilters() {
    setIncludeOk(true); setIncludeError(true); setMinLatency(''); setMaxLatency('');
    setTraceFilter(''); setSessionFilter(''); setActorFilter('');
  }

  return (
    <section className="panel active experiments-panel" data-panel="experiments">
      <PanelHeader
        title={t('experiments.workspace.title')}
        meta={t('experiments.workspace.meta')}
        action={(
          <div className="experiment-header-actions">
            <label><input type="checkbox" checked={autoRefresh} onChange={(event) => setAutoRefresh(event.target.checked)} /> {t('experiments.action.autoRefresh')}</label>
            <Button type="button" size="sm" onClick={() => Promise.all([listQuery.refetch(), experimentQuery.refetch(), tracesQuery.refetch()])}>
              <RiRefreshLine data-icon="inline-start" aria-hidden="true" />{t('experiments.action.refresh')}
            </Button>
          </div>
        )}
      />
      <StatusLine text={listQuery.isLoading || experimentQuery.isLoading || tracesQuery.isLoading ? t('experiments.status.loading') : ''} error={error instanceof Error ? error.message : undefined} />

      {selectedTraceId && traceDetailQuery.data && experiment ? (
        <DetailView detail={traceDetailQuery.data} experiment={experiment} judges={experimentQuery.data?.judge_results ?? []} onBack={() => setSelectedTraceId(null)} t={t} />
      ) : (
        <div className="experiment-monitor">
          <aside className="experiment-filter-rail" aria-label={t('experiments.filter.title')}>
            <div className="experiment-filter-head"><strong>{t('experiments.filter.title')}</strong><button type="button" onClick={resetFilters}>{t('experiments.action.reset')}</button></div>
            <label>{t('experiments.filter.scenario')}
              <select value={experimentId ?? ''} onChange={(event) => setSelectedExperimentId(event.target.value)}>
                {listQuery.data?.experiments.map((item) => <option key={item.experiment_id} value={item.experiment_id}>{item.name}</option>)}
              </select>
            </label>
            <fieldset><legend>{t('experiments.filter.status')}</legend>
              <label><input type="checkbox" checked={includeOk} onChange={(event) => setIncludeOk(event.target.checked)} /> OK</label>
              <label><input type="checkbox" checked={includeError} onChange={(event) => setIncludeError(event.target.checked)} /> ERROR</label>
            </fieldset>
            <fieldset><legend>{t('experiments.filter.latency')}</legend><div className="experiment-range"><Input type="number" value={minLatency} onChange={(event) => setMinLatency(event.target.value)} placeholder="Min" /><span>–</span><Input type="number" value={maxLatency} onChange={(event) => setMaxLatency(event.target.value)} placeholder="Max" /></div></fieldset>
            <label>{t('experiments.filter.trace')}<Input value={traceFilter} onChange={(event) => setTraceFilter(event.target.value)} placeholder={t('experiments.filter.enter')} /></label>
            <label>{t('experiments.filter.session')}<Input value={sessionFilter} onChange={(event) => setSessionFilter(event.target.value)} placeholder={t('experiments.filter.enter')} /></label>
            <label>{t('experiments.filter.actor')}<Input value={actorFilter} onChange={(event) => setActorFilter(event.target.value)} placeholder={t('experiments.filter.enter')} /></label>
          </aside>

          <main className="experiment-trace-workspace">
            <div className="experiment-toolbar">
              <nav role="tablist" className="experiment-view-tabs">
                <button type="button" role="tab" aria-selected={view === 'trace'} className={view === 'trace' ? 'active' : ''} onClick={() => setView('trace')}>Trace</button>
                <button type="button" role="tab" aria-selected={view === 'span'} className={view === 'span' ? 'active' : ''} onClick={() => setView('span')}>Span</button>
              </nav>
              <code>{experiment?.scenario_id ?? '—'}</code>
            </div>
            <section className="experiment-run-overview" aria-label={t('experiments.runs.title')}>
              <header>
                <div><strong>{t('experiments.runs.title')}</strong><span>{t('experiments.runs.meta')}</span></div>
                <span>{experimentQuery.data?.metrics?.runs?.total ?? 0} {t('experiments.runs.runs')} · {experimentQuery.data?.metrics?.judges?.total ?? 0} Judge</span>
              </header>
              <div className="experiment-run-grid">
                {(experimentQuery.data?.runs ?? []).map((run) => {
                  const label = typeof run.parameters?.label === 'string' ? run.parameters.label : run.run_id;
                  const parent = run.parent_run_id ?? run.parent_session_id;
                  const calls = typeof run.metrics?.tool_calls === 'number' ? run.metrics.tool_calls : null;
                  const duration = typeof run.metrics?.duration_ms === 'number' ? run.metrics.duration_ms : null;
                  return (
                    <article key={run.run_id} className={`experiment-run-card ${run.status}`}>
                      <div><span className={`experiment-status-dot ${run.status === 'passed' ? 'ok' : 'err'}`} /><strong>{label}</strong><Badge variant="outline">{run.status}</Badge></div>
                      <code title={run.session_id}>{compactId(run.session_id)}</code>
                      <small title={parent ?? undefined}>{parent ? `↳ ${parent}` : t('experiments.runs.root')}{calls != null ? ` · ${calls} ${t('experiments.runs.calls')}` : ''}{duration != null ? ` · ${formatDurationMs(duration)}` : ''}</small>
                    </article>
                  );
                })}
              </div>
              {(experimentQuery.data?.judge_results ?? []).map((judge) => (
                <article className="experiment-run-judge" key={`${judge.run_id ?? ''}-${judge.evaluator_id}`}>
                  <Badge variant="outline" className={judge.status === 'passed' ? 'tone-ok' : 'tone-err'}>{judge.status}</Badge>
                  <div><strong>{judge.evaluator_id}</strong><span>{judge.summary ?? '—'}</span></div>
                  {judge.scores ? <code>{Object.entries(judge.scores).map(([key, value]) => `${key} ${String(value)}`).join(' · ')}</code> : null}
                  {judge.evidence?.length ? <small>{judge.evidence.length} {t('experiments.runs.evidence')}</small> : null}
                </article>
              ))}
            </section>
            <div className="experiment-trace-kpis">
              <MetricTile label={t('experiments.kpi.requests')} value={filteredTraces.length} />
              <MetricTile tone={filteredTraces.some((trace) => !trace.success) ? 'err' : 'ok'} label={t('experiments.kpi.errors')} value={filteredTraces.filter((trace) => !trace.success).length} />
              <MetricTile label={t('experiments.kpi.p50')} value={formatDurationMs(percentile(latencies, 0.5))} />
              <MetricTile label={t('experiments.kpi.p99')} value={formatDurationMs(percentile(latencies, 0.99))} />
              <MetricTile label={t('experiments.kpi.tokens')} value={formatTokenCount(totalTokens)} />
            </div>

            {view === 'trace' ? (
              <div className="experiment-trace-table-wrap">
                <table className="experiment-trace-table">
                  <thead><tr><th>{t('experiments.table.status')}</th><th>{t('experiments.table.io')}</th><th>{t('experiments.table.started')}</th><th>{t('experiments.table.latency')}</th><th>{t('experiments.table.session')}</th><th>{t('experiments.table.actor')}</th><th>{t('experiments.kpi.tokens')}</th><th>{t('experiments.table.app')}</th></tr></thead>
                  <tbody>{filteredTraces.map((trace) => (
                    <tr className="experiment-trace-row" key={trace.request_id} tabIndex={0} onClick={() => setSelectedTraceId(trace.request_id)} onKeyDown={(event) => { if (event.key === 'Enter') setSelectedTraceId(trace.request_id); }}>
                      <td><span className={`experiment-status-dot ${trace.success ? 'ok' : 'err'}`} /><span className="sr-only">{trace.status}</span></td>
                      <td><strong>{trace.tool}</strong><small>{compactId(trace.request_id)} · {trace.success ? 'Completed' : 'Failed'}</small></td>
                      <td>{formatTraceDate(trace.timestamp)}</td><td><span className="experiment-latency">{formatDurationMs(trace.total_ms)}</span></td>
                      <td><code title={trace.session_id ?? ''}>{trace.session_id ?? '—'}</code></td><td>{actorLabel(trace)}</td>
                      <td><small>↑ {formatTokenCount(trace.input_tokens)}<br />↓ {formatTokenCount(trace.output_tokens)}</small></td><td>{trace.dcc_type ?? '—'}</td>
                    </tr>
                  ))}</tbody>
                </table>
                {filteredTraces.length === 0 ? <p className="empty">{t('experiments.empty.traces')}</p> : null}
              </div>
            ) : (
              <div className="experiment-trace-table-wrap">
                <table className="experiment-trace-table experiment-span-table"><thead><tr><th>{t('experiments.table.status')}</th><th>Span</th><th>{t('experiments.table.latency')}</th><th>Trace ID</th></tr></thead>
                  <tbody>{(traceDetailQuery.data?.spans ?? []).map((span: TraceSpan) => <tr key={`${span.name}-${span.started_ns}`}><td><span className={`experiment-status-dot ${span.ok ? 'ok' : 'err'}`} /></td><td><strong>{span.name}</strong></td><td>{formatDurationMs(spanDurationMs(span))}</td><td><code>{compactId(previewTraceId)}</code></td></tr>)}</tbody>
                </table>
              </div>
            )}
          </main>
        </div>
      )}
    </section>
  );
}
