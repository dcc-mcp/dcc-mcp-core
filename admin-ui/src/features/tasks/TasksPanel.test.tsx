import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { TaskRow } from '../../admin-types';
import { createTranslator } from '../../i18n';
import { TasksPanel } from './TasksPanel';

afterEach(cleanup);

describe('TasksPanel', () => {
  it('keeps outcomes, artifacts, validation, and navigation inside progressive detail', () => {
    const task: TaskRow = {
      task_id: 'task-one', task_type: 'render', title: 'Render preview', status: 'failed',
      started_at: '2026-10-15T08:00:00Z', duration_ms: 1200, goal: 'Produce a preview',
      failure_reason: 'The render worker disconnected.', app_types: ['blender'],
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
