export type LogSeverity = 'error' | 'warn' | 'info' | 'debug' | 'trace' | 'unknown';
export type LogSeverityFilter = LogSeverity | 'all';

export const LOG_SEVERITIES: LogSeverity[] = ['error', 'warn', 'info', 'debug'];

export type LogRow = {
  timestamp: string;
  level: string;
  message: string;
  source?: string;
  target?: string;
  thread?: string;
  event?: string;
  dcc_type?: string;
  instance_id?: string | null;
  request_id?: string;
  tool?: string;
  success?: boolean;
  detail?: string;
  reason?: string | null;
};

export type RequestLogGroup = {
  requestId: string;
  timestamp: string;
  tool: string;
  dccType: string;
  status: string;
  success?: boolean;
  steps: LogRow[];
};

export type LogSeverityCounts = Record<LogSeverity, number> & { total: number };

function haystack(...parts: Array<string | number | boolean | null | undefined>): string {
  return parts.filter((part) => part != null).join(' ').toLowerCase();
}

export function normalizeLogRow(raw: unknown): LogRow {
  if (!raw || typeof raw !== 'object') {
    return { timestamp: '', level: '', message: '' };
  }
  const o = raw as Record<string, unknown>;
  return {
    timestamp: String(o.timestamp ?? ''),
    level: String(o.level ?? o.severity ?? ''),
    message: String(o.message ?? ''),
    source: o.source != null ? String(o.source) : undefined,
    target: o.target != null ? String(o.target) : undefined,
    thread: o.thread != null ? String(o.thread) : undefined,
    event: o.event != null ? String(o.event) : undefined,
    dcc_type: o.dcc_type != null ? String(o.dcc_type) : undefined,
    instance_id:
      o.instance_id === null || o.instance_id === undefined
        ? null
        : String(o.instance_id),
    request_id: o.request_id != null ? String(o.request_id) : undefined,
    tool: o.tool != null ? String(o.tool) : undefined,
    success: typeof o.success === 'boolean' ? o.success : undefined,
    detail: o.detail != null ? String(o.detail) : undefined,
    reason: o.reason == null ? null : String(o.reason),
  };
}

export function normalizeLogSeverity(log: LogRow): LogSeverity {
  const level = haystack(log.level);
  const text = haystack(log.message, log.event ?? '', log.reason ?? '', log.detail ?? '');
  if (log.success === false || level.includes('error') || level.includes('err') || level.includes('fatal') || text.includes('exception') || text.includes('failed')) {
    return 'error';
  }
  if (level.includes('warn') || text.includes('warning') || text.includes('timeout') || text.includes('stale')) {
    return 'warn';
  }
  if (level.includes('debug')) {
    return 'debug';
  }
  if (level.includes('trace')) {
    return 'trace';
  }
  if (level.includes('info') || log.success === true) {
    return 'info';
  }
  return 'unknown';
}

export function requestLogSeverity(run: RequestLogGroup): LogSeverity {
  const severities = run.steps.map(normalizeLogSeverity);
  if (severities.includes('error')) return 'error';
  if (severities.includes('warn')) return 'warn';
  if (severities.includes('debug')) return 'debug';
  if (severities.includes('trace')) return 'trace';
  if (severities.includes('info')) return 'info';
  return 'unknown';
}

export function isProblemLog(log: LogRow): boolean {
  const severity = normalizeLogSeverity(log);
  return severity === 'error' || severity === 'warn';
}

export function logStepTitle(log: LogRow): string {
  if (log.event) {
    return String(log.event);
  }
  if (log.tool) {
    return log.tool;
  }
  return log.source ?? 'event';
}

export function logStepDetail(log: LogRow): string {
  const parts = [log.message];
  if (log.detail) parts.push(log.detail);
  if (log.reason) parts.push(log.reason);
  return parts.filter(Boolean).join(' - ');
}

export function buildRequestLogGroups(rows: LogRow[]): RequestLogGroup[] {
  const map = new Map<string, LogRow[]>();
  for (const row of rows) {
    if (!row.request_id) {
      continue;
    }
    const bucket = map.get(row.request_id) ?? [];
    bucket.push(row);
    map.set(row.request_id, bucket);
  }
  return Array.from(map.entries())
    .map(([requestId, steps]) => {
      const sorted = [...steps].sort((a, b) => (a.timestamp || '').localeCompare(b.timestamp || ''));
      const newest = sorted[sorted.length - 1] ?? steps[0];
      return {
        requestId,
        timestamp: newest?.timestamp ?? '',
        tool: newest?.tool ?? newest?.message ?? 'unknown tool',
        dccType: newest?.dcc_type ?? '?',
        status: newest?.success === false ? 'failed' : 'ok',
        success: newest?.success,
        steps: sorted,
      };
    })
    .sort((a, b) => (b.timestamp || '').localeCompare(a.timestamp || ''));
}

export function summarizeLogSeverity(rows: LogRow[]): LogSeverityCounts {
  const counts: LogSeverityCounts = {
    total: rows.length,
    error: 0,
    warn: 0,
    info: 0,
    debug: 0,
    trace: 0,
    unknown: 0,
  };
  for (const row of rows) {
    counts[normalizeLogSeverity(row)] += 1;
  }
  return counts;
}

export function filterLogs(rows: LogRow[], query: string, severity: LogSeverityFilter): LogRow[] {
  const q = query.trim().toLowerCase();
  return rows.filter((row) => {
    const rowSeverity = normalizeLogSeverity(row);
    if (severity !== 'all' && rowSeverity !== severity) {
      return false;
    }
    if (!q) {
      return true;
    }
    return haystack(
      row.timestamp,
      row.level,
      rowSeverity,
      row.message,
      row.source ?? '',
      row.target ?? '',
      row.thread ?? '',
      row.event != null ? String(row.event) : '',
      row.dcc_type ?? '',
      row.instance_id != null ? String(row.instance_id) : '',
      row.request_id ?? '',
      row.tool ?? '',
      row.detail ?? '',
      row.reason ?? '',
    ).includes(q);
  });
}
