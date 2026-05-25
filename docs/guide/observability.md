# Observability

dcc-mcp-core provides complementary observability surfaces for production deployments.

## 1. OTLP Distributed Tracing (#768)

Wire spans to any OpenTelemetry-compatible backend (Jaeger, Grafana Tempo, DataDog, New Relic, etc.).

### Activation

Set the standard `OTEL_*` environment variables — no code changes needed:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
OTEL_SERVICE_NAME=dcc-mcp-gateway \
  dcc-mcp-server ...
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Collector endpoint — setting this activates OTLP automatically |
| `OTEL_SERVICE_NAME` | Override service name in traces |
| `OTEL_RESOURCE_ATTRIBUTES` | Extra resource attrs (`key=val,key2=val2`) |
| `OTEL_EXPORTER_OTLP_HEADERS` | Auth headers for SaaS backends (e.g. `api-key=...`) |
| `OTEL_TRACES_SAMPLER` | Sampler (`always_on`, `always_off`, `traceidratio`) |
| `OTEL_TRACES_SAMPLER_ARG` | Sampler arg (e.g. `0.1` for 10% sampling) |

### DCC Span Attributes

Every `tools/call` trace includes:

| Attribute | Example | Description |
|-----------|---------|-------------|
| `dcc.type` | `"maya"` | DCC application type |
| `dcc.instance_id` | `"a1b2c3d4-..."` | Unique DCC instance UUID |
| `dcc.scene` | `"/projects/shot01.ma"` | Current scene path (when known) |
| `dcc.job_id` | `"job-..."` | Job ID (when wrapped by `JobHandle`) |
| `mcp.method` | `"tools/call"` | MCP method name |
| `mcp.tool_slug` | `"maya__open_scene"` | Full tool name |
| `mcp.affinity` | `"main"` | Thread affinity requirement |
| `mcp.session_id` | `"sess-..."` | MCP session ID |
| `mcp.request_id` | `"req-..."` | Per-request unique ID |

### Gateway Agent Workflow Spans (#1180)

The gateway also emits bounded agent workflow spans when discovery and dynamic
capability calls pass through REST or MCP:

| Span | Meaning |
|------|---------|
| `gateway.search` | Agent searched for a tool or skill. |
| `gateway.describe` | Agent inspected a selected tool. |
| `gateway.load_skill` | Agent loaded a skill discovered through search. |
| `gateway.call` | Agent invoked one selected backend tool. |
| `gateway.call_batch` | Agent invoked an ordered batch. |

These spans use `openinference.span.kind` (`CHAIN` for search and `TOOL` for
describe/load/call) plus a documented `dcc_mcp.*` attribute namespace for
gateway-specific fields:

| Attribute | Description |
|-----------|-------------|
| `dcc_mcp.workflow.operation` | One of the span names above. |
| `dcc_mcp.transport` | `rest` or `mcp`. |
| `dcc_mcp.trace_id`, `dcc_mcp.request_id`, `dcc_mcp.parent_request_id`, `dcc_mcp.session_id` | Correlation IDs that match Admin trace/debug-bundle surfaces. |
| `dcc_mcp.agent.id`, `.name`, `.kind`, `.model`, `.model_provider`, `.model_version`, `.reasoning_effort`, `.turn_id`, `.task`, `.tags` | Bounded `agent_context` / caller metadata. |
| `dcc_mcp.agent.user_intent_summary`, `.reply_summary`, `.user_input_hash`, `.reply_hash`, `.user_input_chars`, `.reply_chars` | Low-sensitivity turn summaries, hashes, and lengths for evaluation correlation. |
| `dcc_mcp.dcc.type`, `dcc_mcp.instance.id`, `dcc_mcp.skill.name`, `dcc_mcp.tool.slug` | Selected DCC route and skill/tool identity. |
| `dcc_mcp.search.id`, `.ranker_version`, `.selected_rank`, `.score`, `.match_reasons`, `.total`, `.zero_results` | Search-quality context carried from `/v1/search` or gateway `search`. |
| `dcc_mcp.policy.outcome`, `.reason` | Whether gateway policy allowed, denied, or throttled the action and why. |
| `dcc_mcp.success`, `dcc_mcp.error.kind`, `dcc_mcp.batch.size` | Execution outcome fields. |

