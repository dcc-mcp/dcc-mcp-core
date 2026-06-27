import { useMemo } from 'react';
import { RiRefreshLine } from '@remixicon/react';
import { Button } from '../../components/ui/button';
import {
  PanelHeader,
  StatusLine,
  MetricTile,
  WorkflowGraphDetail,
  WorkflowCard,
  isOkStatus,
  isErrStatus,
  isWarnStatus,
} from '../../admin-ui-core';
import type { WorkflowRow, NavigateOptions, Panel, Translator } from '../../admin-types';

export type WorkflowsPanelProps = {
  updatedAt: string;
  error?: string;
  workflows: WorkflowRow[];
  filteredWorkflows: WorkflowRow[];
  selectedWorkflowId: string | null;
  onSelectWorkflowId: (id: string | null) => void;
  onGoToPanel: (panel: Panel, opts?: NavigateOptions) => void;
  onCopyIssueReport: (requestId: string) => void;
  onRefresh: () => void;
  copiedNotice: string;
  t: Translator;
};

export function WorkflowsPanel({
  updatedAt,
  error,
  workflows,
  filteredWorkflows,
  selectedWorkflowId,
  onSelectWorkflowId,
  onGoToPanel,
  onCopyIssueReport,
  onRefresh,
  copiedNotice,
  t,
}: WorkflowsPanelProps) {
  const workflowSummary = useMemo(() => {
    const completed = workflows.filter((workflow) => isOkStatus(workflow.status)).length;
    const failed = workflows.filter((workflow) => isErrStatus(workflow.status)).length;
    const warning = workflows.filter((workflow) => isWarnStatus(workflow.status)).length;
    const zeroResults = workflows.filter((workflow) => workflow.discovery.zero_result_count > 0).length;
    const total = workflows.length;
    const settled = completed + failed;
    const successRate = settled > 0 ? (completed / settled) * 100 : 0;
    const searches = workflows.reduce((sum, workflow) => sum + (workflow.discovery.search_count ?? 0), 0);
    const totalSteps = workflows.reduce((sum, workflow) => sum + (workflow.step_count ?? 0), 0);
    const avgSteps = total > 0 ? totalSteps / total : 0;
    return { completed, failed, warning, zeroResults, total, successRate, searches, avgSteps };
  }, [workflows]);

  const visibleSelectedWorkflow = useMemo(
    () => workflows.find((w) => w.workflow_id === selectedWorkflowId),
    [workflows, selectedWorkflowId]
  );

  return (
    <section className="panel active workflows-panel">
      <PanelHeader
        title={t('workflows.title')}
        meta={t('workflows.meta')}
        action={
          <Button type="button" size="sm" onClick={onRefresh}>
            <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
            {t('action.refresh')}
          </Button>
        }
      />
      <StatusLine text={copiedNotice || updatedAt} error={error} />
      <div className="metric-grid compact">
        <MetricTile label={t('common.metric.total')} value={workflowSummary.total} />
        <MetricTile
          tone={workflowSummary.successRate >= 80 || workflowSummary.total === 0 ? 'ok' : 'warn'}
          label={t('common.metric.successRate')}
          value={`${workflowSummary.successRate.toFixed(1)}%`}
          detail={t('stats.detail.okFailed', { ok: workflowSummary.completed, failed: workflowSummary.failed })}
        />
        <MetricTile tone="ok" label={t('workflows.metric.completed')} value={workflowSummary.completed} />
        <MetricTile
          tone={workflowSummary.warning > 0 ? 'warn' : undefined}
          label={t('workflows.metric.warnings')}
          value={workflowSummary.warning}
        />
        <MetricTile
          tone={workflowSummary.failed > 0 ? 'err' : undefined}
          label={t('workflows.metric.failed')}
          value={workflowSummary.failed}
        />
        <MetricTile
          tone={workflowSummary.zeroResults > 0 ? 'warn' : undefined}
          label={t('workflows.metric.zeroResult')}
          value={workflowSummary.zeroResults}
        />
        <MetricTile
          label={t('workflows.metric.searches')}
          value={workflowSummary.searches}
          detail={t('workflows.metric.avgSteps', { value: workflowSummary.avgSteps.toFixed(1) })}
        />
        <MetricTile label={t('common.metric.visible')} value={`${filteredWorkflows.length} / ${workflows.length}`} />
      </div>
      {visibleSelectedWorkflow ? (
        <WorkflowGraphDetail
          workflow={visibleSelectedWorkflow}
          onClose={() => onSelectWorkflowId(null)}
          onOpenTrace={(requestId) => onGoToPanel('traces', { traceId: requestId })}
          onCopyIssueReport={onCopyIssueReport}
          t={t}
        />
      ) : null}
      {workflows.length === 0 ? (
        <p className="empty">{t('workflows.empty.none')}</p>
      ) : filteredWorkflows.length === 0 ? (
        <p className="empty">{t('workflows.empty.search')}</p>
      ) : (
        <div className="workflow-board">
          {filteredWorkflows.map((workflow) => (
            <WorkflowCard
              key={`${workflow.group_kind}-${workflow.workflow_id}`}
              workflow={workflow}
              onInspect={onSelectWorkflowId}
              onOpenTrace={(requestId) => onGoToPanel('traces', { traceId: requestId })}
              onCopyIssueReport={onCopyIssueReport}
              t={t}
            />
          ))}
        </div>
      )}
    </section>
  );
}
