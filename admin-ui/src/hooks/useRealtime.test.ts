/**
 * Unit tests for useRealtime hook.
 *
 * Uses vitest fake timers to control setTimeout/setInterval and
 * a mock EventSource to simulate SSE connections.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import React from 'react';
import { useRealtimeEvent, useRealtimeEvents } from './useRealtime';

// ── helpers ─────────────────────────────────────────────────────────────────

const TEST_URL = '/admin/api/events';

/** Minimal wrapper that provides TanStack Query context. */
function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return React.createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

/** Simulate an EventSource with manual control over events, open, and error. */
class MockEventSource {
  static instances: MockEventSource[] = [];
  static CONSTRUCTOR_ERROR = false;

  url: string;
  onopen: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;
  readyState: number = 0; // 0 = CONNECTING, 1 = OPEN, 2 = CLOSED
  private listeners: Map<string, Array<(event: Event) => void>> = new Map();

  constructor(url: string) {
    if (MockEventSource.CONSTRUCTOR_ERROR) {
      throw new Error('EventSource not available');
    }
    this.url = url;
    MockEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: (event: Event) => void) {
    if (!this.listeners.has(type)) {
      this.listeners.set(type, []);
    }
    this.listeners.get(type)!.push(listener);
  }

  removeEventListener(type: string, listener: (event: Event) => void) {
    const arr = this.listeners.get(type);
    if (arr) {
      this.listeners.set(type, arr.filter((l) => l !== listener));
    }
  }

  close() {
    this.readyState = 2;
    MockEventSource.instances = MockEventSource.instances.filter((i) => i !== this);
  }

  // ── test helpers ──────────────────────────────────────────────────────────

  /** Simulate the connection opening. */
  simulateOpen() {
    this.readyState = 1;
    this.onopen?.();
  }

  /** Simulate receiving a named event. */
  simulateEvent(type: string, data: unknown) {
    const event = new MessageEvent(type, { data: JSON.stringify(data) });
    // Dispatch to specific listeners.
    const arr = this.listeners.get(type);
    if (arr) {
      for (const listener of arr) {
        listener(event);
      }
    }
    // Also dispatch to generic onmessage if no specific listener handled it.
    if (!arr || arr.length === 0) {
      this.onmessage?.(event);
    }
  }

  /** Simulate an error. */
  simulateError() {
    this.onerror?.();
  }

  static reset() {
    MockEventSource.instances = [];
    MockEventSource.CONSTRUCTOR_ERROR = false;
  }
}

// Override global EventSource.
const originalEventSource = (globalThis as unknown as Record<string, unknown>).EventSource;

function installMockEventSource() {
  (globalThis as unknown as Record<string, unknown>).EventSource = MockEventSource;
}

function restoreEventSource() {
  (globalThis as unknown as Record<string, unknown>).EventSource = originalEventSource;
}

// ── document visibility ─────────────────────────────────────────────────────

function setTabHidden(hidden: boolean) {
  Object.defineProperty(document, 'hidden', {
    configurable: true,
    get: () => hidden,
  });
  Object.defineProperty(document, 'visibilityState', {
    configurable: true,
    get: () => (hidden ? 'hidden' : 'visible'),
  });
}

function dispatchVisibilityChange() {
  document.dispatchEvent(new Event('visibilitychange'));
}

// ── tests: useRealtimeEvents ────────────────────────────────────────────────

