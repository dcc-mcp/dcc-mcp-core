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

## Non-goals

Routing-cache improvements for cancellation (#322) and HTTP/2
multiplexing tuning are out of scope for both #320 and #321.