The gateway intentionally does **not** export hidden chain-of-thought, raw
prompts, raw agent replies, unbounded request bodies, secrets, or arbitrary
`agent_context` metadata. Put `search_id` in REST `meta.search_id` or MCP
`_meta.search_id` when following a search hit into `describe`, `load_skill`,
`call`, or `call_batch`; that is what lets OTLP traces connect selected rank
and score to actual tool outcomes. Add `turn_id` and model identity to
`agent_context` when an evaluation needs to correlate search quality,
time-to-first-success, and downstream call success back to one agent turn.

### Trace Context

Gateway observability keeps these identifiers separate:

| Field | Meaning |
|-------|---------|
| `trace_id` | End-to-end unit of work. REST callers can provide it with W3C `traceparent`; otherwise the gateway generates one. |
| `request_id` | One gateway-facing HTTP/MCP request. REST uses `X-Request-Id`/`X-Correlation-Id` or generates one; MCP uses the JSON-RPC `id`. |
| `span_id` / `parent_span_id` | Timed segment identity and causal parent from W3C `traceparent`. |
| `parent_request_id` | Request-level parent/child relationship for agent turns, batches, jobs, and retries. |

`traceparent` is never used as `request_id`. When both `X-Request-Id` and
`traceparent` are present, admin traces record the request id from
`X-Request-Id` and the trace id from the W3C header. Backend REST calls receive
`traceparent`, `tracestate`, `X-Request-Id`, and
`X-Dcc-Mcp-Parent-Request-Id` when available.

### Rust Config (programmatic)

```rust
use dcc_mcp_telemetry::{TelemetryConfig, ExporterBackend};

TelemetryConfig::default()
    .with_otlp_exporter("http://localhost:4317")
    .init()?;
```

Requires the `otlp-exporter` Cargo feature:

```toml
dcc-mcp-telemetry = { features = ["otlp-exporter"] }
```

### Quick Start: Jaeger All-in-One

```bash
docker run -d --name jaeger \
  -p 4317:4317 \   # OTLP gRPC
  -p 16686:16686 \ # Jaeger UI
  jaegertracing/all-in-one:latest

OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
OTEL_SERVICE_NAME=dcc-mcp-gateway \
  dcc-mcp-server ...
```

Open `http://localhost:16686` to view traces.

### Quick Start: Grafana Tempo

```bash
# docker-compose.yml
services:
  tempo:
    image: grafana/tempo:latest
    ports: ["4317:4317", "3200:3200"]
```

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 dcc-mcp-server ...
```

### Quick Start: Phoenix Through an OTLP Collector

Phoenix accepts OTLP/HTTP traces at `/v1/traces`. The Rust gateway exporter
uses OTLP/gRPC today, so route it through the OpenTelemetry Collector:

```yaml
# otel-collector.yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317

exporters:
  otlphttp/phoenix:
    traces_endpoint: http://phoenix:6006/v1/traces

service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [otlphttp/phoenix]
```

```bash
docker run -d --name phoenix -p 6006:6006 arizephoenix/phoenix:latest
docker run --rm -p 4317:4317 \
  -v "$PWD/otel-collector.yaml:/etc/otelcol-contrib/config.yaml" \
  otel/opentelemetry-collector-contrib:latest

OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
OTEL_SERVICE_NAME=dcc-mcp-gateway \
  dcc-mcp-server ...
