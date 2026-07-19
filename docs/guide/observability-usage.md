# Observability Usage Guide

How to consume observability data from each interface: AI agents, the CLI, and
the Admin UI.

---

## 1. AI Agent Usage

Agents can query observability data programmatically through the
`ObservabilityQuery` Python API.

### Prerequisites

The gateway admin SQLite database must exist on the machine. It is created
automatically when the gateway runs for the first time. The default path is:

```
<tempdir>/dcc-mcp-registry/gateway_admin.sqlite
```

Override with `DCC_MCP_GATEWAY_ADMIN_DB` environment variable.

### Basic queries

```python
from dcc_mcp_core import ObservabilityQuery

query = ObservabilityQuery(db_path="/path/to/gateway_admin.sqlite")

# Session statistics
stats = query.get_session_stats(
    dcc_type="maya",
    since_ms=1700000000000,
)

print(stats["data"])
# {
#   "total_sessions": 42,
#   "active_sessions": 3,
#   "ended_normally": 35,
#   "ended_abnormally": 4,
#   "avg_duration_ms": 312000.0,
#   "total_tool_calls": 1280,
#   "total_errors": 15,
# }

# Tool-call statistics
tool_stats = query.get_tool_call_stats(
    tool_name="maya__poly_sphere",
    since_ms=1700000000000,
    limit=50,
)

print(tool_stats["data"]["stats"])
# {
#   "total_calls": 200,
#   "success_count": 195,
#   "failure_count": 5,
#   "success_rate": 0.975,
#   "avg_duration_ms": 45.2,
# }

print(tool_stats["data"]["events"])  # list of individual call records
```

### Session tree

```python
# All root sessions
tree = query.get_session_tree(dcc_type="maya")

# Drill into one session
children = query.get_session_tree(root_session_id="sess-abc123")
```

### Coverage and crash stats

```python
# Gateway coverage ratio
cov = query.get_coverage_stats(since_ms=1700000000000)

# Crash analysis
crashes = query.get_crash_stats(dcc_type="houdini", since_ms=1700000000000)
```

### Diagnostic checks

```python
# Does the database exist?
from pathlib import Path
db = Path("/path/to/gateway_admin.sqlite")
if not db.exists():
    print("Gateway has never run on this machine — no observability data.")

# Are there recent sessions?
stats = query.get_session_stats()
if stats["data"]["total_sessions"] == 0:
    print("No sessions recorded yet.")
```

### Agent context for call attribution

When making MCP or REST calls through the gateway, attach agent context for
observability correlation:

```python
# MCP: attach context to params._meta
params = {
    "name": "maya__poly_sphere",
    "arguments": {"radius": 2.0},
    "_meta": {
        "agent_context": {
            "agent_id": "my-agent",
            "agent_name": "SceneBuilder",
            "turn_id": "turn-42",
            "model": "claude-sonnet-4-20250514",
        }
    }
}
```

```json
// REST: attach context to meta.agent_context
POST /v1/call
{
  "tool": "maya__poly_sphere",
  "args": {"radius": 2.0},
  "meta": {
    "agent_context": {
      "agent_id": "my-agent",
      "turn_id": "turn-42"
    }
  }
}
```

---

## 2. CLI Usage

The `dcc-mcp-cli stats` command queries the gateway admin SQLite database.

### Quick start

```bash
# Show summary statistics
dcc-mcp-cli stats

# Filter by time range
dcc-mcp-cli stats --since 1h
dcc-mcp-cli stats --since 24h
dcc-mcp-cli stats --since 7d

# Filter by DCC type
dcc-mcp-cli stats --dcc maya
dcc-mcp-cli stats --dcc blender

# Show top tools
dcc-mcp-cli stats --top-tools 10

# Show session breakdown
dcc-mcp-cli stats --sessions
```

### Output format

```
=== Session Statistics (last 24h) ===
Total sessions:    42
Active:             3
Ended normally:    35
Ended abnormally:   4
Avg duration:     5.2m
Total tool calls: 1,280
Total errors:       15

=== Tool Call Performance ===
Tool                  Calls  Success  P95(ms)  P99(ms)
maya__poly_sphere      200   97.5%     120      340
blender__bevel_edge     50   94.0%      85      210
houdini__vdb_fog        30   96.7%     450     1200

=== Coverage ===
Observed:   1,250
Unobserved:   30
Coverage:   97.7%
```

