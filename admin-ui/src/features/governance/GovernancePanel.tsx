import { RiRefreshLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import type {
  GovernanceDecisionRow,
  GovernancePayload,
  Translator,
} from '../../admin-types';
import {
  agentLabel,
  compactId,
  compactList,
  EmptyRow,
  formatTraceDate,
  GovernanceControlCard,
  MetricTile,
  PanelHeader,
  StatusLine,
} from '../../admin-ui-core';

export type GovernanceSummary = {
  allowed: number;
  denied: number;
  throttled: number;
  captured: number;
  skipped: number;
  redacted: number;
  captureEnabled: boolean;
  readOnly: boolean;
  allowlists: number;
};

export type GovernancePanelProps = {
  governance: GovernancePayload | null;
  governanceSummary: GovernanceSummary;
  filteredGovernanceDecisions: GovernanceDecisionRow[];
  updatedAt: string;
  error?: string;
  onRefresh: () => void;
  t: Translator;
};

export function GovernancePanel({
  governance,
  governanceSummary,
  filteredGovernanceDecisions,
  updatedAt,
  error,
  onRefresh,
  t,
}: GovernancePanelProps) {
  return (
    <section className="panel active governance-panel" data-panel="governance">
      <PanelHeader
        title={t('governance.title')}
        meta={governance?.mode?.reason ?? t('governance.meta')}
        action={(
          <Button type="button" size="sm" onClick={onRefresh}>
            <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
            {t('action.refresh')}
          </Button>
        )}
      />
      <StatusLine text={updatedAt} error={error} />
      <div className="metric-grid">
        <MetricTile
          tone={governanceSummary.captureEnabled ? 'warn' : 'ok'}
          label={t('governance.metric.capture')}
          value={governanceSummary.captureEnabled ? t('common.status.on') : t('common.status.off')}
          detail={governance?.traffic_capture?.mode ?? t('governance.detail.safeAggregateOnly')}
        />
        <MetricTile
          tone={governanceSummary.readOnly ? 'warn' : undefined}
          label={t('governance.metric.readOnly')}
          value={governanceSummary.readOnly ? t('common.status.on') : t('common.status.off')}
          detail={t('governance.detail.activeAllowlists', { count: governanceSummary.allowlists })}
        />
        <MetricTile label={t('governance.metric.denied')} value={governanceSummary.denied} detail={t('governance.detail.recentPolicyDecisions')} />
        <MetricTile tone={governanceSummary.throttled ? 'warn' : undefined} label={t('governance.metric.throttled')} value={governanceSummary.throttled} detail={t('governance.detail.recentPressureDecisions')} />
      </div>
      <div className="governance-layout">
        <section className="governance-section">
          <h3 className="section-kicker">{t('governance.section.effectivePolicy')}</h3>
          <div className="governance-card">
            <div className="governance-kv">
              <span><strong>DCC</strong>{compactList(governance?.policy?.allowed_dcc_types)}</span>
              <span><strong>{t('governance.label.skills')}</strong>{compactList([...(governance?.policy?.allowed_skill_names ?? []), ...(governance?.policy?.allowed_skill_families ?? [])])}</span>
              <span><strong>{t('governance.label.tools')}</strong>{compactList([...(governance?.policy?.allowed_tool_slugs ?? []), ...(governance?.policy?.allowed_tool_slug_prefixes ?? [])])}</span>
              <span><strong>{t('governance.label.mode')}</strong>{governance?.policy?.unrestricted ? t('governance.state.unrestricted') : t('governance.state.constrained')}</span>
            </div>
          </div>
        </section>
        <section className="governance-section">
          <h3 className="section-kicker">{t('governance.section.trafficCapture')}</h3>
          <div className="governance-card">
            <div className="governance-kv">
              <span><strong>{t('governance.label.sinks')}</strong>{governance?.traffic_capture?.sink_count ?? 0}</span>
              <span><strong>{t('governance.label.guardrail')}</strong>{governance?.traffic_capture?.production_guardrail ?? t('governance.state.inactive')}</span>
              <span><strong>{t('governance.label.captured')}</strong>{governanceSummary.captured}</span>
              <span><strong>{t('governance.label.skipped')}</strong>{governanceSummary.skipped}</span>
            </div>
            <p className="mono-path">{compactList(governance?.traffic_capture?.redaction?.paths, t('governance.empty.captureRedactionRules'))}</p>
          </div>
        </section>
        <section className="governance-section wide">
          <h3 className="section-kicker">{t('governance.section.middlewareControls')}</h3>
          <div className="governance-card-grid">
            {(governance?.middleware?.controls ?? []).length === 0 ? (
              <p className="empty">{t('governance.empty.controls')}</p>
            ) : (
              (governance?.middleware?.controls ?? []).map((control, index) => (
                <GovernanceControlCard key={`${control.kind}-${control.mode}-${index}`} control={control} t={t} />
              ))
            )}
          </div>
        </section>
        <section className="governance-section wide">
          <h3 className="section-kicker">{t('governance.section.recentRequestDecisions')}</h3>
          <table>
            <thead>
              <tr>
                <th>{t('common.table.request')}</th>
                <th>{t('governance.table.outcome')}</th>
                <th>{t('governance.table.agentSession')}</th>
                <th>{t('common.table.tool')}</th>
                <th>{t('governance.table.capture')}</th>
                <th>{t('governance.table.redaction')}</th>
              </tr>
            </thead>
            <tbody>
              {(governance?.recent_decisions ?? []).length === 0 ? (
                <EmptyRow columns={6}>{t('governance.empty.decisions')}</EmptyRow>
              ) : filteredGovernanceDecisions.length === 0 ? (
                <EmptyRow columns={6}>{t('governance.empty.decisionsSearch')}</EmptyRow>
              ) : (
                filteredGovernanceDecisions.map((row, index) => (
                  <tr key={`${row.request_id ?? row.trace_id ?? 'decision'}-${index}`}>
                    <td>
                      <span className="mono-path">{compactId(row.request_id)}</span>
                      <div className="muted">{formatTraceDate(row.timestamp)}</div>
                    </td>
                    <td>
                      <span className={`badge ${row.outcome === 'allowed' ? 'badge-ok' : row.outcome === 'throttled' || row.outcome === 'denied' ? 'badge-err' : 'badge-muted'}`}>
                        {row.outcome ?? 'unknown'}
                      </span>
                      {row.reason ? <div className="muted">{row.policy?.reason ?? row.reason}</div> : null}
                    </td>
                    <td>
                      {agentLabel(row)}
                      <div className="muted">{compactId(row.session_id)}</div>
                    </td>
                    <td>
                      <span className="mono-path">{row.tool ?? '-'}</span>
                      <div className="muted">{row.dcc_type ?? '-'}</div>
                    </td>
                    <td>
                      {(row.traffic_capture?.captured ?? 0) > 0 ? t('governance.capture.captured') : t('governance.capture.skipped')}
                      <div className="muted">{compactList(row.traffic_capture?.reasons, t('governance.capture.noReason'))}</div>
                    </td>
                    <td className="mono-path">{compactList(row.privacy?.redacted_paths, t('governance.privacy.none'))}</td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </section>
      </div>
    </section>
  );
}
