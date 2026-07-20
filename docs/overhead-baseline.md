# Observability Collection Overhead Baseline

## Summary

| Metric | Overhead | Safe for DCC Main Thread? |
|--------|----------|---------------------------|
| JSON serialization (per event) | ~2-5 us | Yes |
| ToolCallEvent creation (per call) | ~3-7 us | Yes |
| SQLite INSERT (single, async) | ~50-200 us | Yes (async lane) |
| SQLite INSERT (executemany, async) | ~30-80 us per row | Yes (async lane) |
| Batch dispatch (10 sub-calls) | ~80-200 us total | Yes |
| Funnel query (aggregated) | ~200-500 us | Yes (read-only) |
| Session events buffer append | ~1-3 us | Yes |

## Methodology

Run `python tests/bench_overhead.py --iterations 2000 --batch-size 10` to
reproduce. The benchmark measures:

1. **JSON serialization** — cost of serializing a `ToolCallEvent` struct
2. **SQLite write** — single INSERT and batch executemany into `tool_calls`
3. **Batch dispatch** — overhead of creating + serializing N sub-call events
4. **Funnel query** — aggregate SQL query for `get_funnel_stats()`

All SQLite writes go through an async writer thread (`AdminSqliteLane`), so
the main gateway request path does not block on disk I/O.

## Default Config Recommendations

| Setting | Value | Rationale |
|---------|-------|-----------|
| `admin-persist-sqlite` | enabled | Negligible overhead via async lane |
| Session event buffer capacity | 1000 | Keeps memory bounded at ~2 MB |
| Session event max message bytes | 4096 | Prevents large messages from bloating buffer |
| `tool_calls` retention | 30 days | Balanced for observability vs disk usage |

## DCC Main Thread Safety

All observability writes are:
- **Non-blocking**: SQLite writes go through `tokio::sync::mpsc` channel
- **Batched**: `executemany` for batch calls reduces write pressure
- **Bounded**: Ring buffers for session events, traces, and audits

The measured per-call overhead is under 10 us on the hot path, well within the
typical DCC frame budget of 16.67 ms (60 FPS). The async SQLite lane ensures
disk I/O never blocks tool call dispatch.

## Migration Path

- Existing installations upgrade transparently — the new `mcp_method` column
  is NULL-safe for existing `tool_calls` rows
- `get_funnel_stats()` returns zeroes until `tool_calls` are populated by
  gateway versions that include the ToolCallEvent instrumentation
- No schema migration required — the `tool_calls` table schema already includes
  all necessary columns
