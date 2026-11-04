import { describe, expect, it } from 'vitest';
import { normalizeSessionsPayload, type SessionsApiPayload } from './session-data';

const summary = { active: 0, ended: 0, crashed: 1, by_dcc: { maya: 1 } };
const wire: SessionsApiPayload = {
  total: 1, summary,
  sessions: [{ session_id: 'session-one', status: 'crashed', dcc_type: 'maya', instance_id: 'maya-one',
    started_at_ms: 1779091200000, ended_at_ms: 1779091201000, tool_call_count: 3,
    core_version: '0.20.45', end_reason_json: '{"host_crash":{"detail":"render stopped"}}' }],
};

describe('session API normalization', () => {
  it('reads the real summary and SQLite millisecond/version/end-reason fields', () => {
    const result = normalizeSessionsPayload(wire);
    expect(result.kpi).toEqual(summary);
    expect(result.sessions[0]).toMatchObject({ started_at: '2026-05-18T08:00:00.000Z',
      ended_at: '2026-05-18T08:00:01.000Z', version: '0.20.45', turn_count: null,
      end_reason: '{"host_crash":{"detail":"render stopped"}}', tool_call_count: 3 });
  });

  it('preserves the existing ISO/kpi envelope and explicit turn counts', () => {
    const result = normalizeSessionsPayload({ total: 1, kpi: { ...summary, ended: 1, crashed: 0, total: 1 },
      sessions: [{ session_id: 'session-one', status: 'ended', started_at: '2026-05-18T08:00:00Z',
        ended_at: '2026-05-18T08:00:01Z', duration_ms: 1000, turn_count: 2, tool_call_count: 3,
        version: '0.20.45', end_reason: 'completed' }] });
    expect(result.kpi.ended).toBe(1);
    expect(result.sessions[0]).toMatchObject({ started_at: '2026-05-18T08:00:00Z',
      ended_at: '2026-05-18T08:00:01Z', duration_ms: 1000, turn_count: 2, end_reason: 'completed' });
  });

  it('handles empty real responses and unavailable timestamps without throwing', () => {
    expect(normalizeSessionsPayload({ total: 0, sessions: [], summary: { ...summary, crashed: 0, by_dcc: {} } }).sessions).toEqual([]);
    const result = normalizeSessionsPayload({ ...wire, sessions: [{ ...wire.sessions[0], started_at_ms: Number.NaN, ended_at_ms: null }] });
    expect(result.sessions[0]).toMatchObject({ started_at: '', ended_at: null, turn_count: null });
  });
});
