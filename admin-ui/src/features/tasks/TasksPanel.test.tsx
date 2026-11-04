import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { TaskRow } from '../../admin-types';
import { createTranslator } from '../../i18n';
import { TasksPanel } from './TasksPanel';

afterEach(cleanup);

describe('TasksPanel', () => {
  it.each(['queued', 'running', 'completed'])('does not present a %s task goal as a recorded result', (status) => {
    const task: TaskRow = {
      task_id: 'goal-only', task_type: 'agent_turn', title: 'Prepare a preview', status,
      started_at: '2026-10-15T08:00:00Z', goal: 'Create a reviewable scene.',
    };
    render(<TasksPanel updatedAt="" tasks={[task]} filteredTasks={[task]} onRefresh={vi.fn()} onGoToPanel={vi.fn()} t={createTranslator('en')} />);
    fireEvent.click(screen.getByText(task.title).closest('summary')!);
    expect(screen.getByText('Goal', { exact: true })).toBeVisible();
    expect(screen.getByText(task.goal!, { exact: true })).toBeVisible();
    expect(screen.queryByText('Result', { exact: true })).not.toBeInTheDocument();
  });

  it.each([
    { status: 'running', goal: 'Inspect the target scene.', summary: 'Inspect the target scene.', distinct: false },
    { status: 'running', goal: undefined, summary: 'Inspect the target scene.', distinct: true },
    { status: 'failed', goal: undefined, summary: 'Inspect the target scene.', distinct: true },
  ])('keeps user-intent summary separate from outcomes: $status / $distinct', ({ distinct, ...intent }) => {
    const task: TaskRow = {
      task_id: 'intent-only', task_type: 'agent_turn', title: 'Prepare a preview',
      started_at: '2026-10-15T08:00:00Z', ...intent,
    };
    render(<TasksPanel updatedAt="" tasks={[task]} filteredTasks={[task]} onRefresh={vi.fn()} onGoToPanel={vi.fn()} t={createTranslator('en')} />);
    fireEvent.click(screen.getByText(task.title).closest('summary')!);
    expect(screen.getByText(task.summary!, { exact: true })).toBeVisible();
    expect(screen.queryByText('Result', { exact: true })).not.toBeInTheDocument();
    expect(screen.queryByText('Failure', { exact: true })).not.toBeInTheDocument();
    expect(screen.queryByText('Summary', { exact: true }) !== null).toBe(distinct);
  });

  it('shows a recorded final result while preserving a distinct intent summary', () => {
    const task: TaskRow = {
      task_id: 'recorded-result', task_type: 'agent_turn', title: 'Prepare a preview', status: 'completed',
      started_at: '2026-10-15T08:00:00Z', goal: 'Create a reviewable scene.',
      summary: 'Inspect the target scene.', final_result: 'The scene is ready.',
    };
    render(<TasksPanel updatedAt="" tasks={[task]} filteredTasks={[task]} onRefresh={vi.fn()} onGoToPanel={vi.fn()} t={createTranslator('en')} />);
    fireEvent.click(screen.getByText(task.title).closest('summary')!);
    const result = screen.getByText('Result', { exact: true }).closest('p')!;
    expect(result).toBeVisible();
    expect(result).toHaveTextContent(task.final_result!);
    expect(result).not.toHaveTextContent(task.goal!);
    expect(result).not.toHaveTextContent(task.summary!);
    expect(screen.getByText('Summary', { exact: true }).closest('p')).toHaveTextContent(task.summary!);
  });

  it('keeps outcomes, artifacts, validation, and navigation inside progressive detail', () => {
    const task: TaskRow = {
      task_id: 'task-one', task_type: 'render', title: 'Render preview', status: 'failed',
      started_at: '2026-10-15T08:00:00Z', duration_ms: 1200, goal: 'Produce a preview',
      failure_reason: 'The render worker disconnected.', app_types: ['blender'],
      final_result: 'The preview was queued before the failure.',
      artifacts: [{ kind: 'image', name: 'preview.png' }],
      validation_checks: [{ title: 'Output exists', status: 'failed' }],
      related: { request_ids: ['req-one'], workflow_ids: ['flow-one'] },
    };
    const onGoToPanel = vi.fn();
    render(<TasksPanel updatedAt="" tasks={[task]} filteredTasks={[task]} onRefresh={vi.fn()} onGoToPanel={onGoToPanel} t={createTranslator('en')} />);
    const details = screen.getByText('Render preview').closest('details')!;
    expect(details).not.toHaveAttribute('open');
    expect(screen.getByText('The render worker disconnected.')).not.toBeVisible();
    expect(within(details.querySelector('summary')!).getByText('failed')).toBeVisible();
    fireEvent.click(details.querySelector('summary')!);
    expect(screen.getByText('The render worker disconnected.')).toBeVisible();
    expect(screen.queryByText(task.final_result!, { exact: true })).not.toBeInTheDocument();
    expect(screen.getByText('image: preview.png')).toBeVisible();
    expect(screen.getByText('Output exists')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'trace req-one' }));
    expect(onGoToPanel).toHaveBeenLastCalledWith('traces', { traceId: 'req-one' });
    fireEvent.click(screen.getByRole('button', { name: '1 workflow(s)' }));
    expect(onGoToPanel).toHaveBeenLastCalledWith('workflows');
    fireEvent.click(screen.getByRole('button', { name: '1 call(s)' }));
    expect(onGoToPanel).toHaveBeenLastCalledWith('traces', { tracesTab: 'calls' });
  });
});
