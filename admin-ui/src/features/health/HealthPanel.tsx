import { RiRefreshLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import { HealthCard, StatusLine, formatUptime, formatBytes, gatewayLabel } from '../../admin-ui-core';
import type { HealthPayload, Translator } from '../../admin-types';

export type HealthPanelProps = {
  updatedAt: string;
  error?: string;
  health: HealthPayload | null;
  onRefresh: () => void;
  t: Translator;
};

export function HealthPanel({
  updatedAt,
  error,
  health,
  onRefresh,
  t,
}: HealthPanelProps) {
  return (
    <section className="panel active health-panel">
      <h2>{t('health.title')}</h2>
      <StatusLine text={updatedAt} error={error} />
      <div className="health-grid">
        <HealthCard tone={health?.status === 'ok' ? 'ok' : 'warn'} label={t('health.metric.status')} value={health?.status ?? '?'} />
        <HealthCard label={t('health.metric.uptime')} value={formatUptime(health?.uptime_secs)} />
        <HealthCard tone={health && health.instances_ready > 0 ? 'ok' : 'warn'} label={t('health.metric.ready')} value={`${health?.instances_ready ?? 0} / ${health?.instances_total ?? 0}`} />
        <HealthCard label={t('health.metric.version')} value={health?.version ?? '?'} />
        <HealthCard label={t('health.metric.gatewayOwner')} value={gatewayLabel(health)} />
        <HealthCard label={t('health.metric.gatewayCandidates')} value={String(health?.gateway?.candidates?.length ?? 0)} />
        <HealthCard
          label={t('health.metric.responseFormat')}
          value={`${health?.response_format?.default ?? 'toon'} / ${health?.response_format?.token_estimator ?? '-'}`}
        />
        <HealthCard label={t('health.metric.rss')} value={formatBytes(health?.rss_bytes ?? undefined)} />
        <HealthCard label={t('health.metric.bodyLimit')} value={health?.limits ? formatBytes(health.limits.body_max_bytes) : '?'} />
        <HealthCard
          label={t('health.metric.rateLimit')}
          value={health?.limits ? (health.limits.rate_limit_per_minute_per_ip === 0 ? 'off' : String(health.limits.rate_limit_per_minute_per_ip)) : '?'}
        />
        <HealthCard
          label={t('health.metric.xffTrustedDepth')}
          value={health?.limits ? String(health.limits.xff_trusted_depth) : '?'}
        />
        <HealthCard label={t('health.metric.readRetries')} value={health?.limits ? String(health.limits.read_retry_max) : '?'} />
        <HealthCard label={t('health.metric.circuitLimit')} value={health?.limits ? `${health.limits.circuit_failure_threshold} / ${health.limits.circuit_open_secs}s` : '?'} />
        <HealthCard
          tone={health?.circuits && health.circuits.circuits_open > 0 ? 'warn' : undefined}
          label={t('health.metric.circuitsOpenTracked')}
          value={health?.circuits ? `${health.circuits.circuits_open} / ${health.circuits.tracked_backends}` : '?'}
        />
      </div>
      <Button type="button" size="sm" onClick={onRefresh}>
        <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
        {t('action.refresh')}
      </Button>
    </section>
  );
}