```

---

## 2. Gateway Contention Events (#766)

Gateway election, eviction, and probe events are available as a bounded MCP resource.

### MCP Resource

```python
# Read the last N contention events
result = resources.read("resources://gateway/events")
```

Returns JSONL (one JSON object per line):

```json
{"timestamp":"2026-05-05T10:00:00Z","event":"election_won","dcc_type":"maya","instance_id":"a1b2c3d4","reason":null}
{"timestamp":"2026-05-05T10:01:00Z","event":"ghost_reaped","dcc_type":"blender","instance_id":"b2c3d4e5","reason":"pid_dead"}
```

### Event Types

| Event | Meaning |
|-------|---------|
| `election_won` | This instance became the active gateway |
| `voluntary_yield` | This instance yielded to a newer candidate |
| `ghost_reaped` | A stale registration was cleaned up |
| `probe_booting` | Backend is starting up |
| `probe_unreachable` | Health probe failed |
| `auto_deregister` | Instance deregistered itself on clean shutdown |

Ring buffer holds the **last 1000 events**.

---

## 3. Admin Call Audit and Dispatch Traces

The elected gateway exposes a read-only HTML dashboard at `GET /admin` and machine-readable JSON endpoints for operators and AI agents:

| Endpoint | Use when |
|----------|----------|
| `GET /admin/api/calls` | Correlate recent calls by `request_id`, tool slug, DCC type, instance, error preview, and duration. |
| `GET /admin/api/traces?limit=200` | Inspect recent dispatch waterfalls, bounded input payloads (16 KiB), and bounded output payloads (64 KiB). |
| `GET /admin/api/traces/{request_id}` | Drill into one call without scanning the whole trace ring. |
| `GET /v1/debug/traces/{trace_id}` | Stable debug lookup by trace id or request id. |
| `GET /v1/debug/bundles/{trace_id}` | Full-chain debug bundle across every retained request in a trace. |
| `GET /admin/api/workflows?limit=200` | Group retained searches, describes, skill loads, calls, traces, and audits into agent session/workflow chains. |
| `GET /admin/api/stats?range=1h\|24h\|7d` | Compute success rate, latency percentiles, and top tools/instances/agents from the trace log. |
| `GET /admin/api/governance?limit=300` / `GET /v1/debug/governance` | Inspect effective policy, read-only state, traffic capture guardrails, redaction paths, middleware quota state, and recent allowed/denied/throttled decisions. |
| `GET /admin/api/workers` | Inspect per-instance worker cards from the live registry. |

By default these buffers are in memory only. Set `DCC_MCP_GATEWAY_AUDIT_DIR` to append bounded JSONL files:

- `audit.jsonl` — rows backing `/admin/api/calls`.
- `traces.jsonl` — rows backing `/admin/api/traces` and stats.

`DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` (default `5000`) caps each file. On restart, the gateway seeds the in-memory admin buffers from those files. The persisted trace payloads use the same bounded/redacted `TracePayload` values as the live API; unbounded raw request bodies are not stored.

MCP and REST clients can attach optional agent/caller context to correlate an
operator-visible request with the caller's explicit plan and observations. Use
`params._meta.agent_context` for MCP, REST `meta.agent_context` or
`caller_context` fields, or `x-dcc-mcp-agent-*` headers. Fields are bounded and
intended for concise telemetry such as `agent_id`, `agent_name`,
`model_provider`, `model_version`, `model`, `reasoning_effort`, `session_id`,
`turn_id`, `user_intent_summary`, `agent_reply_summary`, `user_input_hash`,
`agent_reply_hash`, `user_input_chars`, `agent_reply_chars`, `task`,
`reasoning_summary`, `plan`, `observations`, `parent_request_id`, and tags; do
not send hidden chain-of-thought, raw user input, raw agent replies, or secrets.
`trace_id`, `request_id`,
`span_id`, and `parent_span_id` are recorded separately so one trace can contain
multiple request ids without losing per-request compatibility. Admin call and trace rows
also include absolute `links` for the trace page, trace API, debug bundle,
OpenAPI Inspector, OpenAPI spec/docs, and stats page so operators can paste a
complete, replayable investigation target into an LLM evaluation or
code-optimization prompt. The same link set includes `issue_report_url`, a
standalone JSON export shaped for GitHub issue attachments with summary
metadata, a suggested issue title/body, and the correlated debug bundle.
`/admin/api/workflows` and `/v1/debug/workflows` reuse the same stores to show
session/workflow rows with bounded agent metadata, selected search rank,
zero-result searches, time-to-first-success, and step links back to trace
detail, debug bundles, issue reports, OpenAPI, and docs.
When a live worker exposes an `mcp_url`, the Admin Dashboard also derives that
worker's `/v1/openapi.json`, `/docs`, and instance-scoped OpenAPI Inspector
links, making it possible to follow a gateway trace down to the exact backend
REST contract that served the request.

---

## 4. Adapter Session/Job Events (#1078)

DCC adapters can expose runtime output without inventing adapter-specific
diagnostics tools by registering a bounded `SessionEventBuffer`:

```python
from dcc_mcp_core import SessionEventBuffer

