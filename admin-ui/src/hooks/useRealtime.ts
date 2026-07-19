/**
 * SSE-based real-time data hook for admin UI.
 *
 * Connects to /admin/api/events SSE stream for live updates.
 * Falls back to polling when SSE is unavailable or the tab is in background.
 *
 * ## Backend contract
 *
 * Endpoint: `GET /admin/api/events`
 * Response: `text/event-stream`
 * Events:
 *   event: <eventType>  (e.g. "health", "sessions", "workers")
 *   data: <JSON payload>
 *
 * ## Usage
 *
 * ```tsx
 * const status = useRealtimeEvents(activePanel === 'health', ['health', 'workers']);
 * useRealtimeEvent('health', (data) => console.log(data), activePanel === 'health');
 * ```
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { API_BASE } from '../platform';

// ── types ───────────────────────────────────────────────────────────────────

export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected' | 'error';

/** Shape of an SSE event received from /admin/api/events */
export interface RealtimeEvent<T = unknown> {
  type: string;
  data: T;
  queryKeys?: readonly (string | number | { [k: string]: unknown })[];
}

/** Configuration for the SSE connection. */
export interface RealtimeConfig {
  /** EventSource URL. Defaults to `${API_BASE}/events`. */
  url?: string;
  /** Max reconnect interval in ms. Default 30_000. */
  maxReconnectMs?: number;
  /** Polling interval in ms when SSE is unavailable or tab is in background. Default 30_000. */
  pollIntervalMs?: number;
  /** The base reconnect delay in ms for exponential backoff (step 1). Default 1_000. */
  baseReconnectMs?: number;
}

// ── defaults ────────────────────────────────────────────────────────────────

const DEFAULT_CONFIG: Required<RealtimeConfig> = {
  url: `${API_BASE}/events`,
  maxReconnectMs: 30_000,
  pollIntervalMs: 30_000,
  baseReconnectMs: 1_000,
};

// ── helpers ─────────────────────────────────────────────────────────────────

/** Compute the reconnect delay using exponential backoff: 1s, 2s, 4s, 8s, capped at max. */
function reconnectDelay(attempt: number, base: number, max: number): number {
  return Math.min(base * 2 ** Math.min(attempt, 4), max);
}

/** True when the document is hidden (tab in background). */
function isTabHidden(): boolean {
  return typeof document !== 'undefined' && document.hidden;
}

// ── hook: useRealtimeEvents ─────────────────────────────────────────────────

/**
 * Manage an SSE connection to `/admin/api/events` with:
 * - Exponential backoff reconnect (1s → 2s → 4s → 8s → max 30s)
 * - Background-tab throttling (disconnects SSE, falls back to polling)
 * - Graceful fallback to polling when SSE is unavailable
 * - TanStack Query cache updates via queryClient.setQueryData
 *
 * @param enabled - Whether the connection should be active.
 * @param eventTypes - Optional list of event types to process. When empty, all events are processed.
 * @param config - Optional overrides for URL, intervals, etc.
 * @returns The current connection status.
 */
