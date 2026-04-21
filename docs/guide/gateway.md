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

## Non-goals

The SSE multiplexer does **not** forward non-notification response
bodies — that is tracked under issue #321. Routing-cache improvements
for cancellation (#322) and HTTP/2 multiplexing tuning are also out of
scope for #320.
