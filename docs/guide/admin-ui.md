# Built-in Admin Dashboard

The gateway ships an embedded `/admin` web dashboard (issue #772). At runtime it is a single HTML payload served from the binary; contributors edit the Vite/React source in `admin-ui/`, and `crates/dcc-mcp-gateway/build.rs` embeds the built asset during Cargo builds.

## Activation and Defaults

`/admin` is enabled by default on the elected gateway. This is intentional: the gateway and admin dashboard are part of the default local observability surface.

### `dcc-mcp-server` / `server.exe`

```bash
# Default: joins gateway election on :9765; elected process serves /admin
dcc-mcp-server --app maya

# Disable gateway entirely (also disables admin)
dcc-mcp-server --gateway-port 0

# Keep gateway but disable admin
dcc-mcp-server --no-admin

# Move admin under another prefix
dcc-mcp-server --admin-path /dcc-admin
```

Equivalent env vars:

| Env var | Default | Description |
|---------|---------|-------------|
| `DCC_MCP_GATEWAY_PORT` | `9765` | Gateway election port. `0` disables gateway/admin. |
| `DCC_MCP_NO_ADMIN` | `false` | Disable the read-only Admin UI on the elected gateway. |
| `DCC_MCP_ADMIN_PATH` | `/admin` | Admin URL prefix. |
| `DCC_MCP_GATEWAY_AUDIT_DIR` | unset | Optional JSONL directory for durable `audit.jsonl` and `traces.jsonl`; unset keeps zero-disk in-memory behavior. |
| `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` | `5000` | Max JSONL rows retained per durable file when persistence is enabled. |
| `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES` | `52428800` | Approx. 50 MiB byte cap per durable JSONL file; the gateway enforces both row and byte limits. |
| `DCC_MCP_LOG_DIR` | platform log dir | Directory scanned by `/admin/api/logs` for `*.log` files; defaults to `%USERPROFILE%\\AppData\\Local\\dcc-mcp\\log` on Windows and `~/.local/share/dcc-mcp/log` elsewhere. |

### Python API

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

cfg = McpHttpConfig(port=0, server_name="maya-mcp")
# Defaults for Python embedders:
# cfg.gateway_port == 9765
# cfg.admin_enabled is True
# cfg.admin_path == "/admin"

# Disable gateway/admin for an isolated local-only server:
cfg.gateway_port = 0

# Or keep gateway but hide admin:
cfg.admin_enabled = False

server = McpHttpServer(ToolRegistry(), cfg)
handle = server.start()
```

### Rust gateway API

```rust
use dcc_mcp_gateway::gateway::GatewayConfig;

let config = GatewayConfig {
    admin_enabled: true,          // default
    admin_path: "/admin".into(),  // default
    ..GatewayConfig::default()
};
```

When using `dcc-mcp-gateway` directly, compile with the `admin` Cargo feature. `dcc-mcp-http` and the shipped server binary enable this for their embedded gateway path.

## Routes

| Route | Content-Type | Description |
|-------|-------------|-------------|
| `GET /admin` | `text/html` | Embedded React/Vite dashboard served as one HTML asset |
| `GET /admin/api/activity?limit=300` | `application/json` | Unified activity timeline built from audits, traces, and gateway events |
| `GET /admin/api/instances` | `application/json` | Connected DCC instances |
| `GET /admin/api/tools` | `application/json` | Registered MCP tools |
| `GET /admin/api/tasks?limit=300` | `application/json` | Task-like snapshots reconstructed from dispatch traces |
| `GET /admin/api/calls` | `application/json` | Recent tool calls (requires `AuditMiddleware`) |
| `GET /admin/api/traces` | `application/json` | Recent per-call dispatch traces; accepts `?limit=200` |
| `GET /admin/api/traces/{request_id}` | `application/json` | Full waterfall for one recorded dispatch trace |
| `GET /admin/api/debug-bundle/{request_id}` | `application/json` | One-stop debug bundle containing the trace, matching audit row, related activity, and hints |
| `GET /admin/api/stats?range=1h\|24h\|7d` | `application/json` | Aggregated call counts, success rate, latency, and top tools/instances/agents |
| `GET /admin/api/workers` | `application/json` | Per-instance worker cards from the live registry |
| `GET /admin/api/logs` | `application/json` | Merged gateway contention events, on-disk `*.log` rows, and audited call summaries |
| `GET /admin/api/health` | `application/json` | Service health summary |

## Optional Agent / Caller Context

MCP and REST callers may attach optional context so the Admin UI can correlate
why a request was made with the request waterfall. This is a telemetry contract:
callers should send concise summaries, plans, observations, tags, and correlation
ids. The gateway does not attempt to capture hidden model chain-of-thought.

Supported carriers:

- MCP `tools/call` `params._meta.agent_context`
- REST body `agent_context`, `agentContext`, `caller_context`, or
  `meta.agent_context`
- Headers such as `x-dcc-mcp-agent-id`, `x-dcc-mcp-agent-name`,
  `x-dcc-mcp-agent-model`, `x-dcc-mcp-agent-task`,
  `x-dcc-mcp-reasoning-summary`, `x-dcc-mcp-parent-request-id`, and
  `x-dcc-mcp-agent-context` (JSON object)

Example REST request:

```json
{
  "tool_slug": "maya.abcdef01.scene__inspect",
  "arguments": { "include_materials": true },
  "meta": {
    "agent_context": {
      "agent_id": "agent-42",
      "agent_name": "Layout Inspector",
      "model": "gpt-5.4",
      "task": "Find the cheapest scene inspection path before editing",
      "reasoning_summary": "Need scene topology and material counts before selecting an edit tool.",
      "plan": ["inspect scene", "choose edit target"],
      "observations": ["user asked for non-destructive update"],
      "parent_request_id": "req-parent"
    }
  }
}
```

Admin list rows expose `transport`, `agent_id`, `agent_name`, `agent_model`,
span counts, payload byte counts, slowest span summaries, and a `links` object
with absolute URLs for the Admin trace page, trace API, and debug bundle. Full
trace rows include `agent_context`, request/response payload previews, a span
waterfall, and the same copyable links. These URLs are designed to be pasted
directly into an LLM evaluation prompt or another agent's debugging task.

## API Response Shapes

```json
// GET /admin/api/health
{
  "status": "ok",
  "uptime_secs": 3600,
  "instances_total": 3,
  "instances_ready": 2
}

// GET /admin/api/instances
{
  "total": 3,
  "instances": [
    { "id": "a1b2c3d4-...", "dcc_type": "maya", "status": "ready", "address": "127.0.0.1:9001" }
  ]
}

// GET /admin/api/activity?limit=300
{
  "total": 2,
  "events": [
    {
      "event_id": "audit:req-123",
      "timestamp": "2026-05-05T10:00:00Z",
      "kind": "tool_call",
      "severity": "info",
      "status": "ok",
      "message": "tools/call maya__open_scene",
      "tool": "maya__open_scene",
      "duration_ms": 48,
      "correlation": {
        "request_id": "req-123",
        "session_id": "session-1",
        "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
        "dcc_type": "maya"
      }
    }
  ]
}

// GET /admin/api/tasks?limit=300
{
  "total": 1,
  "tasks": [
    {
      "task_id": "req-123",
      "task_type": "tool_call",
      "status": "completed",
      "title": "maya__open_scene",
      "started_at": "2026-05-05T10:00:00Z",
      "duration_ms": 48,
      "correlation": {
        "request_id": "req-123",
        "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
        "dcc_type": "maya"
      }
    }
  ]
}

// GET /admin/api/calls  (requires AuditMiddleware)
{
  "total": 42,
  "calls": [
    {
      "request_id": "req-123",
      "method": "tools/call",
      "tool": "maya.abcdef01.maya__open_scene",
      "dcc_type": "maya",
      "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
      "session_id": "session-1",
      "transport": "mcp",
      "agent_id": "agent-42",
      "agent_name": "Layout Inspector",
      "agent_model": "gpt-5.4",
      "links": {
        "admin_trace_url": "http://127.0.0.1:9765/admin?panel=traces&trace=req-123",
        "trace_api_url": "http://127.0.0.1:9765/admin/api/traces/req-123",
        "debug_bundle_url": "http://127.0.0.1:9765/admin/api/debug-bundle/req-123"
      },
      "success": false,
      "error": "backend timeout",
      "timestamp": "2026-05-05T10:00:00Z"
    }
  ]
}

// GET /admin/api/traces?limit=200
{
  "total": 1,
  "traces": [
    {
      "request_id": "req-123",
      "tool": "maya.abcdef01.maya__open_scene",
      "dcc_type": "maya",
      "transport": "mcp",
      "agent_id": "agent-42",
      "span_count": 3,
      "slowest_span_name": "backend.execute",
      "slowest_span_ms": 45,
      "input_bytes": 42,
      "output_bytes": 96,
      "links": {
        "admin_trace_url": "http://127.0.0.1:9765/admin?panel=traces&trace=req-123",
        "trace_api_url": "http://127.0.0.1:9765/admin/api/traces/req-123",
        "debug_bundle_url": "http://127.0.0.1:9765/admin/api/debug-bundle/req-123"
      },
      "total_ms": 48,
      "success": true,
      "status": "ok"
    }
  ]
}

// GET /admin/api/traces/req-123
{
  "request_id": "req-123",
  "method": "tools/call",
  "tool_slug": "maya.abcdef01.maya__open_scene",
  "dcc_type": "maya",
  "transport": "mcp",
  "agent_context": {
    "agent_id": "agent-42",
    "agent_name": "Layout Inspector",
    "model": "gpt-5.4",
    "reasoning_summary": "Need scene topology before editing."
  },
  "links": {
    "admin_trace_url": "http://127.0.0.1:9765/admin?panel=traces&trace=req-123",
    "trace_api_url": "http://127.0.0.1:9765/admin/api/traces/req-123",
    "debug_bundle_url": "http://127.0.0.1:9765/admin/api/debug-bundle/req-123"
  },
  "total_ms": 48,
  "ok": true,
  "spans": [
    { "name": "backend.execute", "duration_ns": 45000000, "ok": true, "attributes": {} }
  ],
  "input": { "mime_type": "application/json", "truncated": false, "original_size": 42, "content": "{...}" },
  "output": { "mime_type": "application/json", "truncated": false, "original_size": 96, "content": "{...}" }
}

// GET /admin/api/debug-bundle/req-123
{
  "request_id": "req-123",
  "trace": { "request_id": "req-123", "spans": [] },
  "audit": { "request_id": "req-123", "success": true },
  "related_activity": [],
  "hints": []
}

// GET /admin/api/stats?range=24h
{
  "range": "24h",
  "total_calls": 42,
  "success_rate": 0.98,
  "latency_ms": { "p50_ms": 12, "p95_ms": 48 },
  "top_agents": [{ "name": "Layout Inspector", "count": 12 }]
}

// GET /admin/api/workers
{
  "summary": { "live": 2, "stale": 0, "unhealthy": 0 },
  "workers": [
    { "instance_id": "a1b2c3d4-...", "dcc_type": "maya", "status": "available" }
  ]
}

// GET /admin/api/logs
{
  "total": 5,
  "logs": [
    {
      "timestamp": "2026-05-05T09:59:00Z",
      "level": "info",
      "message": "tools/call ok 12ms — maya__open_scene",
      "source": "audit",
      "dcc_type": "maya",
      "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
      "request_id": "req-123",
      "tool": "maya__open_scene",
      "success": true,
      "detail": "instance=abcdef01-2345-6789-abcd-ef0123456789"
    }
  ]
}
```

## Connecting AuditMiddleware

For the `/admin/api/calls` feed to be populated, add `AuditMiddleware` to the middleware chain:

```rust
use dcc_mcp_gateway::gateway::middleware::{AuditMiddleware, MiddlewareChain};

GatewayConfig {
    admin_enabled: true,
    middleware_chain: MiddlewareChain::new()
        .with_before(Arc::new(AuditMiddleware::default())),
    ..GatewayConfig::default()
}
```

The `/admin/api/logs` feed is populated automatically from three bounded sources: the `EventLog` ring buffer (gateway election/eviction/probe events from issue #766), `*.log` files under `DCC_MCP_LOG_DIR` or the platform default log directory, and recent `AuditMiddleware` call rows. The `/admin/api/traces`, `/admin/api/stats`, and `/admin/api/workers` endpoints are populated from the dispatch `TraceLog`, `StatsAggregator`, and live gateway registry respectively.

Set `DCC_MCP_GATEWAY_AUDIT_DIR` to enable durable JSONL persistence. The gateway appends bounded admin call rows to `audit.jsonl` and dispatch traces to `traces.jsonl`, trims each file to both `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` and `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES`, and seeds the in-memory admin buffers from those files on restart. Payloads remain the same bounded/redacted `TracePayload` values used by the in-memory trace capture; persistence does not store unbounded raw request bodies.

## Dashboard Features

The HTML dashboard includes:
- **Debug Workbench**: the default first screen combines health, instances, calls, traces, stats, and warning logs so operators can triage gateway failures without jumping between panels.
- **Gateway owner identity**: the Health and Debug panels show the current `__gateway__` sentinel label from `gateway_name` / `DCC_MCP_GATEWAY_NAME`, plus any challenger candidates.
- **Left navigation**: Debug / Activity / Health / Instances / Tools / Tasks / Calls / Traces / Stats / Skill paths / Logs panels
- **Auto-refresh**: Panels poll their JSON endpoints every 5 seconds
- **DCC icons**: common hosts such as Maya/Autodesk, Blender, GIMP, Inkscape, Krita, Unity, and Unreal get recognizable icons, with a safe fallback for custom hosts.
- **Worker cards**: Per-instance status, heartbeat, and routing metadata
- **Calls table**: request ids, error previews, and trace-detail links; DCC is displayed from the resolved backend slug when available, otherwise from explicit call arguments such as `dcc` / `dcc_type`.
- **Trace drill-down**: `/admin/api/traces/{request_id}` exposes the full waterfall, optional agent/caller context, and bounded/redacted input/output payloads for one call.
- **Logs panel**: groups normalized `contention`, `file`, and `audit` rows so operators can correlate routing events, rolling files, and tool calls in one timeline. File log reads are bounded to recent files and tail slices so the admin API does not scan unbounded historical logs.
- **Durable audit option**: `DCC_MCP_GATEWAY_AUDIT_DIR` preserves the Calls and Traces panels across restarts without changing the JSON API shapes.
- **Dark theme**: Vite/React source with embedded runtime asset and no required runtime build step
- **Responsive**: narrow screens switch to a top navigation rail, and debug cards/charts keep a usable single-column width.

## Security Note

The admin UI is **read-only** and has **no authentication** by default. It binds to the same host as the elected gateway, which defaults to `127.0.0.1`. For production:
- Keep it bound to localhost, or place behind a reverse proxy with IP allowlist/basic auth
- Disable when not needed: `--no-admin`, `DCC_MCP_NO_ADMIN=true`, or `cfg.admin_enabled = False`
- Never expose directly to the public internet

## See also

- [middleware.md](middleware.md) — `AuditMiddleware` that feeds `/admin/api/calls`
- [observability.md](observability.md) — `EventLog` that feeds `/admin/api/logs`
- [gateway.md](gateway.md) — full gateway configuration reference
