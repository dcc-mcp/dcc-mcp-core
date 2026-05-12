import { useCallback, useEffect, useMemo, useState } from 'react';

type Panel = 'health' | 'instances' | 'tools' | 'calls' | 'traces' | 'stats' | 'workers' | 'logs';

type HealthPayload = {
  status: string;
  instances_ready: number;
  instances_total: number;
  uptime_secs: number;
  version: string;
};

type InstanceRow = {
  id: string;
  dcc_type: string;
  status: string;
  host: string;
  port: number;
  scene: string | null;
};

type ToolRow = {
  slug: string;
  dcc_type: string;
  summary: string;
};

type CallRow = {
  timestamp: string;
  request_id: string;
  tool: string;
  dcc_type: string;
  status: string;
  success: boolean;
  error: string | null;
  duration_ms: number | null;
};

type TraceRow = {
  timestamp: string;
  request_id: string;
  tool: string;
  status: string;
  success: boolean;
  total_ms: number | null;
};

type StatsPayload = {
  range: string;
  total_calls: number;
  success_rate: number;
  p50_ms: number | null;
  p95_ms: number | null;
};

type WorkerRow = {
  instance_id: string;
  display_name: string;
  dcc_type: string;
  status: string;
  stale: boolean;
  pid: number | null;
  uptime_secs: number | null;
  version: string | null;
  adapter_version: string | null;
  cpu_percent: number | null;
  memory_bytes: number | null;
  mcp_url: string;
};

type WorkerSummary = {
  live: number;
  stale: number;
  unhealthy: number;
};

type LogRow = {
  timestamp: string;
  level: string;
  message: string;
};

const API_BASE = `${location.origin}/admin/api`;
const PANELS: { id: Panel; label: string }[] = [
  { id: 'health', label: 'Health' },
  { id: 'instances', label: 'Instances' },
  { id: 'tools', label: 'Tools' },
  { id: 'calls', label: 'Calls' },
  { id: 'traces', label: 'Traces' },
  { id: 'stats', label: 'Stats' },
  { id: 'workers', label: 'Workers' },
  { id: 'logs', label: 'Logs' },
];

async function apiJson<T>(path: string): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<T>;
}

function formatTime(value: string | null | undefined): string {
  if (!value) {
    return '-';
  }
  return new Date(value).toLocaleTimeString();
}

function formatUptime(value: number | null | undefined): string {
  if (value == null) {
    return '-';
  }
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const seconds = value % 60;
  return `${hours}h ${minutes}m ${seconds}s`;
}

