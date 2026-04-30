# Gateway

The gateway (`McpHttpConfig::gateway_port > 0`) is a first-wins HTTP
façade that presents every live DCC instance under one MCP endpoint.
A single client can talk to Maya, Blender and Houdini through the same
`/mcp` URL; the gateway discovers live backends via `FileRegistry`,
aggregates their `tools/list`, routes each `tools/call` to the right
backend, and multiplexes server-pushed notifications back to the
originating client session.

## Topology

```
              ┌──────────────── gateway ────────────────┐
  client_A ──▶│  POST /mcp  (tools/list, tools/call)    │───▶ backend (maya)
              │  GET  /mcp  (SSE — MCP 2025-03-26)      │───▶ backend (blender)
  client_B ──▶│  subscribers: per-client broadcast sink │
              │  backend SSE sub: one per backend URL   │
              └────────────────────────────────────────┘
```

## SSE multiplex (#320)

When the gateway detects a new backend it opens a persistent SSE
connection to `<backend>/mcp` (the same Streamable HTTP transport the
client uses against the gateway). Notifications emitted by the backend
are parsed as JSON-RPC messages and routed to the right client:

| MCP method | Correlation key | Source |
|------------|-----------------|--------|
| `notifications/progress` | `params.progressToken` | Set by the gateway when the outbound `tools/call` carried `_meta.progressToken` |
| `notifications/$/dcc.jobUpdated` | `params.job_id` | Set from the backend reply's `_meta.dcc.jobId` / `structuredContent.job_id` |
| `notifications/$/dcc.workflowUpdated` | `params.job_id` | Same as above |

### Pending buffer

Notifications that arrive before the correlation is known (race
between backend SSE push and the `tools/call` HTTP reply) are held in
a bounded per-backend queue: **256 events** or **30 s**, whichever
comes first. When the mapping appears the buffer is drained; stale
entries are dropped with a `warn!` log.

### Reconnect + synthetic `$/dcc.gatewayReconnect`

