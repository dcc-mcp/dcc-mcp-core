import { useMemo } from 'react';
import { RiArrowDownSLine, RiRefreshLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import {
  PanelHeader,
  StatusLine,
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
import './tasks.css';

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
      <dl className="task-summary" aria-label={t('tasks.title')}>
        <div><dt>{t('common.metric.total')}</dt><dd>{taskSummary.total}</dd></div>
        <div><dt>{t('tasks.metric.completed')}</dt><dd>{taskSummary.completed}</dd></div>
        <div><dt>{t('tasks.metric.failed')}</dt><dd>{taskSummary.failed}</dd></div>
        <div><dt>{t('tasks.metric.activeWaiting')}</dt><dd>{taskSummary.active}</dd></div>
        <div><dt>{t('common.metric.successRate')}</dt><dd>{taskSummary.successRate.toFixed(1)}%</dd></div>
        <div><dt>{t('common.metric.avgDuration')}</dt><dd>{formatDurationMs(taskSummary.avgDurationMs)}</dd></div>
        <div><dt>{t('common.metric.visible')}</dt><dd>{filteredTasks.length} / {tasks.length}</dd></div>
      </dl>
      {tasks.length === 0 ? (
        <p className="empty">{t('tasks.empty.none')}</p>
      ) : filteredTasks.length === 0 ? (
        <p className="empty">{t('tasks.empty.search')}</p>
      ) : (
        <div className="task-board">
          {filteredTasks.map((task) => {
            const requestId = taskPrimaryRequestId(task);
            const tone = isErrStatus(task.status) ? 'err' : isWarnStatus(task.status) ? 'warn' : isOkStatus(task.status) ? 'ok' : 'muted';
            const outcome = taskOutcomeText(task);
            const requestCount = taskRequestCount(task);
            return (
              <details key={task.task_id} className={`task-card ${tone}`}>
                <summary className="task-title-row">
                  <span className="task-row-copy">
                    <strong className="task-title">{task.title}</strong>
                    <span className="task-meta">
                      <span>{task.task_type}</span>
                      <span>{compactList(task.app_types, task.correlation?.dcc_type ?? 'gateway')}</span>
                      <TimeValue value={task.started_at} />
                      <span>{formatDurationMs(task.duration_ms)}</span>
                    </span>
                  </span>
                  <StatusBadge value={task.status} />
                  <RiArrowDownSLine className="task-disclosure-icon" aria-hidden="true" />
                </summary>
                <div className="task-detail">
                  {task.goal && task.goal !== task.title ? (
                    <p className="task-outcome"><strong>{t('tasks.label.goal')}</strong>{task.goal}</p>
                  ) : null}
                  {outcome ? (
                    <p className="task-outcome"><strong>{tone === 'err' ? t('tasks.label.failure') : t('tasks.label.result')}</strong>{outcome}</p>
                  ) : null}
                  <div className="task-meta">
                    <span>{t('tasks.label.workflow', { id: taskWorkflowLabel(task) })}</span>
                    <span>{t('tasks.label.calls', { count: requestCount })}</span>
                    <span>{t('tasks.label.client', { value: taskActorLabel(task) })}</span>
                    <code>{task.task_id}</code>
                  </div>
                  {task.artifacts?.length ? (
                    <section className="task-detail-section" aria-label={t('tasks.label.artifacts')}>
                      <h3>{t('tasks.label.artifacts')}</h3>
                      <ul>{task.artifacts.map((artifact) => (
                        <li key={`${artifact.kind}-${artifact.name}-${artifact.request_id ?? ''}`}>{artifact.kind}: {artifact.name}</li>
                      ))}</ul>
                    </section>
                  ) : null}
                  {task.validation_checks?.length ? (
                    <section className="task-detail-section" aria-label={t('tasks.label.validation')}>
                      <h3>{t('tasks.label.validation')}</h3>
                      <ul>{task.validation_checks.map((check) => (
                        <li key={`${check.title}-${check.request_id ?? ''}`}><span>{check.title}</span><StatusBadge value={check.status} /></li>
                      ))}</ul>
                    </section>
                  ) : null}
                  <div className="task-side">
                    {requestId ? (
                      <Button variant="outline" size="sm" type="button" title={requestId} onClick={() => onGoToPanel('traces', { traceId: requestId })}>
                        {t('tasks.link.trace', { id: requestId.slice(0, 12) })}
                      </Button>
                    ) : null}
                    {task.related?.workflow_ids?.length ? (
                      <Button variant="ghost" size="sm" type="button" onClick={() => onGoToPanel('workflows')}>
                        {t('tasks.link.workflows', { count: task.related.workflow_ids.length })}
                      </Button>
                    ) : null}
                    {requestCount ? (
                      <Button variant="ghost" size="sm" type="button" onClick={() => onGoToPanel('traces', { tracesTab: 'calls' })}>
                        {t('tasks.link.calls', { count: requestCount })}
                      </Button>
                    ) : null}
                  </div>
                </div>
              </details>
            );
          })}
        </div>
      )}
    </section>
  );
}