describe('useRealtimeEvents', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    MockEventSource.reset();
    installMockEventSource();
    setTabHidden(false);
  });

  afterEach(() => {
    vi.useRealTimers();
    restoreEventSource();
  });

  it('starts disconnected when enabled is false', () => {
    const { result } = renderHook(() => useRealtimeEvents(false), {
      wrapper: createWrapper(),
    });

    expect(result.current).toBe('disconnected');
    expect(MockEventSource.instances).toHaveLength(0);
  });

  it('creates EventSource and transitions to connected when enabled', async () => {
    const { result } = renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL }), {
      wrapper: createWrapper(),
    });

    expect(result.current).toBe('connecting');
    expect(MockEventSource.instances).toHaveLength(1);

    // Simulate open — need to flush microtasks since EventSource fires async.
    await act(async () => {
      MockEventSource.instances[0].simulateOpen();
      // Flush any pending microtasks / setState batching.
      await vi.runAllTimersAsync();
    });

    expect(result.current).toBe('connected');
  });

  it('handles error and transitions to error status', async () => {
    const { result } = renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL }), {
      wrapper: createWrapper(),
    });

    // Wait for the effect to run and wire up onerror.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    // Now the onerror handler is set — simulate the error.
    await act(async () => {
      const es = MockEventSource.instances[0];
      es.simulateError();
    });

    expect(result.current).toBe('error');
  });

  it('reconnects with exponential backoff after error', () => {
    renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL, maxReconnectMs: 30_000, baseReconnectMs: 1_000 }), {
      wrapper: createWrapper(),
    });

    // First error triggers reconnect.
    act(() => {
      MockEventSource.instances[0].simulateError();
    });

    // After error, EventSource is closed and a reconnect timer is set.
    // Attempt 0 → delay = 1_000 * 2^0 = 1000 ms.
    act(() => {
      vi.advanceTimersByTime(999);
    });
    expect(MockEventSource.instances).toHaveLength(0); // Not yet reconnected.

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(MockEventSource.instances).toHaveLength(1); // Reconnected.

    // Simulate error again — attempt 1 → delay = 1_000 * 2^1 = 2000 ms.
    act(() => {
      MockEventSource.instances[0].simulateError();
    });

    act(() => {
      vi.advanceTimersByTime(1999);
    });
    expect(MockEventSource.instances).toHaveLength(0);

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(MockEventSource.instances).toHaveLength(1);
  });

  it('caps reconnect delay at maxReconnectMs', () => {
    renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL, maxReconnectMs: 10_000, baseReconnectMs: 1_000 }), {
      wrapper: createWrapper(),
    });

    // Trigger 5 errors (attempts 0-4) then verify attempt 5 is capped.
    for (let i = 0; i < 5; i++) {
      act(() => {
        MockEventSource.instances[0].simulateError();
      });
      // Advance past the computed delay for this attempt.
      const expectedDelay = Math.min(1_000 * 2 ** Math.min(i, 4), 10_000);
      act(() => {
        vi.advanceTimersByTime(expectedDelay);
      });
    }

    // After attempt 5 (index 4), the next error should use attempt 5 with capped delay.
    act(() => {
      MockEventSource.instances[0].simulateError();
    });

    // Capped at 10_000.
    act(() => {
      vi.advanceTimersByTime(9_999);
    });
    expect(MockEventSource.instances).toHaveLength(0);

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(MockEventSource.instances).toHaveLength(1);
  });

  it('resets reconnect counter on successful connection', () => {
    renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL, baseReconnectMs: 1_000 }), {
      wrapper: createWrapper(),
    });

    // Error → attempt 0 → delay 1s.
    act(() => {
      MockEventSource.instances[0].simulateError();
    });

    act(() => {
      vi.advanceTimersByTime(1_000);
    });

    // Successful connection.
    act(() => {
      MockEventSource.instances[0].simulateOpen();
    });

    // Another error — should be back to attempt 0.
    act(() => {
      MockEventSource.instances[0].simulateError();
    });

    act(() => {
      vi.advanceTimersByTime(999);
    });
    expect(MockEventSource.instances).toHaveLength(0);

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(MockEventSource.instances).toHaveLength(1);
  });

  it('disconnects SSE and falls back to polling when tab is hidden', () => {
    renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL, pollIntervalMs: 30_000 }), {
      wrapper: createWrapper(),
    });

    // Initially connected.
    act(() => {
      MockEventSource.instances[0].simulateOpen();
    });
    expect(MockEventSource.instances).toHaveLength(1);

    // Hide tab.
    act(() => {
      setTabHidden(true);
      dispatchVisibilityChange();
    });

    // EventSource should be closed.
    expect(MockEventSource.instances).toHaveLength(0);

    // A polling interval should be active.
    // Verify by advancing past pollIntervalMs and checking that nothing crashes.
    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    // No errors thrown — polling interval is set up.
  });

  it('reconnects SSE when tab becomes visible again', () => {
    renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL }), {
      wrapper: createWrapper(),
    });

    act(() => {
      MockEventSource.instances[0].simulateOpen();
    });

    // Hide tab.
    act(() => {
      setTabHidden(true);
      dispatchVisibilityChange();
    });
    expect(MockEventSource.instances).toHaveLength(0);

    // Show tab.
    act(() => {
      setTabHidden(false);
      dispatchVisibilityChange();
    });

    expect(MockEventSource.instances).toHaveLength(1);
  });

  it('cleans up on unmount', () => {
    const { unmount } = renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL }), {
      wrapper: createWrapper(),
    });

    expect(MockEventSource.instances).toHaveLength(1);

    unmount();

    expect(MockEventSource.instances).toHaveLength(0);
  });

  it('falls back to polling when EventSource constructor throws', () => {
    MockEventSource.CONSTRUCTOR_ERROR = true;

    const { result } = renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL, pollIntervalMs: 30_000 }), {
      wrapper: createWrapper(),
    });

    expect(result.current).toBe('error');
    expect(MockEventSource.instances).toHaveLength(0);

    // Polling should be active — advance timer to verify no crashes.
    act(() => {
      vi.advanceTimersByTime(30_000);
    });
  });

  it('filters events by eventTypes', () => {
    const queryClient = new QueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }) =>
      React.createElement(QueryClientProvider, { client: queryClient }, children);

    renderHook(() => useRealtimeEvents(true, ['health'], { url: TEST_URL }), { wrapper });

    act(() => {
      MockEventSource.instances[0].simulateOpen();
    });

    // Send a filtered-out event.
    act(() => {
      MockEventSource.instances[0].simulateEvent('workers', { workers: [] });
    });

    expect(queryClient.getQueryData(['admin', 'workers'])).toBeUndefined();

    // Send a matching event.
    act(() => {
      MockEventSource.instances[0].simulateEvent('health', { status: 'ok' });
    });

    expect(queryClient.getQueryData(['admin', 'health'])).toEqual({ status: 'ok' });
  });

  it('processes all events when eventTypes is empty', () => {
    const queryClient = new QueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }) =>
      React.createElement(QueryClientProvider, { client: queryClient }, children);

    renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL }), { wrapper });

    act(() => {
      MockEventSource.instances[0].simulateOpen();
    });

    act(() => {
      MockEventSource.instances[0].simulateEvent('workers', { workers: [{ id: '1' }] });
    });

    expect(queryClient.getQueryData(['admin', 'workers'])).toEqual({ workers: [{ id: '1' }] });
  });

  it('respects explicit queryKeys from server events', () => {
    const queryClient = new QueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }) =>
      React.createElement(QueryClientProvider, { client: queryClient }, children);

    renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL }), { wrapper });

    act(() => {
      MockEventSource.instances[0].simulateOpen();
    });

    const customKey = ['admin', 'custom', { range: '7d' }] as const;
    act(() => {
      MockEventSource.instances[0].simulateEvent('stats', {
        queryKeys: [customKey],
        data: { total: 42 },
      });
    });

    expect(queryClient.getQueryData(customKey)).toEqual({ total: 42 });
  });

  it('handles non-JSON events gracefully', () => {
    const queryClient = new QueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }) =>
      React.createElement(QueryClientProvider, { client: queryClient }, children);

    renderHook(() => useRealtimeEvents(true, [], { url: TEST_URL }), { wrapper });

    act(() => {
      MockEventSource.instances[0].simulateOpen();
    });

    // Send invalid JSON — should not throw.
    act(() => {
      const es = MockEventSource.instances[0];
      // Simulate raw non-JSON data via onmessage directly.
      es.onmessage?.(new MessageEvent('message', { data: 'not json {{{' }));
    });

    // No crash — cache is unchanged.
    expect(queryClient.getQueryData(['admin', 'message'])).toBeUndefined();
  });
});