Each backend subscriber owns a reconnect loop with jittered
exponential backoff (100 ms → 10 s, ±25% jitter). When a broken
stream reconnects, the gateway emits a synthetic
`notifications/$/dcc.gatewayReconnect` notification to every client
that had an in-flight job on that backend:

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/$/dcc.gatewayReconnect",
  "params": { "backend_url": "http://127.0.0.1:18812/mcp" }
}
```

Clients use this to re-query in-flight jobs via `jobs.get_status`.

### Session lifecycle

Per-client SSE sinks are keyed on `Mcp-Session-Id`. A `SessionCleanup`
RAII guard runs when the `GET /mcp` response body is dropped (client
disconnect): the client's sink is removed from the subscriber manager
and any `job_routes` / `progress_token_routes` bound to that session
are scrubbed. Backend subscriptions stay alive — another client might
still depend on them.

### Self-loop guard + pre-subscribe hygiene (#419)

When a DCC process (Maya, Blender, Houdini…) wins gateway election it
keeps *two* rows in `FileRegistry`: the `__gateway__` sentinel **and**
its own plain `"maya"` / `"blender"` / … row. Without filtering, the
backend SSE subscriber would open a connection to its own `/mcp`
endpoint — a self-loop that wastes a socket and floods the reconnect
logs whenever the facade blips.

Two invariants prevent this:

1. **Self-exclusion in every fan-out path.** `GatewayState::live_instances`
   skips rows whose `(host, port)` matches the gateway's own binding,
   using `is_own_instance` from `crates/dcc-mcp-http/src/gateway/sentinel.rs`.
   The helper normalises localhost aliases (`localhost`, `::1`,
   `0.0.0.0`, `[::]`) to `127.0.0.1` so an adapter that advertises its
   host as `"localhost"` is still filtered when the gateway is bound
   to `127.0.0.1`. The `backend_sub_handle` subscription loop and the
   `compute_tools_fingerprint_with_own` watcher apply the same filter.
2. **Synchronous hygiene before the subscriber loop starts.** Inside
   `start_gateway_tasks`, a one-shot `prune_dead_pids()` +
   `cleanup_stale()` pass runs **before** `backend_sub_handle` is
   spawned. The periodic cleanup task only ticks every 15 s; without
   the synchronous pre-pass, ghost rows left behind by a previous
   crash would eat the full exponential-backoff retry budget during
   the first ~15 s of gateway lifetime.

### Instance and Diagnostics Discovery

The gateway exposes instance health through both the MCP tool surface and a
native JSON-RPC method:

```json
{"jsonrpc":"2.0","id":1,"method":"instances/list","params":{"include_stale":true}}
```

The response matches `list_dcc_instances` and includes live, stale, and
unhealthy rows so clients can decide whether to route, reconnect, or ask the
user to restart a DCC instance. `tools/list` is assembled from the current
registry on each call, so instances registered after gateway startup are picked
up without a restart.

### Optional Instance Pooling

Instances can opt into warm-pool semantics through the registry fields surfaced
under `pool` in `list_dcc_instances` / `instances/list`:

```json
{
  "status": "busy",
  "pool": {
    "capacity": 1,
    "lease_owner": "workflow-42",
    "current_job_id": "render-001",
    "lease_expires_at": 1770000000,
    "available": false
  }
}
```

Gateway-local tools manage these leases:

| Tool | Purpose |
|------|---------|
| `acquire_dcc_instance` | Reserve an idle instance by `dcc_type` (or a specific `instance_id`) and mark it `busy` |
| `release_dcc_instance` | Release the lease and mark the instance `available` again |

Pooling is optional. Adapters that never call these tools keep the previous
single-instance behavior: entries default to `capacity: 1`, no lease owner, and
`status: "available"`.

Before the gateway sends JSON-RPC to a backend, it verifies that the target
responds to `GET /health`. This avoids treating non-MCP listeners such as Maya
`commandPort` as routable backends; posting MCP JSON-RPC to commandPort can
trigger Maya's modal commandPort security dialog and block the DCC main thread.

Gateway-native diagnostics tools are always present, even when no backend is
routable:

| Tool | Purpose |
|------|---------|
| `diagnostics__process_status` | Gateway process metadata plus live/stale/unhealthy instance counts |
| `diagnostics__audit_log` | Gateway pending-call and subscription summary |
| `diagnostics__tool_metrics` | Gateway-local tool count, live backend count, and timeout settings |

Backend diagnostics tools remain available as normal prefixed instance tools
when a DCC exposes them.

## Code pointers

| Piece | File |
|-------|------|
| Subscriber manager, reconnect loop | `crates/dcc-mcp-http/src/gateway/sse_subscriber.rs` |
| Per-session SSE plumbing | `crates/dcc-mcp-http/src/gateway/handlers.rs` (`handle_gateway_get`) |
| `tools/call` correlation hooks | `crates/dcc-mcp-http/src/gateway/aggregator.rs` (`route_tools_call`) |
| Subscription watcher | `crates/dcc-mcp-http/src/gateway/mod.rs` (`backend_sub_handle`) |

## Waiting for terminal results from the gateway (#321)

The gateway applies two separate request budgets to an outbound
`tools/call`:

| Case | Timeout | Source |
|------|---------|--------|
| Sync call (no `_meta.dcc.async`, no `progressToken`) | `backend_timeout_ms` (default 10 s) | `McpHttpConfig` |
| Async opt-in call (`_meta.dcc.async=true` or `_meta.progressToken`) | `gateway_async_dispatch_timeout_ms` (default 60 s) | `McpHttpConfig` |
| Async opt-in **with** `_meta.dcc.wait_for_terminal=true` | `gateway_wait_terminal_timeout_ms` (default 10 min) for the wait, `gateway_async_dispatch_timeout_ms` for the initial queuing step | `McpHttpConfig` |

**Why two timeouts?** An async-dispatched tool replies immediately with
`{status:"pending", job_id:"…"}` once the job has been queued on the
backend. Under cold-start conditions (Maya re-importing a heavy module,
Blender firing up a fresh Python interpreter) even that queuing step can
legitimately take >10 s, so the short sync timeout would surface a
spurious transport error while the backend is still starting the work.

### Response stitching (opt-in)

Clients that cannot consume SSE (plain `curl`, a batch script, a CI
runner) can still get the final result in a single `tools/call`
response by setting `_meta.dcc.wait_for_terminal = true` alongside
`_meta.dcc.async = true`:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "maya__bake_simulation",
    "arguments": {...},
    "_meta": {
      "dcc": {"async": true, "wait_for_terminal": true}
    }
  }
}
```

