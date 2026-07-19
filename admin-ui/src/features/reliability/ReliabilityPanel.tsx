import { RiCheckLine, RiCloseLine, RiErrorWarningLine, RiRefreshLine, RiTimerLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import {
  HealthCard,
  MetricTile,
  PanelHeader,
  StatusLine,
  TimeValue,
  formatBytes,
  formatDurationMs,
  formatUptime,
} from '../../admin-ui-core';
import { useReliabilityQuery } from '../../hooks/queries';
import type { CircuitBreakerStatus, Translator } from '../../admin-types';
import './reliability.css';

export type ReliabilityPanelProps = {
  active: boolean;
  updatedAt: string;
  error?: string;
  onRefresh: () => void;
  t: Translator;
};

function circuitTone(state: CircuitBreakerStatus['state']): 'ok' | 'warn' | 'err' {
  if (state === 'open') return 'err';
  if (state === 'half_open') return 'warn';
  return 'ok';
}

function circuitStateLabel(state: CircuitBreakerStatus['state'], t: Translator): string {
  if (state === 'open') return t('reliability.circuits.state.open');
  if (state === 'half_open') return t('reliability.circuits.state.halfOpen');
  return t('reliability.circuits.state.closed');
}

function CircuitBadge({ state, t }: { state: CircuitBreakerStatus['state']; t: Translator }) {
  const tone = circuitTone(state);
  const Icon = tone === 'err' ? RiCloseLine : tone === 'warn' ? RiErrorWarningLine : RiCheckLine;
  return (
    <span className={`badge badge-${tone}`}>
      <Icon data-icon="inline-start" aria-hidden="true" />
      {circuitStateLabel(state, t)}
    </span>
  );
}

function leaderLabel(leader: { name: string; host: string; port: number; version: string | null } | null): string {
  if (!leader) {
    return '-';
  }
  const version = leader.version ? ` · v${leader.version}` : '';
  return `${leader.name} · ${leader.host}:${leader.port}${version}`;
}

export function ReliabilityPanel({
  active,
  updatedAt,
  error,
  onRefresh,
  t,
}: ReliabilityPanelProps) {
  const reliabilityQuery = useReliabilityQuery(active);

  if (!active) return null;

  const data = reliabilityQuery.data ?? null;
  const health = data?.health ?? null;
  const circuits = data?.circuits ?? [];
  const funnel = data?.funnel ?? null;
  const stability = data?.stability_24h ?? null;
  const openCircuits = circuits.filter((circuit) => circuit.state !== 'closed');
  const queryError = reliabilityQuery.error instanceof Error ? reliabilityQuery.error.message : undefined;

  const funnelSteps = funnel
    ? [
        { key: 'instances', label: t('reliability.funnel.instances'), value: funnel.instances },
        { key: 'skills', label: t('reliability.funnel.skills'), value: funnel.skills },
        { key: 'tools', label: t('reliability.funnel.tools'), value: funnel.tools },
        { key: 'resources', label: t('reliability.funnel.resources'), value: funnel.resources },
      ]
    : [];
  const funnelMax = Math.max(1, ...funnelSteps.map((step) => step.value));

  const handleRefresh = () => {
    void reliabilityQuery.refetch();
    onRefresh();
  };

  return (
    <section className="panel active reliability-panel" data-panel="reliability">
      <PanelHeader
        title={t('reliability.panel.title')}
        meta={t('reliability.panel.description')}
        action={
          <Button type="button" size="sm" disabled={reliabilityQuery.isFetching} onClick={handleRefresh}>
            <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
            {t('action.refresh')}
          </Button>
        }
      />
      <StatusLine text={updatedAt} error={error ?? queryError} />

      {reliabilityQuery.isLoading && !data ? (
        <p className="empty">{t('common.status.loading')}</p>
      ) : !data ? (
        <div className="reliability-empty">
          <h3>{t('reliability.empty.title')}</h3>
          <p className="empty">{t('reliability.empty.description')}</p>
        </div>
      ) : (
        <>
          <section className="reliability-section" aria-label={t('reliability.health.title')}>
            <h3 className="reliability-section-title">{t('reliability.health.title')}</h3>
            <div className="health-grid">
              <HealthCard
                tone={health?.status === 'ok' ? 'ok' : 'warn'}
                label={t('reliability.health.status')}
                value={health?.status ?? '-'}
              />
              <HealthCard label={t('reliability.health.uptime')} value={formatUptime(health?.uptime_secs)} />
              <HealthCard label={t('reliability.health.leader')} value={leaderLabel(health?.leader ?? null)} />
              <HealthCard label={t('reliability.health.candidates')} value={String(health?.candidates ?? 0)} />
              <HealthCard label={t('reliability.health.bodyMax')} value={health?.limits ? formatBytes(health.limits.body_max_bytes) : '-'} />
              <HealthCard
                label={t('reliability.health.rateLimit')}
                value={health?.limits ? (health.limits.rate_limit_per_minute_per_ip === 0 ? 'off' : String(health.limits.rate_limit_per_minute_per_ip)) : '-'}
              />
              <HealthCard label={t('reliability.health.circuitThreshold')} value={health?.limits ? String(health.limits.circuit_failure_threshold) : '-'} />
              <HealthCard label={t('reliability.health.circuitOpenSecs')} value={health?.limits ? `${health.limits.circuit_open_secs}s` : '-'} />
            </div>
          </section>

          <section className="reliability-section" aria-label={t('reliability.circuits.title')}>
            <h3 className="reliability-section-title">{t('reliability.circuits.title')}</h3>
            <div className="metric-grid compact">
              <MetricTile label={t('reliability.circuits.tracked')} value={circuits.length} />
              <MetricTile
                tone={openCircuits.length ? 'warn' : 'ok'}
                label={t('reliability.circuits.open')}
                value={openCircuits.length}
              />
            </div>
            {circuits.length === 0 ? (
              <p className="empty">{t('reliability.empty.description')}</p>
            ) : (
              <div className="reliability-circuit-list">
                {circuits.map((circuit) => (
                  <div className={`reliability-circuit-card ${circuitTone(circuit.state)}`} key={circuit.backend}>
                    <div className="reliability-circuit-head">
                      <strong title={circuit.backend}>{circuit.backend}</strong>
                      <CircuitBadge state={circuit.state} t={t} />
                    </div>
                    <div className="reliability-circuit-meta">
                      <span><strong>{t('reliability.circuits.failures')}</strong>{circuit.failures}</span>
                      <span><strong>{t('reliability.circuits.lastFailure')}</strong><TimeValue value={circuit.last_failure} /></span>
                      <span><strong>{t('reliability.circuits.lastSuccess')}</strong><TimeValue value={circuit.last_success} /></span>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="reliability-section" aria-label={t('reliability.funnel.title')}>
            <h3 className="reliability-section-title">{t('reliability.funnel.title')}</h3>
            {funnelSteps.length === 0 ? (
              <p className="empty">{t('reliability.empty.description')}</p>
            ) : (
              <div className="reliability-funnel">
                {funnelSteps.map((step, index) => (
                  <div className="reliability-funnel-row" key={step.key}>
                    <div className="reliability-funnel-label">{step.label}</div>
                    <div className="reliability-funnel-track">
                      <div
                        className="reliability-funnel-fill"
                        style={{ width: `${Math.max(4, (step.value / funnelMax) * 100)}%` }}
                      />
                    </div>
                    <div className="reliability-funnel-value">{step.value}</div>
                    {index < funnelSteps.length - 1 ? <div className="reliability-funnel-arrow" aria-hidden="true">&darr;</div> : null}
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="reliability-section" aria-label={t('reliability.stability.title')}>
            <h3 className="reliability-section-title">
              <RiTimerLine data-icon="inline-start" aria-hidden="true" />
              {t('reliability.stability.title')}
            </h3>
            <div className="metric-grid compact">
              <MetricTile
                tone={stability && stability.crashes > 0 ? 'err' : 'ok'}
                label={t('reliability.stability.crashes')}
                value={stability?.crashes ?? 0}
              />
              <MetricTile label={t('reliability.stability.reconnects')} value={stability?.reconnects ?? 0} />
              <MetricTile label={t('reliability.stability.recoveries')} value={stability?.recoveries ?? 0} />
              <MetricTile
                tone={stability ? (stability.success_rate_pct < 95 ? 'err' : stability.success_rate_pct < 99 ? 'warn' : 'ok') : undefined}
                label={t('reliability.stability.successRate')}
                value={stability ? `${stability.success_rate_pct.toFixed(1)}%` : '-'}
              />
              <MetricTile label={t('reliability.stability.avgLatency')} value={formatDurationMs(stability?.avg_latency_ms)} />
              <MetricTile label={t('reliability.stability.p95Latency')} value={formatDurationMs(stability?.p95_latency_ms)} />
            </div>
          </section>
        </>
      )}
    </section>
  );
}
