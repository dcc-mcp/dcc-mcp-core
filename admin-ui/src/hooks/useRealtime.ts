/**
 * Realtime updates for the admin UI via Server-Sent Events, with an HTTP
 * polling fallback.
 *
 * Connects to `${API_BASE}/events` (an SSE endpoint) and:
 *   - reconnects with exponential backoff (1s -> 2s -> 4s -> 8s -> 30s max)
 *     when the connection drops,
 *   - throttles to a slow 30s poll while the tab is in the background
 *     (document.visibilitychange), closing the live SSE connection to
 *     avoid unnecessary server load,
 *   - falls back to HTTP polling entirely when `EventSource` is not
 *     available in the runtime (feature detection),
 *   - invalidates the TanStack Query caches related to the event's
 *     resource so panels refetch fresh data instead of waiting on their
 *     own poll interval.
 *
 * ## Usage
 *
 * ```tsx
 * const { isConnected, lastEvent, error, reconnect } = useRealtime();
 * ```
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { API_BASE } from '../platform';
import { adminKeys } from './queries';

// ── types ────────────────────────────────────────────────────────────────

/**
 * Resource names the admin API is known to emit events for. Kept in sync
 * with the top-level segments of {@link adminKeys} so that invalidation
 * can target the right queries. The `(string & {})` union member keeps
 * autocomplete for known values while still accepting server event types
 * this client doesn't know about yet.
 */
export type KnownRealtimeEventType =
  | 'health'
  | 'workers'
  | 'instances'
  | 'tools'
  | 'calls'
  | 'traces'
  | 'traffic'
  | 'tasks'
  | 'workflows'
  | 'activity'
  | 'governance'
  | 'logs'
  | 'skills'
  | 'stats'
  | 'analytics'
  | 'marketplace'
  | 'integrations'
  | 'memory'
  | 'message';

export type RealtimeEventType = KnownRealtimeEventType | (string & {});

/** Normalized shape of an event, whether it arrived via SSE or polling. */
export interface RealtimeEvent<T = unknown> {
  type: RealtimeEventType;
  data: T;
  timestamp?: string;
  id?: string;
}

export interface UseRealtimeOptions {
  /** Path appended to `API_BASE` for both the SSE stream and the polling fallback. Default `/events`. */
  path?: string;
  /** Master on/off switch. When false, no connection is made and any active one is torn down. Default true. */
  enabled?: boolean;
  /** Initial reconnect delay in ms. Default 1000. */
  baseReconnectDelayMs?: number;
  /** Ceiling for the exponential backoff delay in ms. Default 30000. */
  maxReconnectDelayMs?: number;
  /** Poll interval used when SSE is unavailable and the tab is visible. Default 5000. */
  pollIntervalMs?: number;
  /** Poll interval used while the tab is hidden (both as SSE substitute and as the fallback's own throttle). Default 30000. */
  backgroundPollIntervalMs?: number;
  /** Called for every normalized event, in addition to the built-in cache invalidation. */
  onEvent?: (event: RealtimeEvent) => void;
}

export interface UseRealtimeResult {
  isConnected: boolean;
  lastEvent: RealtimeEvent | null;
  error: Error | null;
  /** Force an immediate (re)connect attempt, resetting the backoff counter. No-op when `enabled` is false. */
  reconnect: () => void;
}

const DEFAULT_PATH = '/events';
const DEFAULT_BASE_RECONNECT_DELAY_MS = 1_000;
const DEFAULT_MAX_RECONNECT_DELAY_MS = 30_000;
const DEFAULT_POLL_INTERVAL_MS = 5_000;
const DEFAULT_BACKGROUND_POLL_INTERVAL_MS = 30_000;

// ── pure helpers (exported for unit testing) ────────────────────────────

/** Exponential backoff: base * 2^attempt, capped at maxMs. */
export function computeBackoffDelay(attempt: number, baseMs: number, maxMs: number): number {
  const delay = baseMs * 2 ** Math.max(0, attempt);
  return Math.min(delay, maxMs);
}