The gateway now:

1. Forwards the call to the backend with the longer
   `gateway_async_dispatch_timeout_ms` budget.
2. Receives the `{pending, job_id}` envelope and subscribes to the
   per-job broadcast bus owned by the SSE subscriber manager.
3. Blocks the HTTP response until a
   `notifications/$/dcc.jobUpdated` frame with `status in {completed,
   failed, cancelled, interrupted}` arrives over the backend's SSE
   stream, or until `gateway_wait_terminal_timeout_ms` elapses.
4. Merges the terminal status, `result`, and `error` into the original
   pending envelope's `structuredContent` and returns the resulting
   `CallToolResult`. `isError` is set for any non-`completed` status.

### Timeout semantics

If `gateway_wait_terminal_timeout_ms` elapses before a terminal event
arrives, the gateway returns the **last observed** job envelope
annotated with `_meta.dcc.timed_out = true` and leaves the job running
on the backend. Callers can either reconnect over SSE or keep polling
`jobs.get_status` to collect the eventual result.

### Backend disconnect

If the backend SSE stream drops while a waiter is blocked, the gateway
returns a JSON-RPC `-32000` error identifying the backend and the
`job_id`. The job itself is not cancelled — a subsequent restart of
the backend may surface it as `interrupted` (issue #328) when the
persisted job store rehydrates.

## Job-to-backend routing cache (#322)

To forward a `notifications/cancelled { requestId }` from the client to
the backend that actually owns the job, the gateway keeps a small cache:

```rust
pub struct JobRoute {
    pub client_session_id: ClientSessionId,
    pub backend_id: BackendId,            // e.g. http://127.0.0.1:8001/mcp
    pub tool: String,                     // for logs + cancel payload
    pub created_at: DateTime<Utc>,        // GC anchor
    pub parent_job_id: Option<String>,    // #318 cascade
}
// DashMap<Uuid, JobRoute>
```

Populated when the backend reply to a `tools/call` carries a `job_id`.
Consumed by:

- `notifications/cancelled { requestId }` — the gateway resolves
  `requestId → job_id → JobRoute` and POSTs a cancel to `backend_id`.
- Parent-job cascade — if the cancelled job has a `parent_job_id`, or
  *is itself* a parent, the gateway walks the `children_of` index and
  fans the cancel out to every distinct `backend_id` (which may differ
  from the originating backend — `#318` only covered single-server
  cascade, the gateway extends this across backends).

### Lifecycle

- **Insert** — `aggregator::route_tools_call` → `SubscriberManager::bind_job_route`.
- **Auto-evict** — `deliver()` removes the route as soon as a
  `$/dcc.jobUpdated` with a terminal status (`completed`, `failed`,
  `cancelled`, `interrupted`) is observed.
- **TTL GC** — a background task sweeps routes older than
  `gateway_route_ttl_secs` (default 24 h) every 60 s, so a backend
  crash that never emits a terminal event doesn't leak the route.
- **Per-session cap** — `gateway_max_routes_per_session` (default
  1 000). When a session is already holding `cap` live routes a new
  dispatch is rejected with JSON-RPC `-32005 too_many_in_flight_jobs`.

### Python configuration

```python
from dcc_mcp_core import McpHttpConfig

cfg = McpHttpConfig(
    port=0,
    gateway_route_ttl_secs=3600,              # 1 hour
    gateway_max_routes_per_session=500,
)
```

Both fields are also accessible as getters/setters on the returned
`McpHttpConfig` instance.

## Non-goals

HTTP/2 multiplexing tuning and multi-backend failover for the routing
cache (routes are sticky) are out of scope for #320 / #321 / #322.
