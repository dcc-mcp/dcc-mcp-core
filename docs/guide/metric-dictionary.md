# Metric Dictionary

Canonical reference for every metric, dimension, and aggregated statistic that
dcc-mcp-core observability surfaces produce. Use this dictionary to understand
units, sampling windows, null/missing-value semantics, and which storage layer
holds each metric.

---

## 1. Gateway Admin SQLite — Persistent Metrics

The gateway writes structured rows to the shared `gateway_admin.sqlite`
database. This is the authoritative per-machine store for tool-call traces,
session lifecycle, and aggregated statistics.

### 1.1 `tool_calls` table

| Column | Type | Unit | Sampling | Missing semantics |
|--------|------|------|----------|-------------------|
| `request_id` | TEXT | — | Every tool call | NOT NULL, PK |
| `session_id` | TEXT | — | Every tool call | NOT NULL — empty string when no session |
| `parent_request_id` | TEXT | — | When attributed | `NULL` = orphan (no parent chain) |
| `batch_id` | TEXT | — | When inside a batch | `NULL` = standalone call |
| `tool_name` | TEXT | — | Every tool call | NOT NULL |
| `skill_name` | TEXT | — | When known | `NULL` = direct tool (no skill wrapper) |
| `dcc_type` | TEXT | — | When known | `NULL` = gateway-level call |
| `instance_id` | TEXT | — | When known | `NULL` = no target instance resolved |
| `agent_id` | TEXT | — | When caller provides | `NULL` = anonymous |
| `transport` | TEXT | — | Every call | `"mcp"`, `"rest"`, or `NULL` on legacy paths |
| `via_gateway` | INTEGER | boolean | Every call | `0` = direct DCC, `1` = via gateway, `NULL` = unknown |
| `started_at_ms` | INTEGER | ms since epoch | Every call | NOT NULL |
| `duration_ms` | INTEGER | ms | Every call | NOT NULL — wall-clock, may be 0 for sub-ms calls |
| `success` | INTEGER | boolean | Every call | `0` = failure, `1` = success |
| `error_message` | TEXT | — | On failure | `NULL` on success |
| `error_kind` | TEXT | — | On classified failure | `NULL` = unclassified |
| `mcp_method` | TEXT | — | Every call | Always `"tools/call"` in practice |
| `trace_id` | TEXT | — | When traceparent attached | `NULL` = no distributed trace |
| `span_id` | TEXT | — | When span attached | `NULL` = no span |

**Row retention**: bounded by the gateway ring buffer (default 5000 rows in
JSONL; SQLite mirror retains all rows). The SQLite lane is append-only with
no automatic purge — operators may delete old partitions.

### 1.2 `sessions` table

| Column | Type | Unit | Sampling | Missing semantics |
|--------|------|------|----------|-------------------|
| `session_id` | TEXT | — | Every session start | NOT NULL, PK |
| `parent_session_id` | TEXT | — | When spawned | `NULL` = root session |
| `dcc_type` | TEXT | — | Every session | NOT NULL |
| `instance_id` | TEXT | — | When known | `NULL` = unknown instance |
| `status` | TEXT | — | Every session | One of: `active`, `ended`, `crashed`, `disconnected`, `gpu_crashed`, `timed_out`, `cancelled`, `thread_affinity_failure` |
| `started_at_ms` | INTEGER | ms since epoch | Every session | NOT NULL |
| `last_activity_at_ms` | INTEGER | ms since epoch | Every session | Updated on each tool call |
| `ended_at_ms` | INTEGER | ms since epoch | On end | `NULL` while session is active |
| `end_reason_json` | TEXT | — | On non-normal end | `NULL` for normal end |
| `tool_call_count` | INTEGER | count | Every session | Defaults to 0 |
| `error_count` | INTEGER | count | Every session | Defaults to 0 |
| `core_version` | TEXT | — | Every session | NOT NULL — semver of dcc-mcp-core |
| `adapter_version` | TEXT | — | When DCC adapter provides | `NULL` = unknown |
| `build_sha` | TEXT | — | When build metadata available | `NULL` = dev build |

**Session status semantics**:

| Status | Meaning |
|--------|---------|
| `active` | Session is live and accepting tool calls |
| `ended` | Clean shutdown via `on_session_end` |
| `crashed` | DCC host process terminated unexpectedly |
| `gpu_crashed` | GPU device lost / TDR event |
| `disconnected` | Gateway lost contact with the DCC backend |
| `timed_out` | Session exceeded the configured idle timeout |
| `cancelled` | Client explicitly cancelled the session |
| `thread_affinity_failure` | DCC thread-affinity constraint could not be satisfied |

### 1.3 `session_events` table

| Column | Type | Unit | Sampling | Missing semantics |
|--------|------|------|----------|-------------------|
| `id` | INTEGER | — | Every event | PK, auto-increment |
| `session_id` | TEXT | — | Every event | NOT NULL |
| `event_type` | TEXT | — | Every event | See table below |
| `event_json` | TEXT | — | Every event | NOT NULL — JSON payload |
| `created_at_ms` | INTEGER | ms since epoch | Every event | NOT NULL |