events = SessionEventBuffer("houdini-001")
server.resources().register_session_event_buffer(events)
events.append(
    source="python",
    stream="stderr",
    level="warning",
    message="Cook produced warnings",
    tool_call_id="req-18",
    job_id="job-18",
)
```

Clients poll the resource by cursor:

```text
events://session/houdini-001?cursor=0&limit=100
```

Each event carries a monotonic cursor id, timestamp, source, stream, level,
message, optional truncation metadata, structured metadata, and optional
`session_id` / `tool_call_id` / `job_id` / `correlation_id`. Buffers are
memory bounded and deterministic: old events drop from the front, and long
messages are truncated at UTF-8 boundaries with original/returned sizes.

---

## 5. Prometheus Metrics (#766)

Under the `prometheus` Cargo feature, gateway contention counters are exposed at `/metrics`:

```toml
dcc-mcp-gateway = { features = ["prometheus"] }
```

### Counters

```
# Gateway port-election outcomes
dcc_mcp_gateway_elections_total{outcome="won"}     12
dcc_mcp_gateway_elections_total{outcome="yielded"}  3
dcc_mcp_gateway_elections_total{outcome="lost"}     1

# Registry eviction events
dcc_mcp_gateway_evictions_total{reason="stale"}    1
dcc_mcp_gateway_evictions_total{reason="ghost"}    0
dcc_mcp_gateway_evictions_total{reason="probe_fail"} 2

# Backend readiness-probe outcomes
dcc_mcp_gateway_probes_total{outcome="ready"}      45
dcc_mcp_gateway_probes_total{outcome="booting"}     3
dcc_mcp_gateway_probes_total{outcome="unreachable"} 2

# Gateway governance outcomes
dcc_mcp_gateway_governance_events_total{category="policy",outcome="denied"} 4
dcc_mcp_gateway_governance_events_total{category="rate-limit",outcome="throttled"} 3
```

Label cardinality is bounded — no free-form `instance_id` labels.

### Grafana Example Queries

```promql
# Election rate over time
rate(dcc_mcp_gateway_elections_total[5m])

# Eviction rate
rate(dcc_mcp_gateway_evictions_total[5m])

# Probe success ratio
rate(dcc_mcp_gateway_probes_total{outcome="ready"}[5m])
  / rate(dcc_mcp_gateway_probes_total[5m])
```

---

## See also

- [telemetry.md](telemetry.md) — `ToolMetrics`, `ToolRecorder`, legacy Python telemetry
- [adapter-runtime-contracts.md](adapter-runtime-contracts.md) — session events, artefacts, debug descriptors, UI automation contracts
- [gateway-diagnostics.md](gateway-diagnostics.md) — log templates for election/eviction events
- [production-deployment.md](production-deployment.md) — production monitoring checklist
- [middleware.md](middleware.md) — `AuditMiddleware` for per-call audit logging
