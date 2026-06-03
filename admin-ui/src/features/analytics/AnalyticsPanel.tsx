import { useCallback, useEffect, useMemo, useState } from 'react';
import { apiJson, API_BASE, PanelHeader, StatusLine } from '../../admin-ui-core';
import type { Translator } from '../../admin-types';

// ── types ─────────────────────────────────────────────────────────────────

type AnalyticsOverview = {
  range: string;
  period_start: string;
  period_end: string;
  kpi: {
    calls_total: number;
    calls_failed: number;
    failure_rate_pct: string;
    success_rate_pct: string;
    tokens_input_total: number;
    tokens_output_total: number;
    tokens_response_saved: number;
    tokens_total: number;
    llm_tokens_total: number;
    avg_duration_ms: string;
    avg_tokens_per_call: string;
    unique_instances: number;
    unique_agents: number;
  };
  top_tools: { name: string; calls: number; failures: number; success_rate_pct: number; avg_duration_ms: number }[];
  daily_series: { date: string; dcc_type: string; calls: number; failures: number; tokens_input: number; tokens_output: number }[];
};

type TimeseriesPoint = {
  date: string;
  calls: number;
  failures: number;
  tokens_input: number;
  tokens_output: number;
  avg_duration_ms: string;
};

type HeatmapCell = {
  weekday: number;
  hour: number;
  calls: number;
  failures: number;
  avg_duration_ms: number;
  tokens_total: number;
};

// ── hook ──────────────────────────────────────────────────────────────────

const RANGES = ['7d', '30d', '90d', '180d', '365d'] as const;

function useAnalytics(range: string) {
  const [overview, setOverview] = useState<AnalyticsOverview | null>(null);
  const [timeseries, setTimeseries] = useState<TimeseriesPoint[]>([]);
  const [heatmap, setHeatmap] = useState<HeatmapCell[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchAll = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [ov, ts, hm] = await Promise.all([
        apiJson<AnalyticsOverview>(`/analytics/overview?range=${encodeURIComponent(range)}`),
        apiJson<{ series: TimeseriesPoint[] }>(`/analytics/timeseries?range=${encodeURIComponent(range)}&granularity=day`),
        apiJson<{ heatmap: HeatmapCell[] }>(`/analytics/heatmap?range=${encodeURIComponent(range)}`),
      ]);
      setOverview(ov);
      setTimeseries(ts.series);
      setHeatmap(hm.heatmap);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [range]);

  useEffect(() => { void fetchAll(); }, [fetchAll]);

  return { overview, timeseries, heatmap, loading, error, refetch: fetchAll };
}

// ── helpers ───────────────────────────────────────────────────────────────

