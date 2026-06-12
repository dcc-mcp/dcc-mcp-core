import { useMemo, useState, type CSSProperties } from 'react';
import { RiDownloadCloudLine, RiRefreshLine } from '@remixicon/react';
import { API_BASE, formatDurationMs, PanelHeader, StatusLine } from '../../admin-ui-core';
import { Button } from '../../components/ui/button';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../components/ui/select';
import {
  useAnalyticsOverviewQuery,
  useAnalyticsTimeseriesQuery,
} from '../../hooks/queries';
import type { Translator } from '../../admin-types';
import type { SupportedLocale } from '../../i18n';
import './analytics.css';

const RANGES = ['7d', '30d', '90d', '180d', '365d'] as const;
type AnalyticsRange = typeof RANGES[number];
const TOKEN_CALENDAR_MODES = ['daily', 'weekly', 'cumulative'] as const;
type TokenCalendarMode = typeof TOKEN_CALENDAR_MODES[number];

type TokenCalendarDay = {
  key: string;
  date: Date;
  calls: number;
  tokens: number;
  weeklyTokens: number;
  cumulativeTokens: number;
  outside: boolean;
};

type TokenCalendar = {
  weeks: TokenCalendarDay[][];
  monthLabels: { weekIndex: number; span: number; label: string }[];
  maxTokensByMode: Record<TokenCalendarMode, number>;
};

type TokenActivitySummary = {
  activeDays: number;
  avgTokensPerActiveDay: number;
  currentStreak: number;
  failureDays: number;
  longestStreak: number;
  peakDayLabel: string;
  peakDayTokens: number;
};

// ── helpers ───────────────────────────────────────────────────────────────