function recordOrNull(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

/** Coerce a raw payload (parsed JSON or an opaque string) into a {@link RealtimeEvent}. */
export function normalizeEvent(raw: unknown, fallbackId?: string): RealtimeEvent {
  const record = recordOrNull(raw);
  if (!record) {
    return { type: 'message', data: raw, id: fallbackId };
  }
  const type = typeof record.type === 'string' && record.type ? record.type : 'message';
  const timestamp = typeof record.timestamp === 'string' ? record.timestamp : undefined;
  const id = typeof record.id === 'string' ? record.id : fallbackId;
  const data = 'data' in record ? record.data : record;
  return { type, data, timestamp, id };
}

/** Parse a raw SSE `message` payload into a {@link RealtimeEvent}, tolerating non-JSON payloads. */
export function parseSSEMessageData(rawData: string, lastEventId?: string): RealtimeEvent {
  try {
    return normalizeEvent(JSON.parse(rawData), lastEventId || undefined);
  } catch {
    return { type: 'message', data: rawData, id: lastEventId || undefined };
  }
}

// ── hook ─────────────────────────────────────────────────────────────────

export function useRealtime(options: UseRealtimeOptions = {}): UseRealtimeResult {
  const {
    path = DEFAULT_PATH,
    enabled = true,
    baseReconnectDelayMs = DEFAULT_BASE_RECONNECT_DELAY_MS,
    maxReconnectDelayMs = DEFAULT_MAX_RECONNECT_DELAY_MS,
    pollIntervalMs = DEFAULT_POLL_INTERVAL_MS,
    backgroundPollIntervalMs = DEFAULT_BACKGROUND_POLL_INTERVAL_MS,
    onEvent,
  } = options;

  const queryClient = useQueryClient();

  const [isConnected, setIsConnected] = useState(false);
  const [lastEvent, setLastEvent] = useState<RealtimeEvent | null>(null);
  const [error, setError] = useState<Error | null>(null);

  // Read via ref inside the effect so callback identity changes don't
  // restart the connection lifecycle.
  const onEventRef = useRef(onEvent);
  onEventRef.current = onEvent;

  // Populated by the effect below; `reconnect()` just delegates to it so
  // the public callback identity stays stable across renders.
  const reconnectFnRef = useRef<() => void>(() => {});

  const invalidateForEvent = useCallback((type: RealtimeEventType) => {
    queryClient.invalidateQueries({ queryKey: [...adminKeys.all, type] });
  }, [queryClient]);

  useEffect(() => {
    if (!enabled) {
      setIsConnected(false);
      reconnectFnRef.current = () => {};
      return;
    }

    let stopped = false;
    let es: EventSource | null = null;
    let reconnectTimer: number | null = null;
    let pollTimer: number | null = null;
    let reconnectAttempt = 0;
    const sseSupported = typeof EventSource !== 'undefined';

    const clearReconnectTimer = () => {
      if (reconnectTimer != null) {
        window.clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
    };

    const stopPolling = () => {
      if (pollTimer != null) {
        window.clearInterval(pollTimer);
        pollTimer = null;
      }
    };

    const closeSSE = () => {
      if (es) {
        es.onopen = null;
        es.onmessage = null;
        es.onerror = null;
        es.close();
        es = null;
      }
    };

    const emit = (event: RealtimeEvent) => {
      if (stopped) return;
      setLastEvent(event);
      onEventRef.current?.(event);
      invalidateForEvent(event.type);
    };

    const pollTick = async () => {
      try {
        const res = await fetch(`${API_BASE}${path}`);
        if (!res.ok) {
          throw new Error(`${res.status} ${res.statusText}`);
        }
        const payload = (await res.json()) as unknown;
        if (stopped) return;
        setIsConnected(true);
        setError(null);
        const eventsField = recordOrNull(payload)?.events;
        const rawEvents = Array.isArray(payload)
          ? payload
          : Array.isArray(eventsField)
            ? eventsField
            : [payload];
        for (const raw of rawEvents) {
          emit(normalizeEvent(raw));
        }
      } catch (err) {
        if (stopped) return;
        setIsConnected(false);
        setError(err instanceof Error ? err : new Error(String(err)));
      }
    };

    const startPolling = (intervalMs: number) => {
      stopPolling();
      void pollTick();
      pollTimer = window.setInterval(() => void pollTick(), intervalMs);
    };

    const connectSSE = () => {
      closeSSE();
      clearReconnectTimer();
      try {
        const instance = new EventSource(`${API_BASE}${path}`);
        es = instance;
        instance.onopen = () => {
          reconnectAttempt = 0;
          if (stopped) return;
          setIsConnected(true);
          setError(null);
        };
        instance.onmessage = (ev) => {
          emit(parseSSEMessageData(ev.data, ev.lastEventId));
        };
        instance.onerror = () => {
          if (stopped) return;
          setIsConnected(false);
          closeSSE();
          scheduleReconnect();
        };
      } catch (err) {
        if (stopped) return;
        setIsConnected(false);
        setError(err instanceof Error ? err : new Error(String(err)));
        scheduleReconnect();
      }
    };

    function scheduleReconnect() {
      if (stopped) return;
      if (document.visibilityState === 'hidden') {
        startPolling(backgroundPollIntervalMs);
        return;
      }
      const delay = computeBackoffDelay(reconnectAttempt, baseReconnectDelayMs, maxReconnectDelayMs);
      reconnectAttempt += 1;
      reconnectTimer = window.setTimeout(connectSSE, delay);
    }

    const connectOrPoll = () => {
      if (document.visibilityState === 'hidden') {
        startPolling(backgroundPollIntervalMs);
      } else if (sseSupported) {
        connectSSE();
      } else {
        startPolling(pollIntervalMs);
      }
    };

    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden') {
        clearReconnectTimer();
        closeSSE();
        startPolling(backgroundPollIntervalMs);
      } else {
        stopPolling();
        reconnectAttempt = 0;
        if (sseSupported) {
          connectSSE();
        } else {
          startPolling(pollIntervalMs);
        }
      }
    };

    reconnectFnRef.current = () => {
      reconnectAttempt = 0;
      clearReconnectTimer();
      stopPolling();
      closeSSE();
      setError(null);
      connectOrPoll();
    };

    document.addEventListener('visibilitychange', handleVisibilityChange);
    connectOrPoll();

    return () => {
      stopped = true;
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      closeSSE();
      clearReconnectTimer();
      stopPolling();
      reconnectFnRef.current = () => {};
    };
  }, [enabled, path, baseReconnectDelayMs, maxReconnectDelayMs, pollIntervalMs, backgroundPollIntervalMs, invalidateForEvent]);

  const reconnect = useCallback(() => {
    reconnectFnRef.current();
  }, []);

  return { isConnected, lastEvent, error, reconnect };
}
