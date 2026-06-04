import { Fragment, useMemo, useState } from 'react';
import { API_BASE, PanelHeader, StatusLine } from '../../admin-ui-core';
import {
  useAnalyticsHeatmapQuery,
  useAnalyticsOverviewQuery,
  useAnalyticsTimeseriesQuery,
} from '../../hooks/queries';
import type { Translator } from '../../admin-types';

const RANGES = ['7d', '30d', '90d', '180d', '365d'] as const;

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
}: {
  active: boolean;
  t: Translator;
}) {
  const [range, setRange] = useState<string>('30d');
  const overviewQuery = useAnalyticsOverviewQuery(active, range);
  const timeseriesQuery = useAnalyticsTimeseriesQuery(active, range);
  const heatmapQuery = useAnalyticsHeatmapQuery(active, range);
  const overview = overviewQuery.data ?? null;
  const timeseries = timeseriesQuery.data ?? [];
  const heatmap = heatmapQuery.data ?? [];
  const loading = overviewQuery.isLoading || timeseriesQuery.isLoading || heatmapQuery.isLoading;
  const error =
    overviewQuery.error?.message ??
    timeseriesQuery.error?.message ??
    heatmapQuery.error?.message ??
    null;
  const refetch = () => Promise.all([
    overviewQuery.refetch(),
    timeseriesQuery.refetch(),
    heatmapQuery.refetch(),
  ]);

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
            <button className="refresh-btn" type="button" disabled={loading} onClick={() => { void refetch(); }}>
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
                <Fragment key={hl}>
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
                </Fragment>
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
