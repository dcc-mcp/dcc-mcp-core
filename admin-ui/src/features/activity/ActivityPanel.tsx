import { RiRefreshLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import { StatusBadge, StatusLine, TimeValue } from '../../admin-ui-core';
import type { ActivityEvent, NavigateOptions, Panel, Translator } from '../../admin-types';

export type ActivityPanelProps = {
  updatedAt: string;
  error?: string;
  activity: ActivityEvent[];
  filteredActivity: ActivityEvent[];
  onGoToPanel: (panel: Panel, opts?: NavigateOptions) => void;
  onRefresh: () => void;
  t: Translator;
};

export function ActivityPanel({
  updatedAt,
  error,
  activity,
  filteredActivity,
  onGoToPanel,
  onRefresh,
  t,
}: ActivityPanelProps) {
  return (
    <section className="panel active activity-panel">
      <h2>{t('activity.title')}</h2>
      <StatusLine text={updatedAt} error={error} />
      {activity.length === 0 ? (
        <p className="empty">{t('activity.empty.none')}</p>
      ) : filteredActivity.length === 0 ? (
        <p className="empty">{t('activity.empty.search')}</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>{t('common.table.time')}</th>
              <th>{t('common.table.status')}</th>
              <th>{t('common.table.kind')}</th>
              <th>{t('common.table.message')}</th>
              <th>{t('common.table.dcc')}</th>
              <th>{t('common.table.actor')}</th>
              <th>{t('common.table.platform')}</th>
              <th>{t('common.table.sourceIp')}</th>
              <th>{t('common.table.request')}</th>
              <th>{t('common.table.ms')}</th>
            </tr>
          </thead>
          <tbody>
            {filteredActivity.map((event) => {
              const requestId = event.correlation?.request_id;
              return (
                <tr key={event.event_id}>
                  <td>
                    <TimeValue value={event.timestamp} />
                  </td>
                  <td>
                    <StatusBadge value={event.status} />
                  </td>
                  <td>{event.kind}</td>
                  <td title={event.message}>{event.message}</td>
                  <td>{event.correlation?.dcc_type ?? '-'}</td>
                  <td>{event.correlation?.actor_name ?? event.correlation?.actor_id ?? '-'}</td>
                  <td>{event.correlation?.client_platform ?? '-'}</td>
                  <td>{event.correlation?.source_ip ?? '-'}</td>
                  <td>
                    {requestId ? (
                      <Button
                        variant="secondary"
                        size="xs"
                        type="button"
                        title={requestId}
                        onClick={() => onGoToPanel('traces', { traceId: requestId })}
                      >
                        {requestId.slice(0, 12)}
                      </Button>
                    ) : (
                      '-'
                    )}
                  </td>
                  <td>{event.duration_ms ?? '-'}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <Button type="button" size="sm" onClick={onRefresh}>
        <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
        {t('action.refresh')}
      </Button>
    </section>
  );
}