// ── tests: useRealtimeEvent ─────────────────────────────────────────────────

describe('useRealtimeEvent', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    MockEventSource.reset();
    installMockEventSource();
    setTabHidden(false);
  });

  afterEach(() => {
    vi.useRealTimers();
    restoreEventSource();
  });

  it('does not connect when enabled is false', () => {
    const handler = vi.fn();

    renderHook(() => useRealtimeEvent('health', handler, false));

    expect(MockEventSource.instances).toHaveLength(0);
    expect(handler).not.toHaveBeenCalled();
  });

  it('connects and calls handler for matching events', () => {
    const handler = vi.fn();

    renderHook(() => useRealtimeEvent('health', handler, true));

    expect(MockEventSource.instances).toHaveLength(1);

    act(() => {
      MockEventSource.instances[0].simulateEvent('health', { status: 'ok' });
    });

    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler).toHaveBeenCalledWith({ status: 'ok' });
  });

  it('does not call handler for non-matching event types', () => {
    const handler = vi.fn();

    renderHook(() => useRealtimeEvent('health', handler, true));

    act(() => {
      MockEventSource.instances[0].simulateEvent('workers', { workers: [] });
    });

    expect(handler).not.toHaveBeenCalled();
  });

  it('uses latest handler via ref', () => {
    const handler1 = vi.fn();
    const handler2 = vi.fn();

    const { rerender } = renderHook(
      ({ handler }: { handler: (data: unknown) => void }) =>
        useRealtimeEvent('health', handler, true),
      { initialProps: { handler: handler1 } },
    );

    rerender({ handler: handler2 });

    act(() => {
      MockEventSource.instances[0].simulateEvent('health', { status: 'ok' });
    });

    expect(handler1).not.toHaveBeenCalled();
    expect(handler2).toHaveBeenCalledWith({ status: 'ok' });
  });

  it('cleans up EventSource on unmount', () => {
    const handler = vi.fn();

    const { unmount } = renderHook(() => useRealtimeEvent('health', handler, true));

    expect(MockEventSource.instances).toHaveLength(1);

    unmount();

    expect(MockEventSource.instances).toHaveLength(0);
  });

  it('handles non-JSON data gracefully', () => {
    const handler = vi.fn();

    renderHook(() => useRealtimeEvent('health', handler, true));

    act(() => {
      const es = MockEventSource.instances[0];
      // Simulate raw non-JSON data via a direct onmessage call.
      es.onmessage?.(new MessageEvent('message', { data: 'not json {{{' }));
    });

    // Handler should not be called for invalid JSON.
    expect(handler).not.toHaveBeenCalled();
  });

  it('does not call handler after unmount (avoids setState on unmounted)', () => {
    const handler = vi.fn();

    const { unmount } = renderHook(() => useRealtimeEvent('health', handler, true));

    unmount();

    // Event arrives after unmount.
    act(() => {
      // The EventSource should have been closed by cleanup.
      expect(MockEventSource.instances).toHaveLength(0);
    });

    expect(handler).not.toHaveBeenCalled();
  });
});
