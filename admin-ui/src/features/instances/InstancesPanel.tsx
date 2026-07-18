import {
  RiCheckboxCircleLine,
  RiDownloadCloudLine,
  RiErrorWarningLine,
  RiRefreshLine,
} from '@remixicon/react';
import { Button } from '../../components/ui/button';
import type {
  InstanceRow,
  InstanceSummary,
  Translator,
} from '../../admin-types';
import {
  BackendAccessUrl,
  BackendOpenApiLinks,
  compactInstanceId,
  formatBytes,
  formatUptime,
  groupRows,
  instanceGroupLabel,
  McpBackendLinks,
  resolveDccIcon,
  statusClass,
  StatusBadge,
  StatusLine,
} from '../../admin-ui-core';

export type InstanceUpdateNotice = {
  tone: 'ok' | 'warn' | 'err' | 'muted';
  message: string;
  requiresRestart?: boolean;
};

export type InstancesPanelProps = {
  updatedAt: string;
  error?: string;
  instanceRows: InstanceRow[];
  filteredInstanceRows: InstanceRow[];
  instanceSummary: InstanceSummary;
  instanceUpdateNotices: Record<string, InstanceUpdateNotice>;
  pendingInstanceUpdateId: string | null;
  onUpdateInstance: (instance: InstanceRow) => void;
  onRefresh: () => void;
  t: Translator;
};