export function useRealtimeEvents(
  enabled: boolean,
  eventTypes: string[] = [],
  config: RealtimeConfig = {},
): ConnectionStatus {
  const { url, maxReconnectMs, pollIntervalMs, baseReconnectMs } = {
    ...DEFAULT_CONFIG,
    ...config,
  };

  const queryClient = useQueryClient();
  const [status, setStatus] = useState<ConnectionStatus>('disconnected');
  const esRef = useRef<EventSource | null>(null);
  const reconnectAttemptRef = useRef(0);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;

  // Refs for the config values that change — so stale closures are never a problem.
  const eventTypesRef = useRef(eventTypes);
  eventTypesRef.current = eventTypes;
  const urlRef = useRef(url);
  urlRef.current = url;
  const maxReconnectMsRef = useRef(maxReconnectMs);
  maxReconnectMsRef.current = maxReconnectMs;
  const pollIntervalMsRef = useRef(pollIntervalMs);
  pollIntervalMsRef.current = pollIntervalMs;
  const baseReconnectMsRef = useRef(baseReconnectMs);
  baseReconnectMsRef.current = baseReconnectMs;

  const clearTimers = useCallback(() => {
    if (reconnectTimerRef.current !== null) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
    if (pollTimerRef.current !== null) {
      clearInterval(pollTimerRef.current);
      pollTimerRef.current = null;
    }
  }, []);

  const closeEventSource = useCallback(() => {
    if (esRef.current) {
      esRef.current.close();
      esRef.current = null;
    }
  }, []);

  const startPolling = useCallback(() => {
    clearTimers();
    pollTimerRef.current = setInterval(() => {
      if (enabledRef.current) {
        queryClient.invalidateQueries({ queryKey: ['admin'] });
      }
    }, pollIntervalMsRef.current);
  }, [clearTimers, queryClient]);

  const stopPolling = useCallback(() => {
    if (pollTimerRef.current !== null) {
      clearInterval(pollTimerRef.current);
      pollTimerRef.current = null;
    }
  }, []);

  const connect = useCallback(() => {
    if (!enabledRef.current) return;
    if (esRef.current) return;

    if (isTabHidden()) {
      setStatus('disconnected');
      startPolling();
      return;
    }

    setStatus('connecting');

    try {
      const es = new EventSource(urlRef.current);

      es.onopen = () => {
        setStatus('connected');
        reconnectAttemptRef.current = 0;
      };

      es.onmessage = (event: MessageEvent) => {
        const type = (event as MessageEvent & { type?: string }).type || 'message';
        const types = eventTypesRef.current;
        if (types.length > 0 && !types.includes(type)) return;

        try {
          const payload = JSON.parse(event.data);
          const rt = payload as RealtimeEvent;
          if (Array.isArray(rt.queryKeys) && rt.queryKeys.length > 0) {
            for (const key of rt.queryKeys) {
              queryClient.setQueryData(key, rt.data);
            }
          } else {
            queryClient.setQueryData(['admin', type], rt.data ?? payload);
          }
        } catch {
          // Non-JSON event — skip.
        }
      };

      if (eventTypesRef.current.length > 0) {
        for (const type of eventTypesRef.current) {
          es.addEventListener(type, (event: Event) => {
            const me = event as MessageEvent;
            const types = eventTypesRef.current;
            if (types.length > 0 && !types.includes(me.type || 'message')) return;
            try {
              const payload = JSON.parse(me.data);
              const rt = payload as RealtimeEvent;
              if (Array.isArray(rt.queryKeys) && rt.queryKeys.length > 0) {
                for (const key of rt.queryKeys) {
                  queryClient.setQueryData(key, rt.data);
                }
              } else {
                queryClient.setQueryData(['admin', me.type], rt.data ?? payload);
              }
            } catch {
              // Non-JSON event — skip.
            }
          });
        }
      }

      es.onerror = () => {
        // Disconnect and schedule reconnect with backoff.
        if (esRef.current) {
          esRef.current.close();
          esRef.current = null;
        }
        setStatus('error');

        const attempt = reconnectAttemptRef.current;
        const delay = reconnectDelay(attempt, baseReconnectMsRef.current, maxReconnectMsRef.current);

        reconnectTimerRef.current = setTimeout(() => {
          if (enabledRef.current) {
            reconnectAttemptRef.current = attempt + 1;
            connect();
          }
        }, delay);
      };

      esRef.current = es;
    } catch {
      setStatus('error');
      startPolling();
    }
  }, [queryClient, startPolling]);

  // ── main effect: connect / disconnect ─────────────────────────────────────

  useEffect(() => {
    if (!enabled) {
      closeEventSource();
      clearTimers();
      setStatus('disconnected');
      reconnectAttemptRef.current = 0;
      return;
    }

    connect();

    return () => {
      closeEventSource();
      clearTimers();
    };
  }, [enabled, connect, closeEventSource, clearTimers]);

  // ── visibility change: throttle in background ─────────────────────────────

  useEffect(() => {
    if (!enabled) return;

    const handleVisibilityChange = () => {
      if (!enabledRef.current) return;

      if (document.hidden) {
        closeEventSource();
        clearTimers();
        setStatus('disconnected');
        startPolling();
      } else {
        stopPolling();
        reconnectAttemptRef.current = 0;
        connect();
      }
    };

    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [enabled, connect, closeEventSource, clearTimers, startPolling, stopPolling]);

  return status;
}

// ── hook: useRealtimeEvent ──────────────────────────────────────────────────

/**
 * Subscribe to a single SSE event type.
 *
 * When `enabled` is true, listens for events of `eventType` on the shared SSE
 * connection and calls `handler` with the parsed payload.
 *
 * This hook does NOT manage the connection itself — use `useRealtimeEvents`
 * alongside it to control the lifecycle.
 *
 * @param eventType - The SSE event name to listen for.
 * @param handler - Callback invoked with parsed event data.
 * @param enabled - Whether to subscribe.
 */
export function useRealtimeEvent<T = unknown>(
  eventType: string,
  handler: (data: T) => void,
  enabled: boolean,
): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    if (!enabled) return;

    const url = `${API_BASE}/events`;
    let es: EventSource | null = null;
    let closed = false;

    try {
      es = new EventSource(url);

      es.addEventListener(eventType, (event: Event) => {
        if (closed) return;
        try {
          const data = JSON.parse((event as MessageEvent).data) as T;
          handlerRef.current(data);
        } catch {
          // Non-JSON event — skip.
        }
      });

      es.onerror = () => {
        // Silently ignore — the main connection hook handles reconnection.
      };
    } catch {
      // EventSource not supported.
    }

    return () => {
      closed = true;
      if (es) es.close();
    };
  }, [eventType, enabled]);
}
