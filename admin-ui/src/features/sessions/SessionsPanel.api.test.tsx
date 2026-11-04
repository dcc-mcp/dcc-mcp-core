import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createTranslator } from '../../i18n';
import { SessionsPanel } from './SessionsPanel';

const clients: QueryClient[] = [];
afterEach(() => {
  cleanup();
  clients.splice(0).forEach((client) => client.clear());
  vi.unstubAllGlobals();
});

function renderWirePayload(payload: unknown) {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify(payload), {
    headers: { 'content-type': 'application/json' },
  })));
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  clients.push(client);
  return render(<QueryClientProvider client={client}>
    <SessionsPanel active tab="sessions" onTabChange={vi.fn()} onOpenMemory={vi.fn()} t={createTranslator('en')} />
  </QueryClientProvider>);
}

describe('SessionsPanel production response contract', () => {
  it('renders the API summary and persistence row instead of requiring a mock-only kpi', async () => {
    const { container } = renderWirePayload({ total: 1,
      summary: { active: 0, ended: 0, crashed: 1, disconnected: 0, by_dcc: { maya: 1 } },
      sessions: [{ session_id: 'session-wire', parent_session_id: null, status: 'crashed',
        dcc_type: 'maya', instance_id: 'maya-one', started_at_ms: 1779091200000,
        last_activity_at_ms: 1779091201000, ended_at_ms: 1779091201000, tool_call_count: 3,
        error_count: 1, core_version: '0.20.45', adapter_version: null, build_sha: null,
        end_reason_json: '{"host_crash":{"detail":"render stopped"}}' }],
    });
    const opener = await screen.findByTitle('session-wire');
    expect(container.querySelector('.sessions-summary')).toHaveTextContent('1');
    expect(container.querySelector('time')).toHaveAttribute('datetime', '2026-05-18T08:00:00.000Z');
    expect(container.querySelector('.session-activity small')).toHaveTextContent('— Turns');
    fireEvent.click(opener.closest('button')!);
    const detail = container.querySelector('.sessions-detail-row')!;
    expect(detail).toHaveTextContent('0.20.45');
    expect(detail).toHaveTextContent('render stopped');
    expect(detail).toHaveTextContent('maya-one');
  });

  it('renders an empty real response without crashing the dashboard', async () => {
    const { container } = renderWirePayload({ total: 0, sessions: [],
      summary: { active: 0, ended: 0, crashed: 0, disconnected: 0, by_dcc: {} },
    });
    await screen.findByText('No sessions recorded yet.');
    expect(container.querySelectorAll('.sessions-summary dd')).toHaveLength(4);
    expect(Array.from(container.querySelectorAll('.sessions-summary dd'), (node) => node.textContent)).toEqual(['0', '0', '0', '0']);
  });
});
