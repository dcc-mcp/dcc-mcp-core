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
| `GET /admin/api/stats?range=1h\|24h\|7d` | Compute success rate, latency percentiles, and top tools/instances from the trace log. |
| `GET /admin/api/workers` | Inspect per-instance worker cards from the live registry. |

By default these buffers are in memory only. Set `DCC_MCP_GATEWAY_AUDIT_DIR` to append bounded JSONL files:

- `audit.jsonl` — rows backing `/admin/api/calls`.
- `traces.jsonl` — rows backing `/admin/api/traces` and stats.

`DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` (default `5000`) caps each file. On restart, the gateway seeds the in-memory admin buffers from those files. The persisted trace payloads use the same bounded/redacted `TracePayload` values as the live API; unbounded raw request bodies are not stored.

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
