import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { TraceDetailPayload, TraceSpan } from '../../admin-types';
import { createTranslator } from '../../i18n';
import { TraceDetailPanel } from './TraceDetailPanel';
import { recordedSpanTiming } from './trace-timing';

afterEach(cleanup);
const t = createTranslator('en');
const trace: TraceDetailPayload = {
  request_id: 'request-1', method: 'tools/call', tool_slug: 'inspect_scene', total_ms: 400, ok: false,
  spans: [
    { name: 'dispatch', started_ns: 2_000_000_000, duration_ns: 300_000_000, ok: true, attributes: { instance: 'maya-one' } },
    { name: 'response', started_ns: 2_050_000_000, duration_ns: 100_000_000, ok: false, attributes: { reason: 'unavailable' } },
  ],
  agent_context: { agent_name: 'Scene assistant', task: 'Inspect the scene.', plan: ['Inspect nodes'], reasoning_summary: 'Use the recorded result.' },
};
const callbacks = { onCopy: vi.fn(), onCopyIssueReport: vi.fn(), onDownloadIssueReport: vi.fn() };

describe('recorded span timing', () => {
  it('preserves overlapping timestamps and durations instead of synthesizing a serial timeline', () => {
    const timing = recordedSpanTiming(trace.spans);
    expect(timing.rows.map((row) => row.offsetMs)).toEqual([0, 50]);
    expect(timing.rows.map((row) => row.durationMs)).toEqual([300, 100]);
    expect(timing.extentMs).toBe(300);
  });
  it('keeps missing or invalid timing unknown and preserves a recorded zero duration', () => {
    const rows = [
      { name: 'missing', ok: true },
      { name: 'invalid', started_ns: -1, duration_ns: NaN, ok: true },
      { name: 'instant', started_ns: 100, duration_ns: 0, ok: true },
    ] as TraceSpan[];
    const timing = recordedSpanTiming(rows);
    expect(timing.rows.map((row) => [row.offsetMs, row.durationMs])).toEqual([[null, null], [null, null], [0, 0]]);
    expect(timing.extentMs).toBe(0);
  });
});

describe('TraceDetailPanel', () => {
  it('filters real steps by status and attributes and expands the recorded details', () => {
    render(<TraceDetailPanel trace={trace} fallback="Select a trace" t={t} {...callbacks} />);
    const list = document.querySelector('.trace-step-list')!;
    expect(list.querySelectorAll('details')).toHaveLength(2);
    fireEvent.change(screen.getByLabelText('Search steps'), { target: { value: 'unavailable' } });
    expect(list.querySelectorAll('details')).toHaveLength(1);
    expect(within(list as HTMLElement).getByText('response')).toBeVisible();
    fireEvent.change(screen.getByLabelText('Filter step status'), { target: { value: 'failed' } });
    fireEvent.click(list.querySelector('summary')!);
    expect(within(list as HTMLElement).getByText(/"reason": "unavailable"/)).toBeVisible();
    fireEvent.change(screen.getByLabelText('Search steps'), { target: { value: 'missing' } });
    expect(screen.getByText('No steps match these filters.')).toBeVisible();
  });
  it('does not turn agent text into model timing or invent absent token measurements', () => {
    render(<TraceDetailPanel trace={trace} fallback="" t={t} {...callbacks} />);
    expect(document.querySelectorAll('.trace-timing-row')).toHaveLength(2);
    expect(screen.queryByText('LLM Operations')).not.toBeInTheDocument();
    expect(screen.queryByText('Total tokens')).not.toBeInTheDocument();
    expect(screen.getByText('Inspect nodes')).not.toBeVisible();
    fireEvent.click(document.querySelector('.agent-context-card > summary')!);
    expect(screen.getByText('Inspect nodes')).toBeVisible();
    expect(screen.getByText('Use the recorded result.')).toBeVisible();
  });
  it('resets filters for another request and preserves exports behind evidence details', () => {
    const { rerender } = render(<TraceDetailPanel trace={trace} fallback="" t={t} {...callbacks} />);
    fireEvent.change(screen.getByLabelText('Search steps'), { target: { value: 'nothing' } });
    rerender(<TraceDetailPanel trace={{ ...trace, request_id: 'request-2' }} fallback="" t={t} {...callbacks} />);
    expect(screen.getByLabelText('Search steps')).toHaveValue('');
    fireEvent.click(document.querySelector('.trace-evidence-section > summary')!);
    fireEvent.click(screen.getByRole('button', { name: 'Copy issue JSON' }));
    expect(callbacks.onCopyIssueReport).toHaveBeenCalledWith('request-2');
  });
  it('keeps payload estimates distinct from request-level token measurements', () => {
    render(<TraceDetailPanel trace={{ ...trace, input_tokens: 120, input: { content: '{}', mime_type: 'application/json', original_size: 2, truncated: false, estimated_tokens: 80 } }} fallback="" t={t} {...callbacks} />);
    fireEvent.click(document.querySelector('.trace-payload-section > summary')!);
    expect(screen.getByText('Estimated payload tokens: 80')).toBeVisible();
    fireEvent.click(document.querySelector('.trace-record-section > summary')!);
    const input = screen.getByText('Input tokens').parentElement!;
    expect(input).toHaveTextContent('120');
  });
});
