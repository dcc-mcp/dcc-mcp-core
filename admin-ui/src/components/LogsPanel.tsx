import { RiArrowRightSLine, RiRefreshLine } from '@remixicon/react';
import { Badge } from './ui/badge';
import { Button } from './ui/button';
import { type InterpolationValues, type MessageKey } from '../i18n';
import {
  buildRequestLogGroups,
  LOG_SEVERITIES,
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
  serverVersion: string | null;
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

function keyedLogRows(rows: LogRow[]) {
  const occurrences = new Map<string, number>();
  return rows.map((log) => {
    const identity = JSON.stringify(log);
    const occurrence = occurrences.get(identity) ?? 0;
    occurrences.set(identity, occurrence + 1);
    return { log, key: `${identity}:${occurrence}` };
  });
}

function severityLabel(severity: LogSeverityFilter, t: Translator): string {
  return t(`logs.filter.${severity}`);
}

function SeverityBadge({ severity, t }: { severity: LogSeverity; t: Translator }) {
  return <Badge variant="outline" className={`severity-badge severity-${severity}`}>{severityLabel(severity, t)}</Badge>;
}

function TimeValue({ value }: { value?: string | null }) {
  if (!value) return <span className="log-time">—</span>;
  return <time dateTime={value} title={timestampTitle(value)} className="log-time">{formatTime(value)}</time>;
}

function LogDetails({ log, t }: { log: LogRow; t: Translator }) {
  const metadata: Array<[MessageKey, string | null | undefined]> = [
    ['logs.label.timestamp', log.timestamp],
    ['logs.label.level', log.level],
    ['logs.label.source', log.source],
    ['logs.label.target', log.target],
    ['logs.label.thread', log.thread],
    ['logs.label.dcc', log.dcc_type],
    ['logs.label.instance', log.instance_id],
    ['logs.label.requestId', log.request_id],
    ['logs.label.tool', log.tool],
  ];
  return (
    <div className="log-entry-details">
      {log.message && <p className="log-entry-message">{log.message}</p>}
      {log.detail && <p className="log-entry-message">{log.detail}</p>}
      {log.reason && <p className="log-entry-reason">{t('logs.label.reason')}: {log.reason}</p>}
      <dl className="log-entry-metadata">
        {metadata.filter(([, value]) => value).map(([label, value]) => (
          <div key={label}><dt>{t(label)}</dt><dd>{value}</dd></div>
        ))}
      </dl>
    </div>
  );
}

function LogEntry({ log, step, t }: { log: LogRow; step?: number; t: Translator }) {
  const severity = normalizeLogSeverity(log);
  return (
    <details className={`log-entry severity-${severity}`}>
      <summary className="log-entry-summary">
        <RiArrowRightSLine className="log-disclosure-icon" aria-hidden="true" />
        <TimeValue value={log.timestamp} />
        {step == null ? <SeverityBadge severity={severity} t={t} /> : (
          <span className="log-step-status" title={severityLabel(severity, t)}>
            <span aria-hidden="true" /><span className="sr-only">{severityLabel(severity, t)}</span>
          </span>
        )}
        <span className="log-entry-title">
          {step == null ? logStepTitle(log) : `${t('logs.label.step', { index: step })}: ${logStepTitle(log)}`}
        </span>
        <span className="log-entry-excerpt">{log.message}</span>
      </summary>
      <LogDetails log={log} t={t} />
    </details>
  );
}

export function LogsPanel({
  logs, filteredLogs, severityCounts, severityFilter, serverVersion, updatedAt,
  error, onSeverityFilterChange, onRefresh, t,
}: LogsPanelProps) {
  const requestLogGroups = buildRequestLogGroups(filteredLogs);
  const gatewayLogs = filteredLogs.filter((log) => !log.request_id);
  const filterOptions: LogSeverityFilter[] = ['all', ...LOG_SEVERITIES];

  return (
    <section className="panel active logs-panel">
      <div className="log-panel-heading">
        <div>
          <h2>{t('logs.title')}</h2>
          <p className="log-panel-description">{t('logs.description')}</p>
        </div>
        <Button type="button" size="sm" variant="outline" onClick={onRefresh}>
          <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
          {t('action.refresh')}
        </Button>
      </div>
      <div className="log-panel-meta">
        <span>{t('logs.label.serverVersion')}: {serverVersion ?? '—'}</span>
        {updatedAt && <span>{updatedAt}</span>}
      </div>
      {error && <p className="log-panel-error" role="alert">{error}</p>}
      <div className="log-filter-bar" role="group" aria-label={t('logs.filter.ariaLabel')}>
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
        <span className="log-visible-count">{t('logs.label.entries', { count: filteredLogs.length })}</span>
      </div>
      {logs.length === 0 ? <p className="empty">{t('logs.empty.none')}</p> : filteredLogs.length === 0 ? (
        <p className="empty">{t('logs.empty.search')}</p>
      ) : (
        <div className="log-stream">
          {requestLogGroups.length > 0 && (
            <section className="log-stream-section" aria-label={t('logs.section.requests')}>
              <h3 className="log-section-title">{t('logs.section.requests')} <span>{requestLogGroups.length}</span></h3>
              <div className="log-request-list">
                {requestLogGroups.map((run) => (
                  <details key={run.requestId} className="log-request">
                    <summary className="log-request-summary">
                      <RiArrowRightSLine className="log-disclosure-icon" aria-hidden="true" />
                      <span className="log-request-identity">
                        <span className="log-request-title">{t('logs.label.request')} <code>{run.requestId}</code></span>
                        <span className="log-request-context">{run.dccType !== '?' && `${run.dccType} · `}{run.tool}</span>
                      </span>
                      <TimeValue value={run.timestamp} />
                      <span className="log-step-count">{t('logs.label.steps', { count: run.steps.length })}</span>
                      <SeverityBadge severity={requestLogSeverity(run)} t={t} />
                    </summary>
                    <div className="log-request-steps">
                      {keyedLogRows(run.steps).map(({ log, key }, index) => (
                        <LogEntry key={key} log={log} step={index + 1} t={t} />
                      ))}
                    </div>
                  </details>
                ))}
              </div>
            </section>
          )}
          {gatewayLogs.length > 0 && (
            <section className="log-stream-section" aria-label={t('logs.section.gatewayEvents')}>
              <h3 className="log-section-title">{t('logs.section.gatewayEvents')} <span>{gatewayLogs.length}</span></h3>
              {Array.from(groupRows(gatewayLogs, (log) => log.dcc_type ?? log.target ?? 'gateway').entries())
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([group, groupLogs]) => (
                  <div key={group} className="log-gateway-group">
                    <h4>{group}</h4>
                    <div className="log-gateway-entries">
                      {keyedLogRows(groupLogs).map(({ log, key }) => <LogEntry key={key} log={log} t={t} />)}
                    </div>
                  </div>
                ))}
            </section>
          )}
        </div>
      )}
    </section>
  );
}
