import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createTranslator } from '../i18n';
import { filterLogs, summarizeLogSeverity, type LogRow } from '../logs';
import { LogsPanel } from './LogsPanel';

afterEach(cleanup);

const rows: LogRow[] = [
  {
    timestamp: '2026-10-15T12:00:02Z', level: 'ERROR', message: 'Tool failed',
    request_id: 'req-123', tool: 'create_sphere', dcc_type: 'maya', event: 'tools/call',
    source: 'executor', target: 'dcc_mcp_http_server:executor', thread: 'ThreadId(01)',
    instance_id: 'maya-1234', detail: 'No active scene', reason: 'Scene is unavailable', success: false,
  },
  {
    timestamp: '2026-10-15T12:00:00Z', level: 'INFO', message: 'Dispatch started',
    request_id: 'req-123', tool: 'create_sphere', dcc_type: 'maya', event: 'dispatch',
  },
  {
    timestamp: '2026-10-15T12:00:03Z', level: 'DEBUG', message: 'Registry refreshed',
    source: 'gateway', target: 'registry', thread: 'ThreadId(02)',
  },
];

const props = {
  logs: rows,
  filteredLogs: rows,
  severityCounts: summarizeLogSeverity(rows),
  severityFilter: 'all' as const,
  serverVersion: '0.20.45',
  updatedAt: '',
  onSeverityFilterChange: vi.fn(),
  onRefresh: vi.fn(),
  t: createTranslator('en'),
};

describe('LogsPanel', () => {
  it('keeps requests compact and exposes ordered steps with exact metadata on expansion', () => {
    const { container } = render(<LogsPanel {...props} />);
    const request = container.querySelector<HTMLDetailsElement>('.log-request')!;
    const summary = request.querySelector('summary')!;
    expect(request.open).toBe(false);
    expect(summary).toHaveTextContent('Request req-123');
    expect(summary).toHaveTextContent('Steps: 2');
    expect(summary.querySelectorAll('.severity-badge')).toHaveLength(1);
    expect(summary.querySelector('time')).toHaveAttribute('datetime', rows[0].timestamp);

    fireEvent.click(summary);
    expect(request.open).toBe(true);
    const steps = Array.from(request.querySelectorAll<HTMLDetailsElement>('.log-entry'));
    expect(steps).toHaveLength(2);
    expect(steps[0].querySelector('summary')).toHaveTextContent('Step 1: dispatch');
    expect(steps[1].querySelector('summary')).toHaveTextContent('Step 2: tools/call');
    expect(steps[1].querySelector('summary')!.querySelectorAll('.severity-badge')).toHaveLength(0);

    const metadata = steps[1].querySelector('.log-entry-metadata')!;
    expect(metadata).not.toBeVisible();
    fireEvent.click(steps[1].querySelector('summary')!);
    expect(metadata).toBeVisible();
    expect(metadata).toHaveTextContent('dcc_mcp_http_server:executor');
    expect(metadata).toHaveTextContent('ThreadId(01)');
    expect(metadata).toHaveTextContent('maya-1234');
    expect(steps[1]).toHaveTextContent('No active scene');
    expect(steps[1]).toHaveTextContent('Scene is unavailable');
    expect(container.querySelectorAll('time')).toHaveLength(4);
  });

  it('retains gateway events, search results, and severity and refresh callbacks', () => {
    const onSeverityFilterChange = vi.fn();
    const onRefresh = vi.fn();
    const { rerender } = render(<LogsPanel {...props} onSeverityFilterChange={onSeverityFilterChange} onRefresh={onRefresh} />);
    const gateway = screen.getByRole('region', { name: 'Gateway events' });
    expect(gateway).toHaveTextContent('Registry refreshed');
    expect(gateway.querySelectorAll('summary .severity-badge')).toHaveLength(1);
    fireEvent.click(within(screen.getByRole('group', { name: 'Filter logs by severity' })).getByRole('button', { name: /Error/ }));
    expect(onSeverityFilterChange).toHaveBeenCalledWith('error');
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(onRefresh).toHaveBeenCalledOnce();

    rerender(<LogsPanel {...props} filteredLogs={filterLogs(rows, 'ThreadId(01)', 'all')} />);
    expect(screen.getByRole('region', { name: 'Requests' })).toHaveTextContent('Steps: 1');
    expect(screen.queryByRole('region', { name: 'Gateway events' })).not.toBeInTheDocument();
    expect(screen.queryByText('Dispatch started')).not.toBeInTheDocument();

    rerender(<LogsPanel {...props} filteredLogs={filterLogs(rows, 'no-such-event', 'all')} />);
    expect(screen.getByText('No log lines match your search.')).toBeVisible();
  });

  it('does not create timestamps for events without one and keeps errors actionable', () => {
    const untimed = [{ timestamp: '', level: 'WARN', message: 'Waiting for registry' }];
    const { container } = render(<LogsPanel {...props} logs={untimed} filteredLogs={untimed} error="Gateway unavailable" />);
    expect(container.querySelector('time')).toBeNull();
    expect(screen.getByRole('alert')).toHaveTextContent('Gateway unavailable');
    expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled();
    expect(container).not.toHaveTextContent('undefined');
  });
});