**Event types**:

| Event | Payload | When emitted |
|-------|---------|-------------|
| `session_start` | `{ session_id, dcc_type, instance_id, core_version }` | On first `initialize` |
| `session_end` | `{ session_id, reason }` | On clean close |
| `session_crash` | `{ session_id, signal?, exit_code? }` | On process termination |
| `session_timeout` | `{ session_id, idle_seconds }` | On idle timeout expiry |
| `tool_call_start` | `{ request_id, tool_name, session_id }` | Before dispatch |
| `tool_call_end` | `{ request_id, success, duration_ms }` | After completion |

---

## 2. Prometheus Exporter — Runtime Metrics

Available when compiled with the `prometheus` Cargo feature. Mounted at
`GET /metrics` on the same Axum router as the MCP server.

### 2.1 Per-DCC Server Metrics

| Metric | Type | Unit | Labels | Missing/zero semantics |
|--------|------|------|--------|----------------------|
| `dcc_mcp_tool_calls_total` | counter | invocations | `tool`, `status` | Absent until first call. `status="success"` or `status="error"`. |
| `dcc_mcp_tool_duration_seconds` | histogram | seconds | `tool` | No observations = zero histogram. Buckets: log-ish 1 ms → 30 s. |
| `dcc_mcp_jobs_in_flight` | gauge | count | `tool` | 0 when no jobs are running. |
| `dcc_mcp_job_created_total` | counter | jobs | `tool`, `result` | `result` ∈ `{accepted, queue_full}`. |
| `dcc_mcp_job_wait_seconds` | histogram | seconds | `tool` | Zero for instantly-scheduled jobs. |
| `dcc_mcp_notifications_sent_total` | counter | notifications | `channel` | `channel` ∈ `{sse, ws}`. |
| `dcc_mcp_active_sessions` | gauge | count | — | 0 when no sessions. Refreshed every 5 s. |
| `dcc_mcp_registered_tools` | gauge | count | — | 0 when registry empty. Refreshed every 5 s. |
| `dcc_mcp_build_info` | gauge | — | `version`, `crate` | Always 1. Published once at startup. |

**Sampling window**: gauge metrics refresh on a 5-second background tick.
Counters and histograms advance inline on the `tools/call` hot path.

### 2.2 Gateway-Only Metrics

| Metric | Type | Unit | Labels | Missing/zero semantics |
|--------|------|------|--------|----------------------|
| `dcc_mcp_gateway_elections_total` | counter | elections | `outcome` | `outcome` ∈ `{won, yielded, lost}` |
| `dcc_mcp_gateway_evictions_total` | counter | evictions | `reason` | `reason` ∈ `{stale, ghost, probe_fail}` |
| `dcc_mcp_gateway_probes_total` | counter | probes | `outcome` | `outcome` ∈ `{ready, booting, unreachable}` |
| `dcc_mcp_gateway_governance_events_total` | counter | events | `category`, `outcome` | `category` ∈ `{policy, rate-limit}`; `outcome` ∈ `{allowed, denied, throttled}` |

---

## 3. Observability Query API — Aggregated Metrics

The Python `ObservabilityQuery` class provides method-level aggregates computed
from the gateway admin SQLite database (Section 1).

### 3.1 Session Stats

Field returned by `get_session_stats()`:

| Field | Type | Unit | Sampling window | Missing/zero |
|-------|------|------|-----------------|-------------|
| `total_sessions` | integer | count | Filter time range | 0 when no sessions match |
| `active_sessions` | integer | count | Filter time range | 0 when no active sessions |
| `ended_normally` | integer | count | Filter time range | 0 |
| `ended_abnormally` | integer | count | Filter time range | 0 |
| `avg_duration_ms` | float | ms | Filter time range | 0.0 when no ended sessions |
| `total_tool_calls` | integer | count | Filter time range | 0 |
| `total_errors` | integer | count | Filter time range | 0 |

### 3.2 Tool Call Stats

Field returned by `get_tool_call_stats()`:

| Field | Type | Unit | Sampling window | Missing/zero |
|-------|------|------|-----------------|-------------|
| `total_calls` | integer | count | Filter time range + limit | 0 |
| `success_count` | integer | count | Filter time range + limit | 0 |
| `failure_count` | integer | count | Filter time range + limit | 0 |
| `success_rate` | float | ratio [0–1] | Filter time range + limit | 0.0 when zero total |
| `avg_duration_ms` | float | ms | Filter time range + limit | 0.0 |

Each row in the `events` sub-list repeats the column semantics from the
`tool_calls` table (Section 1.1).

### 3.3 Coverage Stats

Field returned by `get_coverage_stats()`:

