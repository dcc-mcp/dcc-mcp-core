import { useState } from 'react';
import { RiArrowDownSLine, RiCheckLine, RiCloseLine, RiDownloadCloudLine, RiFileCopyLine, RiSearchLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import type { TraceDetailPayload, TraceRow, Translator } from '../../admin-types';
import {
  actorLabel, agentLabel, agentModelLabel, agentTurnChips, buildAgentPacket, compactId, detailTraceTokens,
  firstTrust, formatBytes, formatDurationMs, formatTokenCount, formatTraceDate, latencyClass,
  LatencyValue, payloadPreview, platformLabel, safeCallerContext, sourceIpLabel, StatusBadge,
  tokenAccounting, TokenAccountingDetail, traceLinks, TraceLinks, trustChip, trustFor,
} from '../../admin-ui-core';
import { recordedSpanTiming } from './trace-timing';

const formatSpanDuration = (value: number | null) => formatDurationMs(value == null ? null : Math.round(value * 100) / 100);

type Props = {
  trace: TraceDetailPayload | null;
  record?: TraceRow;
  fallback: string;
  t: Translator;
  onCopy: (text: string, label: string) => void;
  onCopyIssueReport: (requestId: string) => void;
  onDownloadIssueReport: (requestId: string) => void;
};

export function TraceDetailPanel(props: Props) {
  return props.trace
    ? <TraceRun key={props.trace.request_id} {...props} trace={props.trace} />
    : <div className="trace-detail trace-detail-empty"><p>{props.fallback}</p></div>;
}

function TraceRun({ trace, record, t, onCopy, onCopyIssueReport, onDownloadIssueReport }: Props & { trace: TraceDetailPayload }) {
  const [query, setQuery] = useState('');
  const [status, setStatus] = useState('all');
  const spans = Array.isArray(trace.spans) ? trace.spans : [];
  const { rows, extentMs } = recordedSpanTiming(spans);
  const filtered = rows.filter(({ span }) => (status !== 'failed' || !span.ok)
    && `${span.name} ${JSON.stringify(span.attributes ?? {})}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  const agent = trace.agent_context;
  const agentTitle = agent?.agent_name || agent?.agent_id || agent?.agent_kind;
  const model = agentModelLabel(agent);
  const tokens = detailTraceTokens(trace);
  const links = traceLinks(trace.request_id, trace.links);
  const validTiming = rows.filter((row) => row.offsetMs != null && row.durationMs != null);

  return (
    <section className="trace-detail-panel" aria-label={`${t('traces.label.request')} ${trace.request_id}`}>
      <header className="trace-run-header">
        <div className="trace-run-heading">
          <StatusBadge value={trace.ok ? 'ok' : 'err'} />
          <h3>{trace.tool_slug || trace.method}</h3>
          <LatencyValue value={trace.total_ms} t={t} />
        </div>
        <div className="trace-run-meta">
          {agentTitle ? <span className="trace-run-agent">{agentTitle}</span> : null}
          {model ? <span>{model}</span> : null}
          {trace.dcc_type ? <span>{trace.dcc_type}</span> : null}
          {trace.transport ? <span>{trace.transport}</span> : null}
          {trace.started_at != null ? <span>{formatTraceDate(trace.started_at)}</span> : null}
          <span title={trace.request_id}>{compactId(trace.request_id)}</span>
          {tokens.totalTokens != null ? <span>{t('traces.detail.tokenCount', { count: formatTokenCount(tokens.totalTokens) })}</span> : null}
        </div>
        {agent?.task || agent?.user_intent_summary ? <p className="trace-run-intent">{agent.task || agent.user_intent_summary}</p> : null}
      </header>

      {validTiming.length ? (
        <details className="trace-run-section trace-timing-section">
          <summary><RiArrowDownSLine aria-hidden="true" />{t('traces.detail.spanWaterfall')}<span>{formatSpanDuration(extentMs)}</span></summary>
          <p className="trace-section-hint">{t('traces.detail.recordedTimingHint')}</p>
          <div className="trace-timing-chart">
            {validTiming.map(({ span, index, offsetMs, durationMs }) => (
              <div className="trace-timing-row" key={index}>
                <span title={span.name}>{span.name}</span>
                <div className="trace-timing-track">
                  <span className={`trace-timing-bar${span.ok ? '' : ' is-failed'}`}
                    style={{ left: `${extentMs ? offsetMs! / extentMs * 100 : 0}%`, width: `${extentMs ? durationMs! / extentMs * 100 : 0}%` }}
                    title={`+${formatSpanDuration(offsetMs)} · ${formatSpanDuration(durationMs)}`} />
                </div>
                <span>{formatSpanDuration(durationMs)}</span>
              </div>
            ))}
          </div>
        </details>
      ) : null}

      <div className="trace-steps-section">
        <div className="trace-step-toolbar">
          <label className="trace-step-search">
            <RiSearchLine aria-hidden="true" />
            <input type="search" value={query} onChange={(event) => setQuery(event.target.value)}
              aria-label={t('traces.filter.searchSteps')} placeholder={t('traces.filter.searchSteps')} />
          </label>
          <select value={status} onChange={(event) => setStatus(event.target.value)} aria-label={t('traces.filter.stepStatus')}>
            <option value="all">{t('traces.filter.allSteps')}</option>
            <option value="failed">{t('traces.filter.failedSteps')}</option>
          </select>
          <span aria-live="polite">{filtered.length} / {spans.length}</span>
        </div>
        <div className="trace-step-list">
          {!filtered.length ? <p className="empty">{spans.length ? t('traces.empty.stepsSearch') : t('traces.detail.noSpans')}</p> : null}
          {filtered.map(({ span, index, offsetMs, durationMs }) => (
            <details className={`span-row ${span.ok ? 'ok' : 'err'} ${latencyClass(durationMs)}`} key={index}>
              <summary className="trace-step-summary">
                <span className="trace-step-offset" title={`${t('traces.detail.recordedTimingHint')}${offsetMs == null ? '' : ` +${offsetMs} ms`}`}>{offsetMs == null ? '—' : `+${formatSpanDuration(offsetMs)}`}</span>
                {span.ok ? <RiCheckLine className="trace-step-icon" aria-label="ok" /> : <RiCloseLine className="trace-step-icon is-failed" aria-label="err" />}
                <span className="trace-step-name" title={span.name}>{span.name}</span>
                <span className="trace-step-duration" title={durationMs == null ? undefined : `${durationMs} ms`}>{formatSpanDuration(durationMs)}</span>
                <RiArrowDownSLine className="trace-step-chevron" aria-hidden="true" />
              </summary>
              <div className="trace-step-content">
                <div className="trace-run-meta"><span>{t('traces.label.status')}: {span.ok ? 'ok' : 'err'}</span><LatencyValue value={durationMs} t={t} /></div>
                {span.attributes && Object.keys(span.attributes).length
                  ? <pre className="payload-pre">{JSON.stringify(span.attributes, null, 2)}</pre>
                  : <p className="trace-section-hint">{t('traces.detail.noAttributes')}</p>}
              </div>
            </details>
          ))}
        </div>
      </div>

      {agent ? (
        <details className="trace-run-section agent-context-card">
          <summary><RiArrowDownSLine aria-hidden="true" />{t('traces.label.callerContext')}{agentTitle ? <span>{agentTitle}</span> : null}</summary>
          <div className="trace-context-content">
            {agent.task ? <p className="agent-task">{agent.task}</p> : null}
            {agent.user_intent_summary ? <p className="agent-summary">{agent.user_intent_summary}</p> : null}
            {agent.agent_reply_summary ? <p className="agent-summary">{agent.agent_reply_summary}</p> : null}
            {agent.reasoning_summary ? <p className="agent-summary">{agent.reasoning_summary}</p> : null}
            {agent.plan?.length ? <div className="agent-list"><strong>{t('traces.label.plan')}</strong><ol>{agent.plan.map((step, index) => <li key={index}>{step}</li>)}</ol></div> : null}
            {agent.observations?.length ? <div className="agent-list"><strong>{t('traces.label.observations')}</strong><ul>{agent.observations.map((step, index) => <li key={index}>{step}</li>)}</ul></div> : null}
            <div className="agent-meta">
              {actorLabel(agent) !== '-' ? <span>{t('common.table.actor')}: {actorLabel(agent)} {trustChip(firstTrust(agent, ['actor_name', 'actor_id', 'actor_email_hash']))}</span> : null}
              {platformLabel(agent) !== '-' ? <span>{t('common.table.platform')}: {platformLabel(agent)} {trustChip(firstTrust(agent, ['client_platform', 'client_os', 'client_host']))}</span> : null}
              {sourceIpLabel(agent) !== '-' ? <span>{t('common.table.sourceIp')}: {sourceIpLabel(agent)} {trustChip(trustFor(agent, 'source_ip'))}</span> : null}
              {agent.auth_subject ? <span>{t('common.table.auth')}: {agent.auth_subject} {trustChip(trustFor(agent, 'auth_subject'))}</span> : null}
              {agentTurnChips(agent, t).map((chip) => <span key={chip}>{chip}</span>)}
              {agent.parent_request_id ? <span>parent {agent.parent_request_id}</span> : null}
              {agent.trace_id ? <span>trace {agent.trace_id}</span> : null}
              {agent.tags?.map((tag) => <span key={tag}>{tag}</span>)}
            </div>
            <details className="trace-raw-context"><summary>{t('traces.detail.rawContext')}</summary><pre className="payload-pre caller-context-pre">{JSON.stringify(safeCallerContext(agent), null, 2)}</pre></details>
          </div>
        </details>
      ) : null}

      {(['input', 'output'] as const).map((kind) => (
        <details className="trace-run-section trace-payload-section" key={kind}>
          <summary><RiArrowDownSLine aria-hidden="true" />{t(`traces.detail.${kind}`)}
            {trace[kind] ? <span>{formatBytes(trace[kind].original_size)}</span> : null}
          </summary>
          {trace[kind]?.estimated_tokens != null ? <p className="trace-section-hint">{t('traces.label.payloadEstimate')}: {formatTokenCount(trace[kind].estimated_tokens)}</p> : null}
          <pre className="payload-pre">{payloadPreview(trace[kind], t)}</pre>
        </details>
      ))}

      <details className="trace-run-section trace-record-section">
        <summary><RiArrowDownSLine aria-hidden="true" />{t('traces.detail.requestDetails')}</summary>
        <dl className="trace-record-grid">
          <div><dt>{t('traces.label.request')}</dt><dd>{trace.request_id}</dd></div>
          {trace.instance_id ? <div><dt>{t('common.table.instance')}</dt><dd>{trace.instance_id}</dd></div> : null}
          {trace.session_id ? <div><dt>{t('traces.agent.session', { id: '' })}</dt><dd>{trace.session_id}</dd></div> : null}
          {tokens.inputTokens != null ? <div><dt>{t('traces.label.inputTokens')}</dt><dd>{formatTokenCount(tokens.inputTokens)}</dd></div> : null}
          {tokens.outputTokens != null ? <div><dt>{t('traces.label.outputTokens')}</dt><dd>{formatTokenCount(tokens.outputTokens)}</dd></div> : null}
          {tokens.totalTokens != null ? <div><dt>{t('traces.label.totalTokens')}</dt><dd>{formatTokenCount(tokens.totalTokens)}</dd></div> : null}
          {tokens.estimator ? <div><dt>{t('traces.label.estimator')}</dt><dd>{tokens.estimator}</dd></div> : null}
        </dl>
        {record ? (
          <dl className="trace-record-grid">
            <div><dt>{t('common.table.actor')}</dt><dd>{actorLabel(record)} {trustChip(firstTrust(record, ['actor_name', 'actor_id', 'actor_email_hash', 'auth_subject']))}</dd></div>
            <div><dt>{t('common.table.platform')}</dt><dd>{platformLabel(record)} {trustChip(firstTrust(record, ['client_platform', 'client_os', 'client_host']))}</dd></div>
            <div><dt>{t('common.table.sourceIp')}</dt><dd>{sourceIpLabel(record)} {trustChip(trustFor(record, 'source_ip'))}</dd></div>
            <div><dt>{t('traces.label.callerContext')}</dt><dd>{agentLabel(record)}</dd></div>
            {record.slowest_span_name ? <div><dt>{t('traces.detail.spanWaterfall')}</dt><dd>{t('traces.detail.slowestSpan', { name: record.slowest_span_name, duration: formatDurationMs(record.slowest_span_ms) })}</dd></div> : null}
          </dl>
        ) : null}
        {tokenAccounting(trace) ? <TokenAccountingDetail row={trace} t={t} /> : null}
        {record ? <details className="trace-raw-context"><summary>{t('traces.detail.rawRecord')}</summary><pre className="payload-pre">{JSON.stringify(record, null, 2)}</pre></details> : null}
      </details>

      <details className="trace-run-section trace-evidence-section">
        <summary className="trace-related-head"><RiArrowDownSLine aria-hidden="true" />{t('traces.detail.relatedEvidence')}</summary>
        <p className="trace-section-hint">{t('traces.detail.relatedEvidenceHint')}</p>
        <div className="trace-copy-actions">
          <Button variant="outline" size="sm" onClick={() => onCopy(links.admin_trace_url ?? '', 'trace URL')}><RiFileCopyLine aria-hidden="true" />{t('traces.action.copyUrl')}</Button>
          <Button variant="outline" size="sm" onClick={() => onCopy(buildAgentPacket(trace), 'agent packet')}><RiFileCopyLine aria-hidden="true" />{t('traces.action.copyAgentPacket')}</Button>
          <Button variant="outline" size="sm" onClick={() => onCopyIssueReport(trace.request_id)}><RiFileCopyLine aria-hidden="true" />{t('traces.action.copyIssueJson')}</Button>
          <Button variant="outline" size="sm" onClick={() => onDownloadIssueReport(trace.request_id)}><RiDownloadCloudLine aria-hidden="true" />{t('traces.action.downloadJson')}</Button>
        </div>
        <TraceLinks links={links} t={t} />
      </details>
    </section>
  );
}
