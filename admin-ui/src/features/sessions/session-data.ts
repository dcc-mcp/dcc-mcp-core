export type SessionStatus = 'active' | 'ended' | 'interrupted' | 'disconnected' | 'crashed'
  | 'gpu_crashed' | 'timed_out' | 'cancelled' | 'thread_affinity_failure' | 'unknown';

export type SessionRow = {
  session_id: string;
  parent_session_id?: string | null;
  dcc_type?: string | null;
  instance_id?: string | null;
  status: SessionStatus;
  started_at: string;
  ended_at?: string | null;
  duration_ms?: number | null;
  end_reason?: string | null;
  turn_count: number | null;
  tool_call_count: number;
  version?: string | null;
  agent_name?: string | null;
  actor_name?: string | null;
};

type SessionSummary = {
  active: number;
  ended: number;
  crashed: number;
  by_dcc: Record<string, number>;
};

// Wire fields are projected by gateway_admin_session_sqlite.rs and sessions.rs.
// Keep support for the ISO/kpi envelope used by older UI/dev fixtures.
export type SessionsApiPayload = {
  sessions: Array<Omit<SessionRow, 'started_at' | 'turn_count'> & {
    started_at?: string;
    turn_count?: number | null;
    started_at_ms?: number | null;
    ended_at_ms?: number | null;
    core_version?: string | null;
    end_reason_json?: string | null;
  }>;
  total: number;
  summary?: SessionSummary;
  kpi?: SessionSummary & { total: number };
};

function isoTimestamp(value: number | null | undefined): string | null {
  if (value == null || !Number.isFinite(value)) return null;
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toISOString() : null;
}

export function normalizeSessionsPayload(payload: SessionsApiPayload) {
  return {
    total: payload.total,
    kpi: payload.summary ?? payload.kpi ?? { active: 0, ended: 0, crashed: 0, by_dcc: {} },
    sessions: payload.sessions.map((session): SessionRow => ({
      ...session,
      started_at: session.started_at ?? isoTimestamp(session.started_at_ms) ?? '',
      ended_at: session.ended_at ?? isoTimestamp(session.ended_at_ms),
      version: session.version ?? session.core_version,
      end_reason: session.end_reason ?? session.end_reason_json,
      // The persistence API does not currently record turns. Do not invent zero.
      turn_count: session.turn_count ?? null,
    })),
  };
}