| Field | Type | Unit | Sampling window | Missing/zero |
|-------|------|------|-----------------|-------------|
| `observed_requests` | integer | count | Filter time range | 0 |
| `unobserved_requests` | integer | count | Filter time range | 0 |
| `coverage_ratio` | float | ratio [0–1] | Filter time range | 0.0 when zero total |

### 3.4 Crash Stats

Field returned by `get_crash_stats()`:

| Field | Type | Unit | Sampling window | Missing/zero |
|-------|------|------|-----------------|-------------|
| `total_crashes` | integer | count | Filter time range | 0 |
| `host_crashes` | integer | count | Filter time range | 0 |
| `gpu_crashes` | integer | count | Filter time range | 0 |

### 3.5 Funnel Stats

Field returned by `get_funnel_stats()`:

| Field | Type | Unit | Sampling window | Missing/zero |
|-------|------|------|-----------------|-------------|
| `searches_total` | integer | count | Filter time range | 0 until search instrumentation is wired |
| `searches_zero_results` | integer | count | Filter time range | 0 |
| `skills_loaded` | integer | count | Filter time range | 0 |
| `skills_called` | integer | count | Filter time range | Upper bound from `tool_calls` count |
| `skills_succeeded` | integer | count | Filter time range | 0 |
| `script_fallbacks` | integer | count | Filter time range | 0 |
| `ui_control_fallbacks` | integer | count | Filter time range | 0 |

---

## 4. In-Process Python Telemetry

The `ToolRecorder` class provides in-memory percentiles without a database.

### `ToolMetrics` fields

| Field | Type | Unit | Sampling | Missing/zero |
|-------|------|------|----------|-------------|
| `invocation_count` | integer | count | Every `start()`/`finish()` | 0 = never recorded |
| `success_count` | integer | count | Same | 0 |
| `failure_count` | integer | count | Same | 0 |
| `avg_duration_ms` | float | ms | Same | 0.0 |
| `p95_duration_ms` | float | ms | Same | 0.0 when < 20 samples |
| `p99_duration_ms` | float | ms | Same | 0.0 when < 100 samples |
| `success_rate()` | float | ratio [0–1] | Same | 0.0 |

**Percentile accuracy**: p95 stabilises after ~20 samples; p99 after ~100.
Below those thresholds, percentiles may match the maximum observed value.

---

## 5. Admin API Statistics

The `GET /admin/api/stats` endpoint computes on-demand from the trace ring
buffer (default 200 traces, configurable up to 5000).

| Group | Field | Unit | Missing/zero |
|-------|-------|------|-------------|
| Overall | `total_calls` | count | 0 |
| | `success_rate` | ratio [0–1] | 0.0 |
| | `avg_duration_ms` | ms | 0.0 |
| Latency | `p50_duration_ms` | ms | 0.0 |
| | `p95_duration_ms` | ms | 0.0 |
| | `p99_duration_ms` | ms | 0.0 |
| Top tools | `tool`, `call_count` | count | Empty list |
| Top instances | `instance_id`, `call_count` | count | Empty list |
| Hourly | `hour` (UTC 0–23), `call_count` | count | Zero-filled 24-bucket array |

**Range filter**: `?range=1h|24h|7d|all`. Default `all` = entire ring buffer.
StatsFilter supports optional `dcc_type`, `skill`, `tool`, `status`, `instance_id`,
and `session_id` dimensions.

---

## 6. OTLP Distributed Tracing

OpenTelemetry-compatible spans with these key attributes:

| Attribute | Type | Unit | Present | Missing |
|-----------|------|------|---------|---------|
| `dcc.type` | string | — | Every DCC span | `NULL` on gateway-only spans |
| `dcc.instance_id` | string | — | When resolved | `NULL` |
| `mcp.tool_slug` | string | — | `tools/call` spans | `NULL` |
| `mcp.session_id` | string | — | Session context | `NULL` |
| `dcc_mcp.success` | bool | — | Every span | `false` on error |

See the full attribute table in [observability.md](observability.md).

---

## Index

| Surface | Storage | Query interface | Retention |
|---------|---------|----------------|-----------|
| SQLite `tool_calls` | `gateway_admin.sqlite` | `ObservabilityQuery`, Admin API | Append-only |
| SQLite `sessions` | `gateway_admin.sqlite` | `ObservabilityQuery`, Admin API | Append-only |
| SQLite `session_events` | `gateway_admin.sqlite` | Admin API | Append-only |
| Prometheus | In-process registry | `GET /metrics` | Resets on process restart |
| OTLP traces | External collector | Jaeger/Tempo/Phoenix | Collector-managed |
| ToolRecorder | In-process memory | Python `metrics()` | Resets on `reset()` or process restart |
| Admin audit ring | In-memory + optional JSONL | `GET /admin/api/calls` | Default 5000 rows |
| Admin trace ring | In-memory + optional JSONL | `GET /admin/api/traces` | Default 200 traces |
| Admin stats | On-demand from trace ring | `GET /admin/api/stats` | Same as trace ring |
