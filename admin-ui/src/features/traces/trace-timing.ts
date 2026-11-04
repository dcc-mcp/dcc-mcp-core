import type { TraceSpan } from '../../admin-types';

/** Offset from the earliest recorded span, never an inferred request/model start. */
export function recordedSpanTiming(spans: TraceSpan[]) {
  const starts = spans.map((span) => span.started_ns).filter((value) => Number.isFinite(value) && value >= 0);
  const origin = starts.length ? Math.min(...starts) : null;
  const rows = spans.map((span, index) => ({
    span,
    index,
    offsetMs: origin != null && Number.isFinite(span.started_ns) && span.started_ns >= 0
      ? (span.started_ns - origin) / 1_000_000 : null,
    durationMs: Number.isFinite(span.duration_ns) && span.duration_ns >= 0
      ? span.duration_ns / 1_000_000 : null,
  }));
  const extentMs = Math.max(0, ...rows.map((row) => row.offsetMs != null && row.durationMs != null
    ? row.offsetMs + row.durationMs : 0));
  return { rows, extentMs };
}