export function InstancesPanel({
  updatedAt,
  error,
  instanceRows,
  filteredInstanceRows,
  instanceSummary,
  instanceUpdateNotices,
  pendingInstanceUpdateId,
  onUpdateInstance,
  onRefresh,
  t,
}: InstancesPanelProps) {
  return (
    <section className="panel active instances-panel">
      <h2>{t('instances.title')}</h2>
      <p className="empty log-hint">
        {t('instances.description')}
      </p>
      <StatusLine text={updatedAt} error={error} />
      {instanceRows.length === 0 ? (
        <p className="empty">{t('instances.empty.none')}</p>
      ) : filteredInstanceRows.length === 0 ? (
        <p className="empty">{t('instances.empty.search')}</p>
      ) : (
        <div className="instance-groups">
          {Array.from(groupRows(filteredInstanceRows, instanceGroupLabel).entries())
            .sort(([a], [b]) => a.localeCompare(b))
            .map(([group, groupInstances]) => {
              const flagged = groupInstances.filter((instance) => instance.stale || !statusClass(instance.status).includes('ok')).length;
              return (
                <div key={group} className="instance-group">
                  <div className="instance-group-head">
                    <h3>{group}</h3>
                    <span>{t('instances.group.meta', { count: groupInstances.length, flagged })}</span>
                  </div>
                  <div className="instances-list" role="list">
                    {groupInstances.map((instance) => {
                      const updateVersion = instanceUpdateVersion(instance);
                      const updateNotice = instanceUpdateNotices[instance.instance_id];
                      const isUpdating = pendingInstanceUpdateId === instance.instance_id;
                      const updateLabel = isUpdating ? t('instances.update.checking') : t('instances.update.action');
                      const stateTone = instance.stale ? 'stale' : statusClass(instance.status).replace('badge badge-', '');
                      return (
                        <article
                          key={instance.instance_id}
                          className={`instance-row ${stateTone}`}
                          data-instance-id={instance.instance_id}
                          role="listitem"
                        >
                          <div className="instance-row-main">
                            <div className="instance-identity">
                              <img src={resolveDccIcon(instance.dcc_type)} alt="" className="dcc-icon" aria-hidden />
                              <div className="instance-identity-copy">
                                <div className="instance-title">
                                  {instance.display_name ?? compactInstanceId(instance.instance_id)}
                                  <span>{compactInstanceId(instance.instance_id)}</span>
                                </div>
                                <div className="instance-subline">
                                  <span>{t('instances.field.appType')} {instance.dcc_type}</span>
                                  <span>{t('instances.field.instanceType')} {instance.instance_type ?? 'unknown'}</span>
                                  {instance.pid != null ? <span>PID {instance.pid}</span> : null}
                                  {instance.uptime_secs != null ? <span>{formatUptime(instance.uptime_secs)}</span> : null}
                                </div>
                              </div>
                            </div>
                            <div className="instance-state-strip" aria-label={t('instances.state.aria')}>
                              <span>
                                <small>{t('instances.field.status')}</small>
                                <StatusBadge value={instance.status} />
                              </span>
                              {instance.dispatch_status ? (
                                <span>
                                  <small>{t('instances.field.dispatch')}</small>
                                  <span><StatusBadge value={instance.dispatch_status} /> {instance.dispatch_ready ? t('instances.dispatch.callable') : t('instances.dispatch.notCallable')}</span>
                                </span>
                              ) : null}
                              {instance.failure_reason ? (
                                <span className="instance-state-failure">
                                  <small>{t('instances.field.failure')}</small>
                                  <span>{instance.failure_reason}</span>
                                </span>
                              ) : null}
                            </div>
                          </div>

                          <div className="instance-row-details">
                            <span className="instance-detail-item">
                              <small>{t('instances.field.version')}</small>
                              <strong>{instance.version ?? '-'}</strong>
                            </span>
                            <span className="instance-detail-item">
                              <small>{t('instances.field.serverVersion')}</small>
                              <strong>{instance.server_version ?? '-'}</strong>
                            </span>
                            <span className="instance-detail-item">
                              <small>{t('instances.field.adapter')}</small>
                              <strong>{instance.adapter_version ?? '-'}</strong>
                            </span>
                            <span className="instance-detail-item">
                              <small>{t('instances.field.scene')}</small>
                              <strong>{instance.scene ?? '-'}</strong>
                            </span>
                            <span className="instance-detail-item">
                              <small>{t('instances.field.cpu')}</small>
                              <strong>{instance.cpu_percent == null ? '-' : instance.cpu_percent.toFixed(1)}</strong>
                            </span>
                            <span className="instance-detail-item">
                              <small>{t('instances.field.memory')}</small>
                              <strong>{formatBytes(instance.memory_bytes)}</strong>
                            </span>
                            {instance.host_rpc_uri || instance.host_rpc_scheme ? (
                              <span className="instance-detail-item wide">
                                <small>{t('instances.field.hostRpc')}</small>
                                <strong title={instance.host_rpc_uri ?? undefined}>{instance.host_rpc_uri ?? instance.host_rpc_scheme}</strong>
                              </span>
                            ) : null}
                            <span className="instance-detail-item wide">
                              <small>{t('instances.field.accessUrl')}</small>
                              <strong><BackendAccessUrl mcpUrl={instance.mcp_url} /></strong>
                            </span>
                          </div>

                          <div className="instance-row-actions">
                            <div className="instance-update-cell">
                              <div className="instance-update-head">
                                <span className="instance-update-heading">{t('instances.update.label')}</span>
                                <Button
                                  aria-label={t('instances.update.aria', { name: instance.display_name ?? compactInstanceId(instance.instance_id) })}
                                  className="instance-update-button"
                                  disabled={isUpdating}
                                  size="sm"
                                  type="button"
                                  variant="outline"
                                  onClick={() => onUpdateInstance(instance)}
                                >
                                  {isUpdating ? (
                                    <RiRefreshLine className="is-spinning" data-icon="inline-start" aria-hidden="true" />
                                  ) : (
                                    <RiDownloadCloudLine data-icon="inline-start" aria-hidden="true" />
                                  )}
                                  <span>{updateLabel}</span>
                                </Button>
                              </div>
                              <span className="instance-update-meta">
                                {t('instances.update.current', {
                                  version: updateVersion ?? t('instances.update.unknownCurrent'),
                                })}
                              </span>
                              {updateNotice ? (
                                <div className={`instance-update-result ${updateNotice.tone}`} role="status">
                                  {updateNotice.tone === 'ok' ? (
                                    <RiCheckboxCircleLine aria-hidden="true" />
                                  ) : updateNotice.tone === 'warn' || updateNotice.tone === 'err' ? (
                                    <RiErrorWarningLine aria-hidden="true" />
                                  ) : (
                                    <RiRefreshLine aria-hidden="true" />
                                  )}
                                  <span>
                                    <strong>{updateNotice.message}</strong>
                                    {updateNotice.requiresRestart ? (
                                      <small>{t('instances.update.restartRequired')}</small>
                                    ) : null}
                                  </span>
                                </div>
                              ) : (
                                <p className="instance-update-help">{t('instances.update.help')}</p>
                              )}
                            </div>
                            <div className="instance-link-groups">
                              <span>
                                <small>{t('instances.field.endpoints')}</small>
                                <McpBackendLinks mcpUrl={instance.mcp_url} />
                              </span>
                              <span>
                                <small>{t('instances.field.openapi')}</small>
                                <BackendOpenApiLinks instance={instance} />
                              </span>
                            </div>
                          </div>
                        </article>
                      );
                    })}
                  </div>
                </div>
              );
            })}
        </div>
      )}
      <div className="status-bar">{t('instances.summary', { live: instanceSummary.live, stale: instanceSummary.stale, unhealthy: instanceSummary.unhealthy })}</div>
      <Button type="button" size="sm" onClick={onRefresh}>
        <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
        {t('action.refresh')}
      </Button>
    </section>
  );
}

function instanceUpdateVersion(instance: InstanceRow): string | null {
  return instance.server_version ?? null;
}