function fmt(n: number | undefined): string {
  if (n == null || n === 0) return '—';
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1000).toFixed(1)}K`;
  return String(n);
}

const WEEKDAY_LABELS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const HOUR_LABELS = Array.from({ length: 24 }, (_, i) => `${i.toString().padStart(2, '0')}:00`);

function heatmapColor(calls: number, maxCalls: number): string {
  if (maxCalls === 0) return '#1e1e2e';
  const ratio = calls / maxCalls;
  // Blue gradient: dark blue -> bright cyan
  const r = Math.round(30 + ratio * 50);
  const g = Math.round(30 + ratio * 80);
  const b = Math.round(100 + ratio * 155);
  return `rgb(${r},${g},${b})`;
}

// ── KPI card ──────────────────────────────────────────────────────────────

function KpiCard({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className="metric-tile">
      <div className="metric-label">{label}</div>
      <div className="metric-value">{value}</div>
      {detail ? <div className="metric-detail">{detail}</div> : null}
    </div>
  );
}

// ── Mini bar chart ────────────────────────────────────────────────────────

function MiniBarChart({ data, maxVal, height }: { data: { label: string; value: number; color?: string }[]; maxVal: number; height: number }) {
  return (
    <div className="mini-bar-chart" style={{ display: 'flex', alignItems: 'flex-end', gap: 2, height, padding: '4px 0' }}>
      {data.map((d, i) => (
        <div key={i} style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', height: '100%', justifyContent: 'flex-end' }}>
          <div
            style={{
              width: '100%',
              height: maxVal > 0 ? `${(d.value / maxVal) * 100}%` : '0%',
              backgroundColor: d.color ?? '#6366f1',
              borderRadius: '2px 2px 0 0',
              minHeight: d.value > 0 ? 2 : 0,
              transition: 'height 0.3s ease',
            }}
            title={`${d.label}: ${d.value}`}
          />
        </div>
      ))}
    </div>
  );
}

// ── Panel ─────────────────────────────────────────────────────────────────

export function AnalyticsPanel({
  active,
  t,
  onUpdated,
  onError,
}: {
  active: boolean;
  t: Translator;
  onUpdated: (text: string) => void;
  onError: (error: unknown) => void;
}) {
  const [range, setRange] = useState<string>('30d');
  const { overview, timeseries, heatmap, loading, error, refetch } = useAnalytics(range);

  useEffect(() => {
    if (!active) return;
    refetch();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, range]);

  useEffect(() => {
    if (error) {
      onError(new Error(error));
    }
  }, [error, onError]);

  useEffect(() => {
    if (overview) {
      onUpdated(`Last updated: ${new Date().toLocaleTimeString()}`);
    }
  }, [overview, onUpdated]);

  const maxDayCalls = useMemo(() => Math.max(...timeseries.map((p) => p.calls), 1), [timeseries]);
  const maxHeatCalls = useMemo(() => Math.max(...heatmap.map((c) => c.calls), 1), [heatmap]);

  if (!active) return null;

  return (
    <section className="panel active analytics-panel" data-panel="analytics">
      <PanelHeader
        title={t('analytics.title')}
        action={
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <select className="range-select" value={range} onChange={(e) => setRange(e.target.value)}>
              {RANGES.map((r) => (
                <option key={r} value={r}>{t(`analytics.range.${r}` as any)}</option>
              ))}
            </select>
            <button className="refresh-btn" type="button" disabled={loading} onClick={refetch}>
              {t('analytics.action.refresh')}
            </button>
          </div>
        }
      />
      <StatusLine text={loading ? t('analytics.status.loading') : (error ? t('analytics.status.error') : '')} error={error ?? undefined} />
      <p className="empty log-hint">{t('analytics.description')}</p>

      {overview ? (
        <>
          {/* KPI grid */}
          <div className="metric-grid">
            <KpiCard label={t('analytics.kpi.callsTotal')} value={fmt(overview.kpi.calls_total)} />
            <KpiCard label={t('analytics.kpi.successRate')} value={`${overview.kpi.success_rate_pct}%`} detail={`${overview.kpi.calls_failed} ${t('analytics.kpi.failedCalls').toLowerCase()}`} />
            <KpiCard label={t('analytics.kpi.tokensInput')} value={fmt(overview.kpi.tokens_input_total)} detail={`Output: ${fmt(overview.kpi.tokens_output_total)}`} />
            <KpiCard label={t('analytics.kpi.tokensSaved')} value={fmt(overview.kpi.tokens_response_saved)} />
            <KpiCard label={t('analytics.kpi.avgDuration')} value={`${overview.kpi.avg_duration_ms}ms`} detail={`${overview.kpi.avg_tokens_per_call} tokens/call`} />
            <KpiCard label={t('analytics.kpi.llmTokens')} value={fmt(overview.kpi.llm_tokens_total)} detail={`${overview.kpi.unique_instances} DCC, ${overview.kpi.unique_agents} agents`} />
          </div>

          {/* Timeseries */}
          <div style={{ marginTop: 16 }}>
            <h3>{t('analytics.section.timeseries')}</h3>
            <MiniBarChart
              data={timeseries.map((p) => ({ label: p.date, value: p.calls, color: p.failures > 0 ? '#ef4444' : '#6366f1' }))}
              maxVal={maxDayCalls}
              height={120}
            />
            <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--muted)', marginTop: 4 }}>
              {timeseries.length > 0 && (
                <>
                  <span>{timeseries[0].date}</span>
                  <span>{timeseries[timeseries.length - 1].date}</span>
                </>
              )}
            </div>
          </div>

          {/* Heatmap */}
          <div style={{ marginTop: 16 }}>
            <h3>{t('analytics.section.heatmap')}</h3>
            <div className="heatmap-grid" style={{
              display: 'grid',
              gridTemplateColumns: `auto repeat(7, 1fr)`,
              gap: 2,
              fontSize: 11,
              maxWidth: 600,
            }}>
              {/* Header row */}
              <div />
              {WEEKDAY_LABELS.map((wd) => (
                <div key={wd} style={{ textAlign: 'center', color: 'var(--muted)', fontWeight: 500 }}>{wd}</div>
              ))}
              {/* Data rows */}
              {HOUR_LABELS.map((hl, h) => (
                <>
                  <div key={`hdr-${h}`} style={{ color: 'var(--muted)', textAlign: 'right', paddingRight: 4 }}>{hl}</div>
                  {Array.from({ length: 7 }, (_, wd) => {
                    const cell = heatmap.find((c) => c.weekday === wd && c.hour === h);
                    return (
                      <div
                        key={`${wd}-${h}`}
                        style={{
                          backgroundColor: heatmapColor(cell?.calls ?? 0, maxHeatCalls),
                          textAlign: 'center',
                          padding: '3px 2px',
                          borderRadius: 3,
                          color: (cell?.calls ?? 0) > maxHeatCalls * 0.5 ? '#fff' : 'var(--text)',
                          fontSize: 10,
                        }}
                        title={cell ? `${WEEKDAY_LABELS[wd]} ${hl}: ${cell.calls} calls, ${cell.failures} failures, avg ${cell.avg_duration_ms.toFixed(0)}ms` : undefined}
                      >
                        {cell?.calls ? (cell.calls > 99 ? '…' : cell.calls) : ''}
                      </div>
                    );
                  })}
                </>
              ))}
            </div>
          </div>

          {/* Top tools */}
          <div style={{ marginTop: 16 }}>
            <h3>{t('analytics.section.topTools')}</h3>
            <table className="admin-table" style={{ maxWidth: 600 }}>
              <thead>
                <tr>
                  <th>Tool</th>
                  <th>Calls</th>
                  <th>Failures</th>
                  <th>Success Rate</th>
                  <th>Avg Duration</th>
                </tr>
              </thead>
              <tbody>
                {overview.top_tools.map((tool) => (
                  <tr key={tool.name}>
                    <td style={{ fontFamily: 'var(--mono)', fontSize: 13 }}>{tool.name}</td>
                    <td>{tool.calls}</td>
                    <td>{tool.failures}</td>
                    <td>{tool.success_rate_pct.toFixed(1)}%</td>
                    <td>{tool.avg_duration_ms.toFixed(0)}ms</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* Export buttons */}
          <div style={{ marginTop: 16, display: 'flex', gap: 8 }}>
            <a
              className="refresh-btn"
              href={`${API_BASE}/analytics/export?range=${encodeURIComponent(range)}&format=csv`}
              download
              style={{ textDecoration: 'none' }}
            >
              {t('analytics.action.exportCsv')}
            </a>
            <a
              className="refresh-btn"
              href={`${API_BASE}/analytics/export?range=${encodeURIComponent(range)}&format=json`}
              download
              style={{ textDecoration: 'none' }}
            >
              {t('analytics.action.exportJsonl')}
            </a>
          </div>
        </>
      ) : (
        !loading ? <p className="empty">{t('analytics.empty.noData')}</p> : null
      )}
    </section>
  );
}
