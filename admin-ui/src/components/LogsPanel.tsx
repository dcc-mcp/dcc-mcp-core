import { RiRefreshLine } from '@remixicon/react';
import { Button } from './ui/button';
import { type InterpolationValues, type MessageKey } from '../i18n';
import {
  buildRequestLogGroups,
  LOG_SEVERITIES,
  logStepDetail,
  logStepTitle,
  normalizeLogSeverity,
  requestLogSeverity,
  type LogRow,
  type LogSeverity,
  type LogSeverityCounts,
  type LogSeverityFilter,
} from '../logs';
import { formatTime, timestampTitle } from '../time';

type Translator = (key: MessageKey, values?: InterpolationValues) => string;

type LogsPanelProps = {
  logs: LogRow[];
  filteredLogs: LogRow[];
  severityCounts: LogSeverityCounts;
  severityFilter: LogSeverityFilter;
  updatedAt: string;
  error?: string;
  onSeverityFilterChange: (severity: LogSeverityFilter) => void;
  onRefresh: () => void;
  t: Translator;
};

function groupRows<T>(rows: T[], keyFn: (row: T) => string): Map<string, T[]> {
  const map = new Map<string, T[]>();
  for (const row of rows) {
    const key = keyFn(row);
    const bucket = map.get(key) ?? [];
    bucket.push(row);
    map.set(key, bucket);
  }
  return map;
}

function severityLabel(severity: LogSeverityFilter, t: Translator): string {
  if (severity === 'all') return t('logs.filter.all');
  if (severity === 'error') return t('logs.filter.error');
  if (severity === 'warn') return t('logs.filter.warn');
  if (severity === 'info') return t('logs.filter.info');
  if (severity === 'debug') return t('logs.filter.debug');
  if (severity === 'trace') return t('logs.filter.trace');
  return t('logs.filter.unknown');
}

function SeverityBadge({ severity, t }: { severity: LogSeverity; t: Translator }) {
  return <span className={`severity-badge severity-${severity}`}>{severityLabel(severity, t)}</span>;
}

function TimeValue({ value, className }: { value?: string | null; className?: string }) {
  if (!value) {
    return <span className={className}>-</span>;
  }
  return (
    <time dateTime={value} title={timestampTitle(value)} className={className}>
      {formatTime(value)}
    </time>
  );
}

function gatewayGroupLabel(log: LogRow): string {
  return log.dcc_type ?? 'gateway';
}

function StatusLine({ text, error }: { text: string; error?: string }) {
  return <div className="status-bar">{error ? `Error: ${error}` : text}</div>;
}

export function LogsPanel({
  logs,
  filteredLogs,
  severityCounts,
  severityFilter,
  updatedAt,
  error,
  onSeverityFilterChange,
  onRefresh,
  t,
}: LogsPanelProps) {
  const requestLogGroups = buildRequestLogGroups(filteredLogs);
  const gatewayLogs = filteredLogs.filter((log) => !log.request_id);
  const filterOptions: LogSeverityFilter[] = ['all', ...LOG_SEVERITIES];

  return (
    <section className="panel active logs-panel">
      <div className="logs-hero">
        <div>
          <h2>{t('logs.title')}</h2>
          <p className="empty log-hint">{t('logs.description')}</p>
        </div>
        <Button type="button" size="sm" onClick={onRefresh}>
          <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
          {t('action.refresh')}
        </Button>
      </div>
      <StatusLine text={updatedAt} error={error} />
      <div className="log-severity-grid" aria-label={t('logs.filter.ariaLabel')}>
        {filterOptions.map((severity) => (
          <button
            key={severity}
            type="button"
            className={`log-severity-card ${severityFilter === severity ? 'active' : ''} severity-${severity}`}
            aria-pressed={severityFilter === severity}
            onClick={() => onSeverityFilterChange(severity)}
          >
            <span>{severityLabel(severity, t)}</span>
            <strong>{severity === 'all' ? severityCounts.total : severityCounts[severity]}</strong>
          </button>
        ))}
      </div>
      {logs.length === 0 ? <p className="empty">{t('logs.empty.none')}</p> : filteredLogs.length === 0 ? (
        <p className="empty">{t('logs.empty.search')}</p>
      ) : (
        <div className="live-log-board">
          {requestLogGroups.map((run) => {
            const severity = requestLogSeverity(run);
            return (
              <div key={run.requestId} className={`request-run severity-${severity}`}>
                <div className="run-header">
                  <div>
                    <div className="run-title">
                      {t('logs.label.request')} <span className="mono-path">{run.requestId}</span>
                    </div>
                    <div className="run-meta">
                      <TimeValue value={run.timestamp} /> · {run.dccType} · {run.tool}
                    </div>
                  </div>
                  <SeverityBadge severity={severity} t={t} />
                </div>
                <div className="run-steps">
                  {run.steps.map((log, idx) => {
                    const stepSeverity = normalizeLogSeverity(log);
                    return (
                      <div key={`${log.timestamp}-${log.source ?? ''}-${idx}`} className={`run-step severity-${stepSeverity}`}>
                        <span className={`step-dot ${stepSeverity}`} aria-hidden="true" />
                        <div className="step-body">
                          <div className="step-head">
                            <span className="step-name">{t('logs.label.step', { index: idx + 1 })}: {logStepTitle(log)}</span>
                            <TimeValue className="muted" value={log.timestamp} />
                            <span className="source-pill" data-source={log.source ?? 'contention'}>{log.source ?? 'contention'}</span>
                            <SeverityBadge severity={stepSeverity} t={t} />
                          </div>
                          <div className="step-detail">{logStepDetail(log)}</div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            );
          })}
          {gatewayLogs.length > 0 ? (
            <div className="group-block">
              <h3 className="group-title">{t('logs.section.gatewayEvents')}</h3>
              {Array.from(groupRows(gatewayLogs, gatewayGroupLabel).entries())
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([group, groupLogs]) => (
                  <div key={group} className="gateway-event-group">
                    <p className="group-meta">{group}</p>
                    {groupLogs.map((log, idx) => {
                      const severity = normalizeLogSeverity(log);
                      return (
                        <div key={`${log.timestamp}-${log.request_id ?? ''}-${idx}`} className={`log-line severity-${severity}`}>
                          <span className="source-pill" data-source={log.source ?? 'contention'}>{log.source ?? 'contention'}</span>
                          {' '}
                          <TimeValue className="muted" value={log.timestamp} />
                          {' '}
                          <SeverityBadge severity={severity} t={t} />
                          {' '}
                          {log.event ? <span className="log-event">{String(log.event)}</span> : null}
                          {' '}
                          {log.message}
                          {log.detail ? <span className="muted"> - {log.detail}</span> : null}
                        </div>
                      );
                    })}
                  </div>
                ))}
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}