function formatBytes(value: number | null | undefined): string {
  if (value == null) {
    return '-';
  }
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let index = 0;
  let size = value;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size.toFixed(1)} ${units[index]}`;
}

function statusClass(value: string): string {
  const status = value.toLowerCase();
  if (status.includes('ok') || status.includes('success') || status.includes('ready') || status.includes('available') || status.includes('busy')) {
    return 'badge badge-ok';
  }
  if (status.includes('stale') || status.includes('booting') || status.includes('warn')) {
    return 'badge badge-warn';
  }
  return 'badge badge-err';
}

function StatusBadge({ value }: { value: string }) {
  return <span className={statusClass(value)}>{value}</span>;
}

function StatusLine({ text, error }: { text: string; error?: string }) {
  return <div className="status-bar">{error ? `Error: ${error}` : text}</div>;
}

function HealthCard({ tone, label, value }: { tone?: 'ok' | 'warn'; label: string; value: string | number }) {
  return (
    <div className={`health-card ${tone ?? ''}`}>
      <div className="label">{label}</div>
      <div className="value">{value}</div>
    </div>
  );
}

function EmptyRow({ columns, children }: { columns: number; children: string }) {
  return (
    <tr>
      <td colSpan={columns} className="empty">{children}</td>
    </tr>
  );
}

function App() {
  const [activePanel, setActivePanel] = useState<Panel>('health');
  const [health, setHealth] = useState<HealthPayload | null>(null);
  const [instances, setInstances] = useState<InstanceRow[]>([]);
  const [tools, setTools] = useState<ToolRow[]>([]);
  const [calls, setCalls] = useState<CallRow[]>([]);
  const [traces, setTraces] = useState<TraceRow[]>([]);
  const [stats, setStats] = useState<StatsPayload | null>(null);
  const [statsRange, setStatsRange] = useState('24h');
  const [workers, setWorkers] = useState<WorkerRow[]>([]);
  const [workerSummary, setWorkerSummary] = useState<WorkerSummary>({ live: 0, stale: 0, unhealthy: 0 });
  const [logs, setLogs] = useState<LogRow[]>([]);
  const [traceDetail, setTraceDetail] = useState<string>('Select a trace row for detail.');
  const [callDetail, setCallDetail] = useState<string>('Select a call row for trace detail.');
  const [updatedAt, setUpdatedAt] = useState<Record<Panel, string>>({
    health: 'Loading…',
    instances: 'Loading…',
    tools: 'Loading…',
    calls: 'Loading…',
    traces: 'Loading…',
    stats: 'Loading…',
    workers: 'Loading…',
    logs: 'Loading…',
  });
  const [errors, setErrors] = useState<Partial<Record<Panel, string>>>({});

  const markUpdated = useCallback((panel: Panel, text: string) => {
    setUpdatedAt((current) => ({ ...current, [panel]: text }));
    setErrors((current) => ({ ...current, [panel]: undefined }));
  }, []);

  const markError = useCallback((panel: Panel, error: unknown) => {
    setErrors((current) => ({ ...current, [panel]: error instanceof Error ? error.message : String(error) }));
  }, []);

  const fetchHealth = useCallback(async () => {
    try {
      const payload = await apiJson<HealthPayload>('/health');
      setHealth(payload);
      markUpdated('health', `Last updated: ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('health', error);
    }
  }, [markError, markUpdated]);

  const fetchInstances = useCallback(async () => {
    try {
      const payload = await apiJson<{ instances: InstanceRow[] }>('/instances');
      setInstances(payload.instances);
      markUpdated('instances', `${payload.instances.length} instance(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('instances', error);
    }
  }, [markError, markUpdated]);

  const fetchTools = useCallback(async () => {
    try {
      const payload = await apiJson<{ tools: ToolRow[] }>('/tools');
      setTools(payload.tools);
      markUpdated('tools', `${payload.tools.length} tool(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('tools', error);
    }
  }, [markError, markUpdated]);

  const fetchCalls = useCallback(async () => {
    try {
      const payload = await apiJson<{ calls: CallRow[] }>('/calls');
      setCalls(payload.calls);
      markUpdated('calls', `${payload.calls.length} call(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('calls', error);
    }
  }, [markError, markUpdated]);

  const fetchTraces = useCallback(async () => {
    try {
      const payload = await apiJson<{ traces: TraceRow[] }>('/traces?limit=200');
      setTraces(payload.traces);
      markUpdated('traces', `${payload.traces.length} trace(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('traces', error);
    }
  }, [markError, markUpdated]);

  const fetchStats = useCallback(async () => {
    try {
      const payload = await apiJson<StatsPayload>(`/stats?range=${encodeURIComponent(statsRange)}`);
      setStats(payload);
      markUpdated('stats', `Range ${payload.range} — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('stats', error);
    }
  }, [markError, markUpdated, statsRange]);

  const fetchWorkers = useCallback(async () => {
    try {
      const payload = await apiJson<{ workers: WorkerRow[]; summary: WorkerSummary }>('/workers');
      setWorkers(payload.workers);
      setWorkerSummary(payload.summary);
      markUpdated(
        'workers',
        `${payload.workers.length} worker(s) (live ${payload.summary.live}, stale ${payload.summary.stale}, unhealthy ${payload.summary.unhealthy}) — ${new Date().toLocaleTimeString()}`,
      );
    } catch (error) {
      markError('workers', error);
    }
  }, [markError, markUpdated]);

  const fetchLogs = useCallback(async () => {
    try {
      const payload = await apiJson<{ logs: LogRow[] }>('/logs');
      setLogs(payload.logs);
      markUpdated('logs', `${payload.logs.length} event(s) — ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      markError('logs', error);
    }
  }, [markError, markUpdated]);

  const fetchTraceInto = useCallback(async (requestId: string, target: 'call' | 'trace') => {
    try {
      const payload = await apiJson<unknown>(`/traces/${encodeURIComponent(requestId)}`);
      const detail = JSON.stringify(payload, null, 2);
      if (target === 'call') {
        setCallDetail(detail);
      } else {
        setTraceDetail(detail);
      }
    } catch (error) {
      const detail = `Error: ${error instanceof Error ? error.message : String(error)}`;
      if (target === 'call') {
        setCallDetail(detail);
      } else {
        setTraceDetail(detail);
      }
    }
  }, []);

  const fetchPanel = useCallback((panel: Panel) => {
    if (panel === 'health') void fetchHealth();
    if (panel === 'instances') void fetchInstances();
    if (panel === 'tools') void fetchTools();
    if (panel === 'calls') void fetchCalls();
    if (panel === 'traces') void fetchTraces();
    if (panel === 'stats') void fetchStats();
    if (panel === 'workers') void fetchWorkers();
    if (panel === 'logs') void fetchLogs();
  }, [fetchCalls, fetchHealth, fetchInstances, fetchLogs, fetchStats, fetchTools, fetchTraces, fetchWorkers]);

  const statsJson = useMemo(() => JSON.stringify(stats, null, 2), [stats]);

  useEffect(() => {
    fetchPanel(activePanel);
    const timer = window.setInterval(() => fetchPanel(activePanel), 5000);
    return () => window.clearInterval(timer);
  }, [activePanel, fetchPanel]);

  return (
    <div className="app-shell">
      <nav className="side-rail" aria-label="Admin navigation">
        <div className="brand-lockup">
          <div className="brand-accent" aria-hidden="true" />
          <div className="brand-text">
            <h1>DCC-MCP Gateway</h1>
            <p className="brand-tag">Admin console</p>
          </div>
        </div>
        <div className="nav-links">
          {PANELS.map((panel) => (
            <button
              key={panel.id}
              className={panel.id === activePanel ? 'nav-link active' : 'nav-link'}
              type="button"
              onClick={() => setActivePanel(panel.id)}
            >
              {panel.label}
            </button>
          ))}
        </div>
      </nav>
      <main className="main-stage">
        {activePanel === 'health' && (
          <section className="panel active">
            <h2>Health</h2>
            <StatusLine text={updatedAt.health} error={errors.health} />
            <div className="health-grid">
              <HealthCard tone={health?.status === 'ok' ? 'ok' : 'warn'} label="Status" value={health?.status ?? '?'} />
              <HealthCard label="Uptime" value={formatUptime(health?.uptime_secs)} />
              <HealthCard tone={health && health.instances_ready > 0 ? 'ok' : 'warn'} label="Ready" value={`${health?.instances_ready ?? 0} / ${health?.instances_total ?? 0}`} />
              <HealthCard label="Version" value={health?.version ?? '?'} />
            </div>
            <button className="refresh-btn" type="button" onClick={fetchHealth}>Refresh</button>
          </section>
        )}

        {activePanel === 'instances' && (
          <section className="panel active">
            <h2>Instances</h2>
            <StatusLine text={updatedAt.instances} error={errors.instances} />
            <table>
              <thead><tr><th>ID</th><th>DCC</th><th>Status</th><th>Address</th><th>Scene</th></tr></thead>
              <tbody>
                {instances.length === 0 ? <EmptyRow columns={5}>No instances registered.</EmptyRow> : instances.map((instance) => (
                  <tr key={instance.id}>
                    <td>{instance.id.slice(0, 8)}</td>
                    <td>{instance.dcc_type}</td>
                    <td><StatusBadge value={instance.status} /></td>
                    <td>{instance.host}:{instance.port}</td>
                    <td>{instance.scene ?? '-'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <button className="refresh-btn" type="button" onClick={fetchInstances}>Refresh</button>
          </section>
        )}

        {activePanel === 'tools' && (
          <section className="panel active">
            <h2>Tools</h2>
            <StatusLine text={updatedAt.tools} error={errors.tools} />
            <table>
              <thead><tr><th>Slug</th><th>DCC</th><th>Summary</th></tr></thead>
              <tbody>
                {tools.length === 0 ? <EmptyRow columns={3}>No tools registered.</EmptyRow> : tools.map((tool) => (
                  <tr key={tool.slug}>
                    <td>{tool.slug}</td>
                    <td>{tool.dcc_type}</td>
                    <td>{tool.summary.slice(0, 120)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <button className="refresh-btn" type="button" onClick={fetchTools}>Refresh</button>
          </section>
        )}

        {activePanel === 'calls' && (
          <section className="panel active">
            <h2>Recent Calls</h2>
            <StatusLine text={updatedAt.calls} error={errors.calls} />
            <table>
              <thead><tr><th>Time</th><th>Request</th><th>Tool</th><th>DCC</th><th>Status</th><th>Error</th><th>ms</th><th>Detail</th></tr></thead>
              <tbody>
                {calls.length === 0 ? <EmptyRow columns={8}>No recent calls. AuditMiddleware may not be active.</EmptyRow> : calls.map((call) => (
                  <tr key={call.request_id}>
                    <td>{formatTime(call.timestamp)}</td>
                    <td>
                      <button className="refresh-btn" type="button" title={call.request_id} onClick={() => { setActivePanel('traces'); void fetchTraceInto(call.request_id, 'trace'); }}>
                        {call.request_id.slice(0, 12)}
                      </button>
                    </td>
                    <td>{call.tool}</td>
                    <td>{call.dcc_type}</td>
                    <td><StatusBadge value={call.status} /></td>
                    <td title={call.error ?? ''}>{call.error ? call.error.slice(0, 80) : '-'}</td>
                    <td>{call.duration_ms ?? '-'}</td>
                    <td><button className="refresh-btn" type="button" onClick={() => void fetchTraceInto(call.request_id, 'call')}>Expand</button></td>
                  </tr>
                ))}
              </tbody>
            </table>
            <pre className="empty">{callDetail}</pre>
            <button className="refresh-btn" type="button" onClick={fetchCalls}>Refresh</button>
          </section>
        )}

        {activePanel === 'traces' && (
          <section className="panel active" data-panel="traces">
            <h2>Traces</h2>
            <StatusLine text={updatedAt.traces} error={errors.traces} />
            <table>
              <thead><tr><th>Time</th><th>Request</th><th>Tool</th><th>Status</th><th>Total ms</th></tr></thead>
              <tbody>
                {traces.length === 0 ? <EmptyRow columns={5}>No traces recorded.</EmptyRow> : traces.map((trace) => (
                  <tr
                    key={trace.request_id}
                    className="trace-row"
                    onClick={() => void fetchTraceInto(trace.request_id, 'trace')}
                  >
                    <td>{formatTime(trace.timestamp)}</td>
                    <td>{trace.request_id}</td>
                    <td>{trace.tool}</td>
                    <td><StatusBadge value={trace.status} /></td>
                    <td>{trace.total_ms ?? '-'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <pre className="empty">{traceDetail}</pre>
            <button className="refresh-btn" type="button" onClick={fetchTraces}>Refresh</button>
          </section>
        )}

        {activePanel === 'stats' && (
          <section className="panel active" data-panel="stats">
            <h2>Stats</h2>
            <StatusLine text={updatedAt.stats} error={errors.stats} />
            <label className="range-label">
              Range
              <select value={statsRange} onChange={(event) => setStatsRange(event.target.value)}>
                <option value="1h">1h</option>
                <option value="24h">24h</option>
                <option value="7d">7d</option>
              </select>
            </label>
            <div className="health-grid">
              <HealthCard label="Calls" value={stats?.total_calls ?? 0} />
              <HealthCard label="Success %" value={stats ? stats.success_rate.toFixed(1) : '0.0'} />
              <HealthCard label="p50 ms" value={stats?.p50_ms ?? '-'} />
              <HealthCard label="p95 ms" value={stats?.p95_ms ?? '-'} />
            </div>
            <pre>{statsJson}</pre>
            <button className="refresh-btn" type="button" onClick={fetchStats}>Refresh</button>
          </section>
        )}

        {activePanel === 'workers' && (
          <section className="panel active">
            <h2>Workers</h2>
            <StatusLine text={updatedAt.workers} error={errors.workers} />
            <div className="workers-grid">
              {workers.length === 0 ? <p className="empty">No workers registered.</p> : workers.map((worker) => (
                <div key={worker.instance_id} className={`worker-card ${worker.stale ? 'stale' : statusClass(worker.status).replace('badge badge-', '')}`}>
                  <div className="wname">{worker.display_name} <span>{worker.instance_id.slice(0, 8)}</span></div>
                  <div className="wkv">
                    <span>DCC</span><span>{worker.dcc_type}</span>
                    <span>Status</span><span><StatusBadge value={worker.status} /></span>
                    <span>PID</span><span>{worker.pid ?? '-'}</span>
                    <span>Uptime</span><span>{formatUptime(worker.uptime_secs)}</span>
                    <span>Version</span><span>{worker.version ?? '-'}</span>
                    <span>Adapter</span><span>{worker.adapter_version ?? '-'}</span>
                    <span>CPU%</span><span>{worker.cpu_percent == null ? '-' : worker.cpu_percent.toFixed(1)}</span>
                    <span>Memory</span><span>{formatBytes(worker.memory_bytes)}</span>
                    <span>MCP URL</span><span>{worker.mcp_url}</span>
                  </div>
                </div>
              ))}
            </div>
            <div className="status-bar">Summary: live {workerSummary.live}, stale {workerSummary.stale}, unhealthy {workerSummary.unhealthy}</div>
            <button className="refresh-btn" type="button" onClick={fetchWorkers}>Refresh</button>
          </section>
        )}

        {activePanel === 'logs' && (
          <section className="panel active">
            <h2>Event Log</h2>
            <StatusLine text={updatedAt.logs} error={errors.logs} />
            {logs.length === 0 ? <p className="empty">No events recorded.</p> : logs.map((log) => (
              <div key={`${log.timestamp}-${log.message}`} className="log-line">
                <span className="muted">{formatTime(log.timestamp)}</span> <span className="warn-text">[{log.level}]</span> {log.message}
              </div>
            ))}
            <button className="refresh-btn" type="button" onClick={fetchLogs}>Refresh</button>
          </section>
        )}
      </main>
    </div>
  );
}

export default App;
