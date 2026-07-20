import { RiRefreshLine } from '@remixicon/react';
import { PanelHeader, StatusLine, MetricTile } from '../../admin-ui-core';
import { Badge } from '../../components/ui/badge';
import { Button } from '../../components/ui/button';
import { useQuery } from '@tanstack/react-query';
import { apiJson } from '../../admin-ui-core';
import type { Translator } from '../../admin-types';
import type { HealthPayload, InstanceRow, StatsPayload, SkillPayload, ReliabilityPayload } from '../../admin-types';
import './reliability.css';

const POLL_INTERVAL_MS = 5_000;

// ── helpers ───────────────────────────────────────────────────────────────

function fmtUptime(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  return `${d}d ${h}h`;
}

function fmtBytes(bytes: number): string {
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

function toneForCircuits(open: number, tracked: number): 'ok' | 'warn' | 'err' {
  if (tracked === 0) return 'ok';
  if (open === 0) return 'ok';
  if (open / tracked < 0.3) return 'warn';
  return 'err';
}

function toneForUptime(pct: number): 'ok' | 'warn' | 'err' {
  if (pct >= 99.9) return 'ok';
  if (pct >= 99.0) return 'warn';
  return 'err';
}

function toneForRatio(ready: number, total: number): 'ok' | 'warn' | 'err' {
  if (total === 0) return 'ok';
  const r = ready / total;
  if (r >= 0.9) return 'ok';
  if (r >= 0.5) return 'warn';
  return 'err';
}

// ── main component ────────────────────────────────────────────────────────

export function ReliabilityPanel({ active, t }: { active: boolean; t: Translator }) {
  const healthQuery = useQuery({
    queryKey: ['admin', 'health'],
    queryFn: () => apiJson<HealthPayload>(`/health`),
    enabled: active,
    refetchInterval: active ? POLL_INTERVAL_MS : false,
  });

  const instancesQuery = useQuery({
    queryKey: ['admin', 'instances'],
    queryFn: () => apiJson<InstanceRow[]>(`/instances`),
    enabled: active,
    refetchInterval: active ? POLL_INTERVAL_MS : false,
  });

  const skillsQuery = useQuery({
    queryKey: ['admin', 'skills'],
    queryFn: () => apiJson<SkillPayload>(`/skills`),
    enabled: active,
    refetchInterval: active ? POLL_INTERVAL_MS : false,
  });

  const statsQuery = useQuery({
    queryKey: ['admin', 'stats', '24h'],
    queryFn: () => apiJson<StatsPayload>(`/stats?range=24h`),
    enabled: active,
    refetchInterval: active ? POLL_INTERVAL_MS : false,
  });

  const reliabilityQuery = useQuery({
    queryKey: ['admin', 'reliability'],
    queryFn: () => apiJson<ReliabilityPayload>(`/reliability`),
    enabled: active,
    refetchInterval: active ? POLL_INTERVAL_MS : false,
  });

  const error = healthQuery.error
    ? `Health: ${String(healthQuery.error)}`
    : instancesQuery.error
      ? `Instances: ${String(instancesQuery.error)}`
      : skillsQuery.error
        ? `Skills: ${String(skillsQuery.error)}`
        : statsQuery.error
          ? `Stats: ${String(statsQuery.error)}`
          : reliabilityQuery.error
            ? `Reliability: ${String(reliabilityQuery.error)}`
            : undefined;

  const handleRefresh = () => {
    healthQuery.refetch();
    instancesQuery.refetch();
    skillsQuery.refetch();
    statsQuery.refetch();
    reliabilityQuery.refetch();
  };

  if (!active) return null;

  const health = healthQuery.data;
  const instances = instancesQuery.data ?? [];
  const skills = skillsQuery.data;
  const stats = statsQuery.data;
  const reliability = reliabilityQuery.data;

  const isLoading = healthQuery.isLoading || instancesQuery.isLoading || skillsQuery.isLoading || statsQuery.isLoading;

  // ── aggregated data ─────────────────────────────────────────────────────

  const gatewayStatus = health?.status ?? 'unknown';
  const gatewayStatusTone: 'ok' | 'warn' | 'err' = gatewayStatus === 'ok' ? 'ok' : gatewayStatus === 'degraded' ? 'warn' : 'err';
  const uptimeSecs = health?.uptime_secs ?? 0;
  const version = health?.version ?? '—';
  const electionCurrent = health?.gateway?.current ?? null;
  const electionCandidates = health?.gateway?.candidates ?? [];
  const limits = health?.limits;

  const circuitsTracked = health?.circuits?.tracked_backends ?? 0;
  const circuitsOpen = health?.circuits?.circuits_open ?? 0;
  const circuitsTone = toneForCircuits(circuitsOpen, circuitsTracked);

  const instancesReady = instances.filter((i) => i.status === 'ready').length;
  const instancesTotal = instances.length;
  const instancesTone = toneForRatio(instancesReady, instancesTotal);

  const skillsLoaded = skills?.loaded ?? 0;
  const skillsTotal = (skills?.loaded ?? 0) + (skills?.unloaded ?? 0);
  const skillsTone = toneForRatio(skillsLoaded, skillsTotal);

  const toolsRegistered = skills?.action_count ?? 0;
  const resourcesExposed = instancesTotal > 0 ? instancesTotal : 0; // approximate

  // stability — from /api/reliability (PIP-2766)
  const crashes24h = reliability?.stability?.crashes_24h ?? 0;
  const reconnects24h = reliability?.stability?.reconnects_24h ?? 0;
  const recoveries24h = reliability?.stability?.recoveries_24h ?? 0;
  const uptimePct = reliability?.stability?.uptime_pct ?? (stats?.success_rate ? Number(stats.success_rate) : 0);
  const uptimeTone = toneForUptime(uptimePct);

  const successRate = reliability?.stability?.uptime_pct ?? (stats?.success_rate ? Number(stats.success_rate) : 0);
  const p50Ms = stats?.p50_ms ?? null;

  const circuitThreshold = limits?.circuit_failure_threshold ?? 0;
  const circuitOpenSecs = limits?.circuit_open_secs ?? 0;
  const bodyLimit = limits?.body_max_bytes ?? 0;
  const rateLimit = limits?.rate_limit_per_minute_per_ip ?? 0;

  return (
    <section className="panel active reliability-panel" data-panel="reliability">
      <PanelHeader
        title={t('reliability.title')}
        action={
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={handleRefresh}
            aria-label="Refresh"
          >
            <RiRefreshLine className={isLoading ? 'spin' : ''} />
          </Button>
        }
      />

      {error ? <StatusLine error={error} /> : null}

      {isLoading && !health && !instances.length && !skills && !stats ? (
        <StatusLine text="Loading..." />
      ) : (
        <>
          {/* ── Gateway Health ──────────────────────────────────────────── */}
          <div className="reliability-section">
            <h3>{t('reliability.section.gateway')}</h3>
            <div className="reliability-grid">
              <MetricTile
                tone={gatewayStatusTone}
                label={t('reliability.metric.status')}
                value={gatewayStatus}
              />
              <MetricTile
                label={t('reliability.metric.uptime')}
                value={fmtUptime(uptimeSecs)}
              />
              <MetricTile
                label={t('reliability.metric.version')}
                value={version}
              />
            </div>

            {/* election info */}
            <div className="reliability-election" style={{ marginTop: 12 }}>
              <div className="election-row">
                <span className="election-label">{t('reliability.metric.electionCurrent')}</span>
                <span className="election-value">
                  {electionCurrent
                    ? `${electionCurrent.name} (${electionCurrent.host}:${electionCurrent.port})`
                    : t('reliability.election.none')}
                </span>
                {electionCurrent ? (
                  <Badge variant="outline" className="tone-ok">{electionCurrent.role}</Badge>
                ) : null}
              </div>
              {electionCandidates.length > 0 ? (
                <div className="election-row">
                  <span className="election-label">{t('reliability.metric.electionCandidates')}</span>
                  <div className="election-candidates">
                    {electionCandidates.map((c) => (
                      <Badge key={c.instance_id} variant="outline">
                        {c.name} ({c.host}:{c.port})
                      </Badge>
                    ))}
                  </div>
                </div>
              ) : null}
            </div>

            {/* gateway config */}
            <div className="reliability-config-grid" style={{ marginTop: 12 }}>
              <div className="config-row">
                <span className="config-label">{t('reliability.metric.bodyLimit')}</span>
                <span className="config-value">{fmtBytes(bodyLimit)}</span>
              </div>
              <div className="config-row">
                <span className="config-label">{t('reliability.metric.rateLimit')}</span>
                <span className="config-value">{rateLimit}</span>
              </div>
              <div className="config-row">
                <span className="config-label">{t('reliability.metric.circuitThreshold')}</span>
                <span className="config-value">{circuitThreshold}</span>
              </div>
              <div className="config-row">
                <span className="config-label">{t('reliability.metric.circuitOpenSecs')}</span>
                <span className="config-value">{circuitOpenSecs}s</span>
              </div>
            </div>
          </div>

          {/* ── Circuit Breakers ────────────────────────────────────────── */}
          <div className="reliability-section">
            <h3>{t('reliability.section.circuits')}</h3>
            <div className="reliability-grid">
              <MetricTile
                tone={circuitsTone}
                label={t('reliability.metric.circuitsTracked')}
                value={circuitsTracked}
              />
              <MetricTile
                tone={circuitsOpen > 0 ? 'warn' : 'ok'}
                label={t('reliability.metric.circuitsOpen')}
                value={circuitsOpen}
                detail={circuitsTracked > 0
                  ? `${((circuitsOpen / circuitsTracked) * 100).toFixed(0)}%`
                  : '—'}
              />
            </div>
          </div>

          {/* ── Capability Funnel ───────────────────────────────────────── */}
          <div className="reliability-section">
            <h3>{t('reliability.section.capability')}</h3>
            <div className="reliability-grid">
              <MetricTile
                tone={instancesTone}
                label={t('reliability.metric.instancesReady')}
                value={`${instancesReady} / ${instancesTotal}`}
              />
              <MetricTile
                tone={skillsTone}
                label={t('reliability.metric.skillsLoaded')}
                value={`${skillsLoaded} / ${skillsTotal}`}
              />
              <MetricTile
                label={t('reliability.metric.toolsRegistered')}
                value={toolsRegistered}
              />
              <MetricTile
                label={t('reliability.metric.resourcesExposed')}
                value={resourcesExposed}
              />
            </div>
          </div>

          {/* ── Artifact Verification ───────────────────────────────────── */}
          <div className="reliability-section">
            <h3>{t('reliability.section.artifacts')}</h3>
            <div className="reliability-grid">
              <MetricTile
                label={t('reliability.metric.buildsVerified')}
                value="—"
                detail="Not yet reported"
              />
              <MetricTile
                label={t('reliability.metric.buildsTotal')}
                value="—"
              />
              <MetricTile
                tone="ok"
                label={t('reliability.metric.verificationErrors')}
                value="—"
              />
            </div>
          </div>

          {/* ── Stability ───────────────────────────────────────────────── */}
          <div className="reliability-section">
            <h3>{t('reliability.section.stability')}</h3>
            <div className="reliability-grid">
              <MetricTile
                tone={crashes24h > 0 ? 'warn' : 'ok'}
                label={t('reliability.metric.crashes')}
                value={crashes24h || '—'}
              />
              <MetricTile
                tone={reconnects24h > 0 ? 'warn' : 'ok'}
                label={t('reliability.metric.reconnects')}
                value={reconnects24h || '—'}
              />
              <MetricTile
                tone={recoveries24h > 0 ? 'ok' : undefined}
                label={t('reliability.metric.recoveries')}
                value={recoveries24h || '—'}
              />
              <MetricTile
                tone={uptimeTone}
                label={t('reliability.metric.successRate')}
                value={`${successRate.toFixed(1)}%`}
              />
              <MetricTile
                label={t('reliability.metric.latency')}
                value={p50Ms != null ? `${p50Ms}ms` : '—'}
              />
            </div>
          </div>
        </>
      )}
    </section>
  );
}
