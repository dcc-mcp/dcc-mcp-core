import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  computeBackoffDelay,
  normalizeEvent,
  parseSSEMessageData,
  useRealtime,
  type RealtimeEvent,
} from './useRealtime';

// React 19's `act` checks this flag to avoid warning outside of a
// recognized test runner.
(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// ── EventSource mock ─────────────────────────────────────────────────────

class MockEventSource {
  static instances: MockEventSource[] = [];

  url: string;
  readyState = 0;
  closed = false;
  onopen: ((ev: Event) => void) | null = null;
  onmessage: ((ev: MessageEvent) => void) | null = null;
  onerror: ((ev: Event) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }

  close() {
    this.closed = true;
    this.readyState = 2;
  }

  emitOpen() {
    this.readyState = 1;
    this.onopen?.(new Event('open'));
  }

  emitMessage(data: unknown, lastEventId = '') {
    const payload = typeof data === 'string' ? data : JSON.stringify(data);
    this.onmessage?.({ data: payload, lastEventId } as MessageEvent);
  }

  emitError() {
    this.onerror?.(new Event('error'));
  }

  static latest(): MockEventSource {
    const instance = MockEventSource.instances[MockEventSource.instances.length - 1];
    if (!instance) throw new Error('No MockEventSource instance was created');
    return instance;
  }

  static reset() {
    MockEventSource.instances = [];
  }
}

// ── test harness (no @testing-library/react in this project) ───────────

interface Harness<T> {
  result: { current: T };
  queryClient: QueryClient;
  unmount: () => void;
}

const activeRoots: { root: Root; container: HTMLElement }[] = [];

function renderHook<T>(callback: () => T): Harness<T> {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const result = { current: undefined as unknown as T };

  function TestComponent() {
    result.current = callback();
    return null;
  }

  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  activeRoots.push({ root, container });

  act(() => {
    root.render(createElement(QueryClientProvider, { client: queryClient }, createElement(TestComponent)));
  });

  return {
    result,
    queryClient,
    unmount: () => {
      act(() => {
        root.unmount();
      });
      container.remove();
    },
  };
}

function flushMicrotasks(): Promise<unknown> {
  return vi.isFakeTimers() ? vi.advanceTimersByTimeAsync(0) : new Promise((resolve) => setTimeout(resolve, 0));
}

function setVisibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, 'visibilityState', {
    value: state,
    configurable: true,
  });
  document.dispatchEvent(new Event('visibilitychange'));
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

// ── setup / teardown ─────────────────────────────────────────────────────

let fetchMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  MockEventSource.reset();
  vi.stubGlobal('EventSource', MockEventSource as unknown as typeof EventSource);
  fetchMock = vi.fn(async () => jsonResponse({ events: [] }));
  vi.stubGlobal('fetch', fetchMock);
  setVisibility('visible');
});

