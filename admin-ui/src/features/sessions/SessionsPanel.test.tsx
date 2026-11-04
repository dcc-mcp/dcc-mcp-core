import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useQuery } from '@tanstack/react-query';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createTranslator } from '../../i18n';
import { SessionsPanel } from './SessionsPanel';

vi.mock('@tanstack/react-query', () => ({ useQuery: vi.fn() }));
afterEach(cleanup);

const session = { dcc_type: 'blender', instance_id: 'instance-one', status: 'ended', started_at: '2026-10-15T08:00:00Z',
  ended_at: '2026-10-15T08:00:01Z', duration_ms: 1000, turn_count: 2, tool_call_count: 3, version: '0.20.45' };

function setup() {
  vi.mocked(useQuery).mockReturnValue({ data: { sessions: [
    { ...session, session_id: 'root', status: 'active' },
    { ...session, session_id: 'child', parent_session_id: 'root', agent_name: 'layout', end_reason: 'completed' },
    { ...session, session_id: 'grandchild', parent_session_id: 'child', agent_name: 'lighting' },
    { ...session, session_id: 'orphan', parent_session_id: 'missing' },
  ], total: 4, kpi: { total: 4, active: 1, ended: 3, crashed: 0, by_dcc: { blender: 4 } } },
  isLoading: false, error: null, refetch: vi.fn() } as unknown as ReturnType<typeof useQuery>);
  const onOpenMemory = vi.fn();
  const result = render(<SessionsPanel active tab="sessions" onTabChange={vi.fn()} onOpenMemory={onOpenMemory} t={createTranslator('en')} />);
  return { ...result, onOpenMemory };
}

describe('SessionsPanel', () => {
  it('collapses all descendants while keeping independent and filtered sessions reachable', () => {
    const { container } = setup();
    const rows = () => container.querySelectorAll('.sessions-row');
    expect(rows()).toHaveLength(4);
    fireEvent.click(screen.getByRole('button', { name: 'Toggle child sessions of root' }));
    expect(rows()).toHaveLength(2);
    expect(screen.getByTitle('orphan')).toBeVisible();
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search sessions' }), { target: { value: 'lighting' } });
    expect(rows()).toHaveLength(1);
    expect(screen.getByTitle('grandchild')).toBeVisible();
  });

  it('keeps session identity, parent, version, end reason, and context access in detail', () => {
    const { onOpenMemory } = setup();
    fireEvent.click(screen.getByTitle('child').closest('button')!);
    const detail = document.getElementById('session-detail-child')!;
    expect(detail).toHaveTextContent('root');
    expect(detail).toHaveTextContent('0.20.45');
    expect(detail).toHaveTextContent('completed');
    expect(detail).toHaveTextContent('instance-one');
    fireEvent.click(screen.getByRole('button', { name: 'View context' }));
    expect(onOpenMemory).toHaveBeenCalledWith('child');
  });
});
