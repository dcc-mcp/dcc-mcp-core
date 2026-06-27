import { useMemo } from 'react';
import { RiRefreshLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import {
  PanelHeader,
  StatusLine,
  MetricTile,
  StatusBadge,
  TimeValue,
  taskPrimaryRequestId,
  taskOutcomeText,
  taskRequestCount,
  taskWorkflowLabel,
  taskActorLabel,
  compactList,
  formatDurationMs,
  isErrStatus,
  isWarnStatus,
  isOkStatus,
} from '../../admin-ui-core';
import type { TaskRow, NavigateOptions, Panel, Translator } from '../../admin-types';

export type TasksPanelProps = {
  updatedAt: string;
  error?: string;
  tasks: TaskRow[];
  filteredTasks: TaskRow[];
  onGoToPanel: (panel: Panel, opts?: NavigateOptions) => void;
  onRefresh: () => void;
  t: Translator;
};

export function TasksPanel({
  updatedAt,
  error,
  tasks,
  filteredTasks,
  onGoToPanel,
  onRefresh,
  t,
}: TasksPanelProps) {
  const taskSummary = useMemo(() => {
    const completed = tasks.filter((task) => isOkStatus(task.status)).length;
    const failed = tasks.filter((task) => isErrStatus(task.status)).length;
    const active = tasks.filter((task) => isWarnStatus(task.status)).length;
    const total = tasks.length;
    const settled = completed + failed;
    const successRate = settled > 0 ? (completed / settled) * 100 : 0;
    const durations = tasks
      .map((task) => task.duration_ms)
      .filter((ms): ms is number => typeof ms === 'number' && ms >= 0);
    const avgDurationMs =
      durations.length > 0 ? durations.reduce((sum, ms) => sum + ms, 0) / durations.length : null;
    return { completed, failed, active, total, successRate, avgDurationMs };
  }, [tasks]);

  return (
    <section className="panel active tasks-panel">
      <PanelHeader
        title={t('tasks.title')}
        meta={t('tasks.meta')}
        action={
          <Button type="button" size="sm" onClick={onRefresh}>
            <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
            {t('action.refresh')}
          </Button>
        }
      />
      <StatusLine text={updatedAt} error={error} />
      <div className="metric-grid compact">
        <MetricTile label={t('common.metric.total')} value={taskSummary.total} />
        <MetricTile
          tone={taskSummary.successRate >= 80 || taskSummary.total === 0 ? 'ok' : 'warn'}
          label={t('common.metric.successRate')}
          value={`${taskSummary.successRate.toFixed(1)}%`}
          detail={t('stats.detail.okFailed', { ok: taskSummary.completed, failed: taskSummary.failed })}
        />
        <MetricTile tone="ok" label={t('tasks.metric.completed')} value={taskSummary.completed} />
        <MetricTile
          tone={taskSummary.failed > 0 ? 'err' : undefined}
          label={t('tasks.metric.failed')}
          value={taskSummary.failed}
        />
        <MetricTile
          tone={taskSummary.active > 0 ? 'warn' : undefined}
          label={t('tasks.metric.activeWaiting')}
          value={taskSummary.active}
        />
        <MetricTile label={t('common.metric.avgDuration')} value={formatDurationMs(taskSummary.avgDurationMs)} />
        <MetricTile label={t('common.metric.visible')} value={`${filteredTasks.length} / ${tasks.length}`} />
      </div>
      {tasks.length === 0 ? (
        <p className="empty">{t('tasks.empty.none')}</p>
      ) : filteredTasks.length === 0 ? (
        <p className="empty">{t('tasks.empty.search')}</p>
      ) : (
        <div className="task-board">
          {filteredTasks.map((task) => {
            const requestId = taskPrimaryRequestId(task);
            const tone = isErrStatus(task.status)
              ? 'err'
              : isWarnStatus(task.status)
              ? 'warn'
              : isOkStatus(task.status)
              ? 'ok'
              : 'muted';
            const outcome = taskOutcomeText(task);
            const requestCount = taskRequestCount(task);
            return (
              <article key={task.task_id} className={`task-card ${tone}`}>
                <div className="task-main">
                  <div className="task-title-row">
                    <StatusBadge value={task.status} />
                    <span className="task-type">{task.task_type}</span>
                    <TimeValue className="task-time" value={task.started_at} />
                    <span>{formatDurationMs(task.duration_ms)}</span>
                  </div>
                  <h3 title={task.title}>{task.title}</h3>
                  {task.goal && task.goal !== task.title ? (
                    <p className="task-outcome">
                      <strong>{t('tasks.label.goal')}</strong>
                      {task.goal}
                    </p>
                  ) : null}
                  {outcome ? (
                    <p className={`task-outcome ${tone === 'err' ? 'err' : ''}`}>
                      <strong>
                        {tone === 'err' ? t('tasks.label.failure') : t('tasks.label.result')}
                      </strong>
                      {outcome}
                    </p>
                  ) : null}
                  <div className="task-meta">
                    <span>{compactList(task.app_types, task.correlation?.dcc_type ?? 'gateway')}</span>
                    <span>{t('tasks.label.workflow', { id: taskWorkflowLabel(task) })}</span>
                    <span>{t('tasks.label.calls', { count: requestCount })}</span>
                    <span>{t('tasks.label.client', { value: taskActorLabel(task) })}</span>
                  </div>
                  {task.artifacts?.length ? (
                    <div className="task-chip-row" aria-label={t('tasks.label.artifacts')}>
                      {task.artifacts.map((artifact) => (
                        <span key={`${artifact.kind}-${artifact.name}-${artifact.request_id ?? ''}`}>
                          {artifact.kind}: {artifact.name}
                        </span>
                      ))}
                    </div>
                  ) : null}
                  {task.validation_checks?.length ? (
                    <div className="task-chip-row" aria-label={t('tasks.label.validation')}>
                      {task.validation_checks.map((check) => (
                        <span key={`${check.title}-${check.request_id ?? ''}`}>
                          {check.title} <StatusBadge value={check.status} />
                        </span>
                      ))}
                    </div>
                  ) : null}
                </div>
                <div className="task-side">
                  {requestId ? (
                    <button
                      className="link-chip"
                      type="button"
                      title={requestId}
                      onClick={() => onGoToPanel('traces', { traceId: requestId })}
                    >
                      {t('tasks.link.trace', { id: requestId.slice(0, 12) })}
                    </button>
                  ) : (
                    <span className="mono-path">{task.task_id.slice(0, 12)}</span>
                  )}
                  {task.related?.workflow_ids?.length ? (
                    <button className="link-chip" type="button" onClick={() => onGoToPanel('workflows')}>
                      {t('tasks.link.workflows', { count: task.related.workflow_ids.length })}
                    </button>
                  ) : null}
                  {requestCount ? (
                    <button
                      className="link-chip"
                      type="button"
                      onClick={() => onGoToPanel('traces', { tracesTab: 'calls' })}
                    >
                      {t('tasks.link.calls', { count: requestCount })}
                    </button>
                  ) : null}
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