function fmt(n: number | undefined): string {
  if (n == null || n === 0) return '—';
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1000).toFixed(1)}K`;
  return String(n);
}

function ratioPercent(value: number, maxValue: number, minWhenNonZero = 0): string {
  if (maxValue <= 0 || value <= 0) return '0%';
  return `${Math.max(minWhenNonZero, Math.round((value / maxValue) * 100))}%`;
}

function rangeDays(range: string): number {
  const parsed = Number.parseInt(range, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 30;
}

function dateFromAnalytics(value: string | undefined): Date | null {
  if (!value) return null;
  const datePart = value.slice(0, 10);
  const parts = datePart.split('-').map((part) => Number.parseInt(part, 10));
  if (parts.length !== 3 || parts.some((part) => !Number.isFinite(part))) return null;
  const [year, month, day] = parts;
  return new Date(year, month - 1, day);
}

function dateKey(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function addDays(date: Date, days: number): Date {
  const next = new Date(date);
  next.setDate(next.getDate() + days);
  return next;
}

function startOfWeek(date: Date): Date {
  return addDays(date, -date.getDay());
}

function endOfWeek(date: Date): Date {
  return addDays(startOfWeek(date), 6);
}

function monthLabel(date: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { month: 'short' }).format(date);
}

function dayLabel(date: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' }).format(date);
}

function numericDurationMs(value: string | number | null | undefined): number {
  if (typeof value === 'number') return Number.isFinite(value) ? value : 0;
  if (!value) return 0;
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function calendarTokensForMode(day: TokenCalendarDay, mode: TokenCalendarMode): number {
  if (mode === 'weekly') return day.weeklyTokens;
  if (mode === 'cumulative') return day.cumulativeTokens;
  return day.tokens;
}

function calendarLevel(value: number, maxValue: number): 0 | 1 | 2 | 3 | 4 | 5 {
  if (value <= 0 || maxValue <= 0) return 0;
  const ratio = value / maxValue;
  if (ratio < 0.18) return 1;
  if (ratio < 0.36) return 2;
  if (ratio < 0.58) return 3;
  if (ratio < 0.78) return 4;
  return 5;
}

function buildTokenCalendar(
  series: { date: string; calls: number; tokens_input: number; tokens_output: number }[],
  range: string,
  periodEnd?: string,
  locale?: string,
): TokenCalendar {
  const end = dateFromAnalytics(periodEnd) ?? new Date();
  const start = addDays(end, -(rangeDays(range) - 1));
  const valueByDate = new Map(
    series.map((point) => [
      point.date,
      {
        calls: point.calls,
        tokens: (point.tokens_input ?? 0) + (point.tokens_output ?? 0),
      },
    ]),
  );
  const weeks: TokenCalendarDay[][] = [];
  let cursor = startOfWeek(start);
  const displayEnd = endOfWeek(end);
  while (cursor <= displayEnd) {
    const week: TokenCalendarDay[] = [];
    for (let i = 0; i < 7; i += 1) {
      const key = dateKey(cursor);
      const value = valueByDate.get(key);
      week.push({
        key,
        date: new Date(cursor),
        calls: value?.calls ?? 0,
        tokens: value?.tokens ?? 0,
        weeklyTokens: 0,
        cumulativeTokens: 0,
        outside: cursor < start || cursor > end,
      });
      cursor = addDays(cursor, 1);
    }
    weeks.push(week);
  }

  let cumulativeTokens = 0;
  weeks.forEach((week) => {
    const weeklyTokens = week.reduce((total, day) => total + (day.outside ? 0 : day.tokens), 0);
    week.forEach((day) => {
      day.weeklyTokens = day.outside ? 0 : weeklyTokens;
      if (!day.outside) {
        cumulativeTokens += day.tokens;
      }
      day.cumulativeTokens = day.outside ? 0 : cumulativeTokens;
    });
  });

  const starts: { weekIndex: number; label: string }[] = [];
  const seenMonths = new Set<string>();
  weeks.forEach((week, weekIndex) => {
    const firstInMonth = week.find((day) => !day.outside && day.date.getDate() <= 7);
    if (!firstInMonth) return;
    const key = `${firstInMonth.date.getFullYear()}-${firstInMonth.date.getMonth()}`;
    if (!seenMonths.has(key)) {
      starts.push({ weekIndex, label: monthLabel(firstInMonth.date, locale) });
      seenMonths.add(key);
    }
  });
  if (starts.length === 0) {
    const firstWeekIndex = weeks.findIndex((week) => week.some((day) => !day.outside));
    const firstInRange = firstWeekIndex >= 0 ? weeks[firstWeekIndex].find((day) => !day.outside) : null;
    if (firstInRange) {
      starts.push({ weekIndex: firstWeekIndex, label: monthLabel(firstInRange.date, locale) });
    }
  }
  const monthLabels = starts.map((entry, index) => ({
    ...entry,
    span: (starts[index + 1]?.weekIndex ?? weeks.length) - entry.weekIndex,
  }));

  const inRangeDays = weeks.flat().filter((day) => !day.outside);
  const maxTokensByMode = TOKEN_CALENDAR_MODES.reduce((acc, mode) => {
    acc[mode] = Math.max(...inRangeDays.map((day) => calendarTokensForMode(day, mode)), 1);
    return acc;
  }, {} as Record<TokenCalendarMode, number>);

  return { weeks, monthLabels, maxTokensByMode };
}

function buildTokenActivitySummary(
  calendar: TokenCalendar,
  timeseries: { date: string; failures: number }[],
  locale?: string,
): TokenActivitySummary {
  const days = calendar.weeks.flat().filter((day) => !day.outside);
  const activeDays = days.filter((day) => day.calls > 0 || day.tokens > 0).length;
  const totalTokens = days.reduce((total, day) => total + day.tokens, 0);
  const peakDay = days.reduce<TokenCalendarDay | null>(
    (best, day) => (!best || day.tokens > best.tokens ? day : best),
    null,
  );

  let longestStreak = 0;
  let currentStreak = 0;
  days.forEach((day) => {
    if (day.calls > 0 || day.tokens > 0) {
      currentStreak += 1;
      longestStreak = Math.max(longestStreak, currentStreak);
    } else {
      currentStreak = 0;
    }
  });
  const currentActiveStreak = days
    .slice()
    .reverse()
    .reduce(
      (state, day) => {
        if (state.done) return state;
        if (day.calls > 0 || day.tokens > 0) {
          return { count: state.count + 1, done: false };
        }
        return { count: state.count, done: true };
      },
      { count: 0, done: false },
    ).count;

  return {
    activeDays,
    avgTokensPerActiveDay: activeDays ? Math.round(totalTokens / activeDays) : 0,
    currentStreak: currentActiveStreak,
    failureDays: timeseries.filter((point) => point.failures > 0).length,
    longestStreak,
    peakDayLabel: peakDay && peakDay.tokens > 0 ? dayLabel(peakDay.date, locale) : '—',
    peakDayTokens: peakDay?.tokens ?? 0,
  };
}

function toolInitial(name: string): string {
  const display = name.split('__').pop() ?? name;
  const compact = display.replace(/[^A-Za-z0-9]/g, '');
  return (compact.slice(0, 2) || 'TL').toUpperCase();
}

function profileInitials(value: string): string {
  const compact = value
    .split(/[\s_-]+/)
    .map((part) => part.charAt(0))
    .join('')
    .replace(/[^A-Za-z0-9]/g, '');
  return (compact.slice(0, 2) || 'DM').toUpperCase();
}

// ── KPI card ──────────────────────────────────────────────────────────────

function KpiCard({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className="metric-tile">
      <div className="metric-value">{value}</div>
      <div className="metric-label">{label}</div>
      {detail ? <div className="metric-detail">{detail}</div> : null}
    </div>
  );
}

// ── Mini bar chart ────────────────────────────────────────────────────────

function MiniBarChart({ data, maxVal }: { data: { label: string; value: number; tone?: 'ok' | 'err' }[]; maxVal: number }) {
  return (
    <div className="analytics-mini-bar-chart">
      {data.map((d) => (
        <div key={d.label} className="analytics-mini-bar-slot">
          <span
            className={`analytics-mini-bar${d.value > 0 ? ' has-value' : ''}${d.tone === 'err' ? ' is-failed' : ''}`}
            style={{ '--analytics-bar-height': ratioPercent(d.value, maxVal) } as CSSProperties}
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
  locale,
  t,
}: {
  active: boolean;
  locale: SupportedLocale;
  t: Translator;
}) {
  const [range, setRange] = useState<AnalyticsRange>('365d');
  const [calendarMode, setCalendarMode] = useState<TokenCalendarMode>('daily');
  const overviewQuery = useAnalyticsOverviewQuery(active, range);
  const timeseriesQuery = useAnalyticsTimeseriesQuery(active, range);
  const overview = overviewQuery.data ?? null;
  const timeseries = timeseriesQuery.data ?? [];
  const loading = overviewQuery.isLoading || timeseriesQuery.isLoading;
  const error =
    overviewQuery.error?.message ??
    timeseriesQuery.error?.message ??
    null;
  const refetch = () => Promise.all([
    overviewQuery.refetch(),
    timeseriesQuery.refetch(),
  ]);

  const maxDayCalls = useMemo(() => Math.max(...timeseries.map((p) => p.calls), 1), [timeseries]);
  const tokenCalendar = useMemo(
    () => buildTokenCalendar(timeseries, range, overview?.period_end, locale),
    [locale, overview?.period_end, range, timeseries],
  );
  const tokenActivitySummary = useMemo(
    () => buildTokenActivitySummary(tokenCalendar, timeseries, locale),
    [locale, timeseries, tokenCalendar],
  );
  const topToolName = overview?.top_tools[0]?.name ?? 'dcc-mcp';
  const profileTitle = t('analytics.profile.title');
  const profileSubtitle = t('analytics.profile.subtitle', {
    agents: overview?.kpi.unique_agents ?? 0,
    instances: overview?.kpi.unique_instances ?? 0,
    topTool: topToolName,
  });
  const longestTaskMs = useMemo(
    () => Math.max(...timeseries.map((point) => point.max_duration_ms ?? numericDurationMs(point.avg_duration_ms)), 0),
    [timeseries],
  );

  if (!active) return null;

  return (
    <section className="panel active analytics-panel" data-panel="analytics">
      <PanelHeader
        title={t('analytics.title')}
        action={
          <div className="analytics-actions">
            <Select value={range} onValueChange={(value) => setRange(value as AnalyticsRange)}>
              <SelectTrigger
                className="admin-select-trigger range-select-trigger"
                size="sm"
                aria-label={t('stats.label.range')}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="admin-select-content" position="popper" align="end">
                <SelectGroup>
                  {RANGES.map((r) => (
                    <SelectItem key={r} value={r}>{t(`analytics.range.${r}` as any)}</SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
            <Button type="button" size="sm" disabled={loading} onClick={() => { void refetch(); }}>
              <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
              {t('analytics.action.refresh')}
            </Button>
          </div>
        }
      />
      <StatusLine text={loading ? t('analytics.status.loading') : (error ? t('analytics.status.error') : '')} error={error ?? undefined} />
      <p className="empty log-hint">{t('analytics.description')}</p>

      {overview ? (
        <>
          <section className="analytics-profile">
            <div className="analytics-profile-avatar" aria-hidden="true">{profileInitials(profileTitle)}</div>
            <div className="analytics-profile-copy">
              <h3>{profileTitle}</h3>
              <p>{profileSubtitle}</p>
            </div>
            <span className="analytics-profile-badge">{t(`analytics.range.${range}` as any)}</span>
          </section>

          {/* KPI grid */}
          <div className="metric-grid analytics-summary-strip">
            <KpiCard label={t('analytics.kpi.tokensCumulative')} value={fmt(overview.kpi.tokens_total)} />
            <KpiCard label={t('analytics.kpi.tokensPeak')} value={fmt(tokenActivitySummary.peakDayTokens)} />
            <KpiCard label={t('analytics.kpi.longestTask')} value={formatDurationMs(longestTaskMs)} />
            <KpiCard label={t('analytics.kpi.currentStreak')} value={t('analytics.insight.daysValue', { count: tokenActivitySummary.currentStreak })} />
            <KpiCard label={t('analytics.kpi.longestStreak')} value={t('analytics.insight.daysValue', { count: tokenActivitySummary.longestStreak })} />
          </div>

          {/* Heatmap */}
          <section className="analytics-section analytics-token-activity">
            <div className="analytics-token-head">
              <h3>{t('analytics.section.heatmap')}</h3>
              <div className="analytics-token-controls">
                <div className="analytics-token-mode-group" role="group" aria-label={t('analytics.heatmap.modeLabel')}>
                  {TOKEN_CALENDAR_MODES.map((mode) => (
                    <button
                      key={mode}
                      type="button"
                      className={`analytics-token-mode${calendarMode === mode ? ' active' : ''}`}
                      aria-pressed={calendarMode === mode}
                      onClick={() => setCalendarMode(mode)}
                    >
                      {t(`analytics.heatmap.mode.${mode}` as any)}
                    </button>
                  ))}
                </div>
              </div>
            </div>
            <div className="analytics-token-scroll">
              <div
                className="analytics-token-calendar"
                style={{ '--analytics-calendar-weeks': tokenCalendar.weeks.length } as CSSProperties}
              >
                <div className="analytics-token-calendar-grid" role="img" aria-label={t('analytics.section.heatmap')}>
                  {tokenCalendar.weeks.map((week) => (
                    <div key={week[0]?.key ?? 'week'} className="analytics-token-week">
                      {week.map((day) => {
                        const value = calendarTokensForMode(day, calendarMode);
                        const maxValue = tokenCalendar.maxTokensByMode[calendarMode] ?? 1;
                        const level = calendarLevel(value, maxValue);
                        const label = `${day.key}: ${fmt(value)} ${t('analytics.heatmap.tokens')}, ${day.calls} ${t('analytics.kpi.callsTotal')}`;
                        return (
                          <span
                            key={day.key}
                            className={`analytics-token-day${day.outside ? ' is-outside' : ''}`}
                            data-level={level}
                            title={label}
                            aria-label={label}
                          />
                        );
                      })}
                    </div>
                  ))}
                </div>
                <div className="analytics-token-months" aria-hidden="true">
                  {tokenCalendar.monthLabels.map((label) => (
                    <span
                      key={`${label.weekIndex}-${label.label}`}
                      style={{ gridColumn: `${label.weekIndex + 1} / span ${Math.max(label.span, 1)}` }}
                    >
                      {label.label}
                    </span>
                  ))}
                </div>
              </div>
            </div>
            <div className="analytics-token-footer">
              <div className="analytics-token-legend" aria-hidden="true">
                <span>{t('analytics.heatmap.legend.low')}</span>
                {[0, 1, 2, 3, 4, 5].map((level) => (
                  <i key={level} data-level={level} />
                ))}
                <span>{t('analytics.heatmap.legend.high')}</span>
              </div>
            </div>
          </section>

          {/* Activity insights */}
          <section className="analytics-section analytics-insight-grid">
            <div className="analytics-insight-panel">
              <h3>{t('analytics.section.activityInsights')}</h3>
              <div className="analytics-insight-list">
                <div className="analytics-insight-row">
                  <span>{t('analytics.insight.activeDays')}</span>
                  <strong>{t('analytics.insight.daysValue', { count: tokenActivitySummary.activeDays })}</strong>
                </div>
                <div className="analytics-insight-row">
                  <span>{t('analytics.insight.longestStreak')}</span>
                  <strong>{t('analytics.insight.daysValue', { count: tokenActivitySummary.longestStreak })}</strong>
                </div>
                <div className="analytics-insight-row">
                  <span>{t('analytics.insight.peakDay')}</span>
                  <strong>{tokenActivitySummary.peakDayLabel}</strong>
                </div>
                <div className="analytics-insight-row">
                  <span>{t('analytics.insight.peakTokens')}</span>
                  <strong>{fmt(tokenActivitySummary.peakDayTokens)}</strong>
                </div>
                <div className="analytics-insight-row">
                  <span>{t('analytics.insight.avgTokensPerActiveDay')}</span>
                  <strong>{fmt(tokenActivitySummary.avgTokensPerActiveDay)}</strong>
                </div>
                <div className="analytics-insight-row">
                  <span>{t('analytics.insight.failureDays')}</span>
                  <strong>{t('analytics.insight.daysValue', { count: tokenActivitySummary.failureDays })}</strong>
                </div>
              </div>
            </div>

            <div className="analytics-insight-panel">
              <h3>{t('analytics.section.topTools')}</h3>
              <div className="analytics-top-tool-list">
                {overview.top_tools.slice(0, 3).map((tool) => (
                  <div key={tool.name} className="analytics-top-tool-row">
                    <span className="analytics-top-tool-mark" aria-hidden="true">{toolInitial(tool.name)}</span>
                    <code>{tool.name}</code>
                    <span>{t('analytics.tool.runs', { count: tool.calls })}</span>
                  </div>
                ))}
                {overview.top_tools.length === 0 ? <p className="empty">{t('analytics.empty.noData')}</p> : null}
              </div>
            </div>
          </section>

          {/* Timeseries */}
          <section className="analytics-section analytics-timeseries-section">
            <h3>{t('analytics.section.timeseries')}</h3>
            <MiniBarChart
              data={timeseries.map((p) => ({ label: p.date, value: p.calls, tone: p.failures > 0 ? 'err' : 'ok' }))}
              maxVal={maxDayCalls}
            />
            <div className="analytics-axis-labels">
              {timeseries.length > 0 && (
                <>
                  <span>{timeseries[0].date}</span>
                  <span>{timeseries[timeseries.length - 1].date}</span>
                </>
              )}
            </div>
          </section>

          {/* Export buttons */}
          <div className="analytics-export-actions">
            <Button asChild variant="outline" size="sm">
              <a
                href={`${API_BASE}/analytics/export?range=${encodeURIComponent(range)}&format=csv`}
                download
              >
                <RiDownloadCloudLine data-icon="inline-start" aria-hidden="true" />
                {t('analytics.action.exportCsv')}
              </a>
            </Button>
            <Button asChild variant="outline" size="sm">
              <a
                href={`${API_BASE}/analytics/export?range=${encodeURIComponent(range)}&format=json`}
                download
              >
                <RiDownloadCloudLine data-icon="inline-start" aria-hidden="true" />
                {t('analytics.action.exportJsonl')}
              </a>
            </Button>
          </div>
        </>
      ) : (
        !loading ? <p className="empty">{t('analytics.empty.noData')}</p> : null
      )}
    </section>
  );
}