---

## 3. Admin UI Usage

The gateway serves a read-only HTML dashboard at `GET /admin` when the gateway
process is running.

### Dashboard sections

| Tab | Endpoint | What you see |
|-----|----------|-------------|
| **Calls** | `GET /admin/api/calls` | Recent audit records: request_id, tool, DCC, duration, success/failure. |
| **Traces** | `GET /admin/api/traces?limit=200` | Dispatch waterfalls with bounded input/output payloads. |
| **Stats** | `GET /admin/api/stats?range=1h` | Aggregated success rate, latency percentiles, top tools/instances, hourly distribution. |
| **Workflows** | `GET /admin/api/workflows?limit=200` | Session/workflow chains with agent metadata and step links. |
| **Workers** | `GET /admin/api/workers` | Per-instance registry cards (live DCC backends). |
| **Traffic** | `GET /admin/api/traffic?limit=300` | Capture status and retained metadata-only frames. |
| **Governance** | `GET /admin/api/governance?limit=300` | Policy decisions, redaction paths, quota state. |
| **Skills** | Admin UI page | Skill paths, load states, health. |
| **Marketplace** | Admin UI page | Marketplace extension browsing. |
| **Instances** | Admin UI page | DCC instances and their connection status. |

### Enabling audit persistence

By default audit data lives in an in-memory ring buffer. For persistence across
gateway restarts, set:

```bash
DCC_MCP_GATEWAY_AUDIT_DIR=/var/log/dcc-mcp/audit  # enables JSONL files
DCC_MCP_GATEWAY_AUDIT_MAX_ROWS=5000                # default, rows per file
```

When set, the gateway writes `audit.jsonl` and `traces.jsonl` to that directory
and seeds the in-memory buffers from these files on restart.

### Enabling SQLite persistence

Build the gateway with the `admin-persist-sqlite` Cargo feature:

```toml
dcc-mcp-gateway = { features = ["admin-persist-sqlite"] }
```

The gateway then persists every trace, audit, skill path change, and
deregistered instance to `gateway_admin.sqlite`. The SQLite lane runs on a
background writer thread and never blocks the hot path.

### Prometheus integration

```bash
# Scrape a DCC server's metrics
curl -u scraper:change-me http://localhost:8765/metrics

# Add to prometheus.yml
scrape_configs:
  - job_name: dcc-mcp
    scrape_interval: 15s
    static_configs:
      - targets: ["host:8765", "host:8766"]
    metrics_path: /metrics
    basic_auth:
      username: scraper
      password_file: /etc/prometheus/dcc-mcp.pass
```

---

## 4. OTLP Trace Correlation

### Jaeger

```bash
docker run -d --name jaeger \
  -p 4317:4317 \
  -p 16686:16686 \
  jaegertracing/all-in-one:latest

OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
OTEL_SERVICE_NAME=dcc-mcp-gateway \
  dcc-mcp-server
```

### Grafana Tempo

```yaml
# docker-compose.yml
services:
  tempo:
    image: grafana/tempo:latest
    ports: ["4317:4317", "3200:3200"]
```

### Phoenix

Route through an OpenTelemetry Collector (the Rust exporter uses OTLP/gRPC;
Phoenix accepts OTLP/HTTP):

```yaml
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

---

## 5. Python ToolRecorder (In-Process Metrics)

For lightweight per-tool telemetry without a database:

```python
from dcc_mcp_core import ToolRecorder

recorder = ToolRecorder("maya")

# Context manager (recommended)
with recorder.start("create_sphere", "maya") as guard:
    result = cmds.polySphere(r=2.0)

# Query results
metrics = recorder.metrics("create_sphere")
if metrics:
    print(f"Calls: {metrics.invocation_count}")
    print(f"Success: {metrics.success_rate():.1%}")
    print(f"P95: {metrics.p95_duration_ms:.1f} ms")
```