afterEach(() => {
  while (activeRoots.length) {
    const entry = activeRoots.pop()!;
    act(() => {
      entry.root.unmount();
    });
    entry.container.remove();
  }
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

// ── pure helpers ─────────────────────────────────────────────────────────

describe('computeBackoffDelay', () => {
  it('doubles the delay for each attempt', () => {
    expect(computeBackoffDelay(0, 1000, 30000)).toBe(1000);
    expect(computeBackoffDelay(1, 1000, 30000)).toBe(2000);
    expect(computeBackoffDelay(2, 1000, 30000)).toBe(4000);
    expect(computeBackoffDelay(3, 1000, 30000)).toBe(8000);
    expect(computeBackoffDelay(4, 1000, 30000)).toBe(16000);
  });

  it('caps the delay at maxMs', () => {
    expect(computeBackoffDelay(5, 1000, 30000)).toBe(30000);
    expect(computeBackoffDelay(10, 1000, 30000)).toBe(30000);
  });
});

describe('normalizeEvent', () => {
  it('extracts type/data/timestamp/id from a well-formed payload', () => {
    const event = normalizeEvent({ type: 'health', data: { ok: true }, timestamp: '2026-01-01T00:00:00Z', id: 'evt-1' });
    expect(event).toEqual({ type: 'health', data: { ok: true }, timestamp: '2026-01-01T00:00:00Z', id: 'evt-1' });
  });

  it('falls back to type "message" for scalar payloads', () => {
    expect(normalizeEvent('plain text', 'fallback-id')).toEqual({ type: 'message', data: 'plain text', id: 'fallback-id' });
  });

  it('treats an object without a type field as its own data payload', () => {
    const event = normalizeEvent({ foo: 'bar' });
    expect(event.type).toBe('message');
    expect(event.data).toEqual({ foo: 'bar' });
  });
});

describe('parseSSEMessageData', () => {
  it('parses JSON SSE payloads', () => {
    expect(parseSSEMessageData(JSON.stringify({ type: 'tools', data: { count: 3 } }), 'evt-2'))
      .toEqual({ type: 'tools', data: { count: 3 }, id: 'evt-2' });
  });

  it('falls back to the raw string when the payload is not JSON', () => {
    expect(parseSSEMessageData('not-json', 'evt-3')).toEqual({ type: 'message', data: 'not-json', id: 'evt-3' });
  });
});

// ── hook behavior ────────────────────────────────────────────────────────

describe('useRealtime', () => {
  it('opens an SSE connection to the events endpoint on mount', () => {
    const { result } = renderHook(() => useRealtime());

    expect(MockEventSource.instances).toHaveLength(1);
    expect(MockEventSource.latest().url).toContain('/events');
    expect(result.current.isConnected).toBe(false);
  });

  it('respects a custom path', () => {
    renderHook(() => useRealtime({ path: '/custom-events' }));

    expect(MockEventSource.latest().url).toContain('/custom-events');
  });

  it('marks the connection as open once the EventSource fires onopen', () => {
    const { result } = renderHook(() => useRealtime());

    act(() => {
      MockEventSource.latest().emitOpen();
    });

    expect(result.current.isConnected).toBe(true);
    expect(result.current.error).toBeNull();
  });

  it('records the last event and invokes onEvent for incoming SSE messages', () => {
    const onEvent = vi.fn();
    const { result } = renderHook(() => useRealtime({ onEvent }));

    act(() => {
      MockEventSource.latest().emitOpen();
      MockEventSource.latest().emitMessage({ type: 'tools', data: { count: 2 } }, 'evt-1');
    });

    const expected: RealtimeEvent = { type: 'tools', data: { count: 2 }, id: 'evt-1' };
    expect(result.current.lastEvent).toEqual(expected);
    expect(onEvent).toHaveBeenCalledWith(expected);
  });

  it('invalidates the TanStack Query cache for the event type', () => {
    const { result, queryClient } = renderHook(() => useRealtime());
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    act(() => {
      MockEventSource.latest().emitMessage({ type: 'instances', data: {} });
    });

    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['admin', 'instances'] });
    expect(result.current.lastEvent?.type).toBe('instances');
  });

  it('treats non-JSON SSE payloads as opaque "message" events', () => {
    const { result } = renderHook(() => useRealtime());

    act(() => {
      MockEventSource.latest().emitMessage('plain-text-ping');
    });

    expect(result.current.lastEvent).toMatchObject({ type: 'message', data: 'plain-text-ping' });
  });

  it('marks the connection as disconnected when the EventSource errors', () => {
    const { result } = renderHook(() => useRealtime());

    act(() => {
      MockEventSource.latest().emitOpen();
    });
    expect(result.current.isConnected).toBe(true);

    act(() => {
      MockEventSource.latest().emitError();
    });
    expect(result.current.isConnected).toBe(false);
  });

  it('reconnects with exponential backoff after repeated errors', async () => {
    vi.useFakeTimers();
    renderHook(() => useRealtime({ baseReconnectDelayMs: 1000, maxReconnectDelayMs: 30000 }));

    const delays = [1000, 2000, 4000, 8000, 16000];
    for (const delay of delays) {
      const before = MockEventSource.instances.length;
      act(() => {
        MockEventSource.latest().emitError();
      });

      await act(async () => {
        await vi.advanceTimersByTimeAsync(delay - 1);
      });
      expect(MockEventSource.instances.length).toBe(before); // not yet

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
      expect(MockEventSource.instances.length).toBe(before + 1); // reconnected
    }
  });

  it('caps the reconnect delay at maxReconnectDelayMs', async () => {
    vi.useFakeTimers();
    renderHook(() => useRealtime({ baseReconnectDelayMs: 1000, maxReconnectDelayMs: 3000 }));

    act(() => {
      MockEventSource.latest().emitError(); // attempt 0 -> 1000ms
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    act(() => {
      MockEventSource.latest().emitError(); // attempt 1 -> 2000ms
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    const beforeThirdReconnect = MockEventSource.instances.length;
    act(() => {
      MockEventSource.latest().emitError(); // attempt 2 -> would be 4000ms, capped to 3000ms
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2999);
    });
    expect(MockEventSource.instances.length).toBe(beforeThirdReconnect);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(MockEventSource.instances.length).toBe(beforeThirdReconnect + 1);
  });

  it('resets the backoff attempt counter after a successful reconnect', async () => {
    vi.useFakeTimers();
    renderHook(() => useRealtime({ baseReconnectDelayMs: 1000, maxReconnectDelayMs: 30000 }));

    act(() => {
      MockEventSource.latest().emitError(); // attempt 0 -> 1000ms
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    act(() => {
      MockEventSource.latest().emitOpen(); // success resets attempt counter to 0
    });

    const before = MockEventSource.instances.length;
    act(() => {
      MockEventSource.latest().emitError();
    });

    // Should use the base delay again (1000ms), not continue at 2000ms.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(999);
    });
    expect(MockEventSource.instances.length).toBe(before);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(MockEventSource.instances.length).toBe(before + 1);
  });

  it('switches to background polling while the tab is hidden', async () => {
    vi.useFakeTimers();
    renderHook(() => useRealtime({ backgroundPollIntervalMs: 30000 }));

    act(() => {
      MockEventSource.latest().emitOpen();
    });
    expect(MockEventSource.latest().closed).toBe(false);

    setVisibility('hidden');

    expect(MockEventSource.latest().closed).toBe(true);

    await act(async () => {
      await flushMicrotasks();
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(30000);
    });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('does not attempt SSE reconnects while the tab is hidden', async () => {
    vi.useFakeTimers();
    renderHook(() => useRealtime());

    setVisibility('hidden');
    const instancesWhileHidden = MockEventSource.instances.length;

    await act(async () => {
      await vi.advanceTimersByTimeAsync(60000);
    });

    expect(MockEventSource.instances.length).toBe(instancesWhileHidden);
  });

  it('reconnects via SSE and stops polling when the tab becomes visible again', async () => {
    vi.useFakeTimers();
    renderHook(() => useRealtime({ backgroundPollIntervalMs: 30000 }));

    setVisibility('hidden');
    await act(async () => {
      await flushMicrotasks();
    });
    const instancesWhileHidden = MockEventSource.instances.length;

    setVisibility('visible');

    expect(MockEventSource.instances.length).toBe(instancesWhileHidden + 1);

    fetchMock.mockClear();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60000);
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('falls back to HTTP polling when EventSource is unavailable', async () => {
    vi.stubGlobal('EventSource', undefined);
    fetchMock.mockImplementation(async () => jsonResponse({ events: [{ type: 'health', data: { ok: true } }] }));

    const { result } = renderHook(() => useRealtime({ pollIntervalMs: 5000 }));

    await act(async () => {
      await flushMicrotasks();
    });

    expect(MockEventSource.instances).toHaveLength(0);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(result.current.isConnected).toBe(true);
    expect(result.current.lastEvent).toEqual({ type: 'health', data: { ok: true } });
  });

  it('polls repeatedly on the configured interval when using the HTTP fallback', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('EventSource', undefined);

    renderHook(() => useRealtime({ pollIntervalMs: 5000 }));

    await act(async () => {
      await flushMicrotasks();
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(fetchMock).toHaveBeenCalledTimes(2);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it('surfaces an error and disconnects when the polling fallback fetch fails', async () => {
    vi.stubGlobal('EventSource', undefined);
    fetchMock.mockRejectedValue(new Error('network down'));

    const { result } = renderHook(() => useRealtime());

    await act(async () => {
      await flushMicrotasks();
    });

    expect(result.current.isConnected).toBe(false);
    expect(result.current.error).toBeInstanceOf(Error);
    expect(result.current.error?.message).toBe('network down');
  });

  it('surfaces a non-ok HTTP status from the polling fallback as an error', async () => {
    vi.stubGlobal('EventSource', undefined);
    fetchMock.mockResolvedValue(new Response('nope', { status: 503, statusText: 'Service Unavailable' }));

    const { result } = renderHook(() => useRealtime());

    await act(async () => {
      await flushMicrotasks();
    });

    expect(result.current.isConnected).toBe(false);
    expect(result.current.error?.message).toContain('503');
  });

  it('does not connect at all when disabled', async () => {
    const { result } = renderHook(() => useRealtime({ enabled: false }));

    await act(async () => {
      await flushMicrotasks();
    });

    expect(MockEventSource.instances).toHaveLength(0);
    expect(fetchMock).not.toHaveBeenCalled();
    expect(result.current.isConnected).toBe(false);
  });

  it('ignores manual reconnect() calls while disabled', () => {
    const { result } = renderHook(() => useRealtime({ enabled: false }));

    act(() => {
      result.current.reconnect();
    });

    expect(MockEventSource.instances).toHaveLength(0);
  });

  it('closes the EventSource, clears timers, and removes the visibility listener on unmount', async () => {
    vi.useFakeTimers();
    const removeSpy = vi.spyOn(document, 'removeEventListener');
    const { unmount } = renderHook(() => useRealtime());

    const instance = MockEventSource.latest();
    unmount();

    expect(instance.closed).toBe(true);
    expect(removeSpy).toHaveBeenCalledWith('visibilitychange', expect.any(Function));

    const instancesAfterUnmount = MockEventSource.instances.length;
    act(() => {
      instance.emitError();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60000);
    });

    // No further reconnect attempts or background polls after unmount.
    expect(MockEventSource.instances.length).toBe(instancesAfterUnmount);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('supports manual reconnect(), resetting the backoff counter and creating a fresh connection', async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useRealtime());

    act(() => {
      MockEventSource.latest().emitError();
    });

    const before = MockEventSource.instances.length;
    act(() => {
      result.current.reconnect();
    });

    // Manual reconnect connects immediately, ignoring the pending backoff timer.
    expect(MockEventSource.instances.length).toBe(before + 1);

    act(() => {
      MockEventSource.latest().emitError();
    });

    // The backoff counter was reset by reconnect(), so the next retry uses the base delay again.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(999);
    });
    expect(MockEventSource.instances.length).toBe(before + 1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(MockEventSource.instances.length).toBe(before + 2);
  });

  it('does not create duplicate connections when reconnect() is called multiple times rapidly', () => {
    const { result } = renderHook(() => useRealtime());
    const before = MockEventSource.instances.length;

    act(() => {
      result.current.reconnect();
      result.current.reconnect();
      result.current.reconnect();
    });

    // Each call tears down the previous instance before opening a new one,
    // so only one live (non-closed) connection should remain.
    const openInstances = MockEventSource.instances.filter((i) => !i.closed);
    expect(openInstances).toHaveLength(1);
    expect(MockEventSource.instances.length).toBeGreaterThan(before);
  });
});
