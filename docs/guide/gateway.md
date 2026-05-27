# Gateway

The gateway (`McpHttpConfig::gateway_port > 0`) is a first-wins HTTP
façade that presents every live DCC instance under one MCP endpoint.
A single client can talk to Maya, Blender and Houdini through the same
`/mcp` URL; the gateway discovers live backends via `FileRegistry`,
keeps its MCP `tools/list` bounded to four canonical workflow primitives,
indexes backend capabilities on demand, advertises MCP `search` /
`describe` / `load_skill` / `call`, routes REST `/v1/*` calls to the right backend,
and multiplexes server-pushed notifications back to the originating
client session.

Set `gateway_name`, `--gateway-name`, or `DCC_MCP_GATEWAY_NAME` on each
candidate to make ownership explicit. The elected process writes this label
to the `__gateway__` sentinel and `/admin/api/health.gateway.current`; a
challenger writes the same label with `gateway_role=challenger`, so operators
can see both the current owner and the next peer trying to take over.

For production, prefer the machine-wide standalone gateway:

```bash
dcc-mcp-server gateway --port 9765 --name studio-gateway
```

Per-DCC sidecars now auto-launch that process when `GET /health` is not
reachable. They use a single-flight `gateway-launch.lock` in the registry
directory so three DCCs starting at once still spawn at most one gateway.
Use `dcc-mcp-server sidecar --no-ensure-gateway` to disable auto-launch, or
`--legacy-gateway-election` to restore the old per-DCC first-wins election.

For the standalone `dcc-mcp-server` binary, the run mode is explicit:

```bash
dcc-mcp-server                  # implicit auto mode, backwards compatible
dcc-mcp-server auto --app maya  # explicit auto-gateway participation
dcc-mcp-server serve --app maya # per-DCC server, still auto-gateway capable
dcc-mcp-server serve --no-auto-gateway --app maya
dcc-mcp-server gateway --port 9765
```

Use `serve --no-auto-gateway` when an external daemon owns the shared gateway
port and this process should never try to bind it.

## Standalone gateway daemon (#1358)

The `dcc-mcp-server gateway` subcommand runs the gateway **as its own
process**, separate from any per-DCC server. It hosts only the gateway
plane — discovery, aggregation, routing, dynamic capabilities,
resources / prompts fan-out, the read-only admin UI, and audit — and
never executes a tool itself; every `tools/call` is HTTP-forwarded to
the owning DCC backend.

```bash
# Foreground, with a friendly owner label
dcc-mcp-server gateway --host 127.0.0.1 --port 9765 --name studio-gateway

# Bind a LAN listener as well so peers on the same subnet can join
dcc-mcp-server gateway --remote-host 0.0.0.0 --remote-port 59765
```

Common flags (all also accept the matching `DCC_MCP_*` environment
variable):

| Flag | Env var | Default |
|------|---------|---------|
| `--host` | `DCC_MCP_GATEWAY_HOST` | `127.0.0.1` |
| `--port` | `DCC_MCP_GATEWAY_PORT` | `9765` |
| `--name` | `DCC_MCP_GATEWAY_NAME` | `gateway-<host>-pid<n>` |
| `--remote-host` | `DCC_MCP_GATEWAY_REMOTE_HOST` | `0.0.0.0` |
| `--remote-port` | `DCC_MCP_GATEWAY_REMOTE_PORT` | `59765` (0 = disabled) |
| `--registry-dir` | `DCC_MCP_REGISTRY_DIR` | OS default |
| `--no-admin` | `DCC_MCP_NO_ADMIN` | admin enabled |
| `--admin-path` | `DCC_MCP_ADMIN_PATH` | `/admin` |
| `--stale-timeout-secs` | `DCC_MCP_STALE_TIMEOUT` | `30` |

Additional environment knobs:

- `DCC_MCP_GATEWAY_ADMIN_DB` — explicit path for the admin SQLite store
  (defaults to a workspace-anchored location).
- `DCC_MCP_GATEWAY_ADMIN_RETENTION_DAYS` — admin SQLite retention,
  clamped to `[1, 3650]`, default `30`.

### Daemon-mode guarantees

The standalone daemon path stamps the gateway with
`adapter_dcc = "gateway"` so peers can recognise it during election
tiebreaking (see `version.rs` — real DCCs preempt the generic
standalone). At runtime it satisfies the following:

- **No DCC tool execution.** `dcc-mcp-gateway` imports only the
  `EventBus` / `EventEnvelope` wire types from `dcc-mcp-actions`; it
  never owns a `ToolDispatcher` and never invokes a tool inline.
- **No PyO3 / Python host bridge.** `cargo tree -p dcc-mcp-gateway`
  contains zero of `pyo3`, `dcc-mcp-pybridge`, `dcc-mcp-host`,
  `dcc-mcp-sandbox`, or `dcc-mcp-capture`.
- **Runs without any DCC backend.** `GET /health` returns `200 OK`
  with an empty registry. Regression covered by
  `gateway_daemon::tests::standalone_daemon_serves_health_without_any_backend`
  in `crates/dcc-mcp-server/src/gateway_daemon.rs`.
- **Coexists with auto-gateway.** A DCC server built with
  `dcc-mcp-http` default features still elects itself when a daemon is
  absent (issue #1357 made the auto-gateway path a default-on cargo
  feature; turning it off lets the binary skip the gateway runtime
  entirely).

### When to use which mode

| Scenario | Recommended mode |
|----------|------------------|
| Single artist machine, one DCC | `dcc-mcp-server` or `dcc-mcp-server auto --app <dcc>` |
| Workstation hosting multiple DCCs | `auto` / `serve`; auto-gateway elects the first DCC to launch |
| Workstation with a separate gateway owner | `dcc-mcp-server serve --no-auto-gateway --app <dcc>` plus `dcc-mcp-server gateway` |
| Studio render node / shared host / CI | `dcc-mcp-server gateway` daemon, sidecars launch DCCs |
| Headless agent without any DCC installed | `dcc-mcp-server gateway` daemon — DCCs are reached via `FileRegistry` / HTTP registration |

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

Clients use this to re-query in-flight jobs via `jobs_get_status`.

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
   using `is_own_instance` from `crates/dcc-mcp-gateway/src/gateway/sentinel.rs`.
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

The gateway exposes the live DCC registry as a gateway-native MCP resource
(see also `docs/api/http.md`):

```json
{"jsonrpc":"2.0","id":1,"method":"resources/read",
 "params":{"uri":"gateway://instances"}}
```

The payload includes live, stale, and unhealthy rows so clients can decide
whether to route, reconnect, or ask the user to restart a DCC instance.
Each entry already carries `mcp_url`, so clients that have read this
resource can connect directly. Optional URI query parameters
(`?include_stale=false`, `?include_dead=true`) match the legacy tool
flags. `resources/list` advertises only root pointers for gateway-native
families; it does not enumerate every instance-specific URI. Backend
capability indexes refresh on demand before gateway `search` / `describe`,
so instances registered after gateway startup are picked up without a restart.

### Optional Instance Pooling

Instances can opt into warm-pool semantics through the registry fields surfaced
under `pool` in the `gateway://instances` resource:

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

Hidden gateway compatibility tools manage these leases; new integrations should
prefer registry resources plus REST orchestration around `/v1/call`:

| Tool | Purpose |
|------|---------|
| `lease action=acquire` / `acquire_dcc_instance` | Reserve an idle instance by `dcc_type` (or a specific `instance_id`) and mark it `busy` |
| `lease action=release` / `release_dcc_instance` | Release the lease and mark the instance `available` again |

Pooling is optional. Adapters that never call these tools keep the previous
single-instance behavior: entries default to `capacity: 1`, no lease owner, and
`status: "available"`.

Before the gateway routes REST traffic to a backend, it verifies that the target
responds to `GET /v1/readyz` and falls back to `GET /health` only when the
readiness surface is absent. This avoids treating non-MCP listeners such as Maya
`commandPort` as routable backends; posting MCP JSON-RPC to commandPort was a
pre-#818 failure mode that could trigger Maya's modal commandPort security
dialog and block the DCC main thread.

Gateway-native diagnostics are always available as MCP **resources**
(read via `resources/read`), even when no backend is routable:

| Resource URI | Purpose |
|------|---------|
| `gateway://diagnostics/process` | Gateway process metadata plus live/stale/unhealthy instance counts. Optional `?dcc_type=<type>` filter. |
| `gateway://diagnostics/audit` | Gateway pending-call and subscription summary |
| `gateway://diagnostics/metrics` | Gateway-local tool count, live backend count, and timeout settings |

Backend diagnostics tools remain available as normal prefixed instance tools
when a DCC exposes them.

## Operations: ingress limits, `X-Forwarded-For`, resilience, metrics

These knobs apply to the **elected gateway process** (the HTTP listener on
`McpHttpConfig::gateway_port`). They are read once at process start from the
environment unless noted otherwise.

### Rate limiting and client IP

| Variable | Default | Meaning |
|----------|---------|---------|
| `DCC_MCP_GATEWAY_RATE_LIMIT_PER_MINUTE` | `0` (off) | Max HTTP requests per **client key** per rolling UTC minute. `OPTIONS` is not counted. |
| `DCC_MCP_GATEWAY_XFF_TRUSTED_DEPTH` | `0` | When `> 0`, the client key for rate limiting prefers **`X-Forwarded-For`**: treat the **rightmost** `depth` comma-separated fields as trusted reverse-proxy hops; the next field to the **left** is the client IP. If the header is missing, malformed, or shorter than `depth + 1`, the TCP peer address is used. |

**Security:** only set `DCC_MCP_GATEWAY_XFF_TRUSTED_DEPTH` when every path to the
gateway passes through that many trusted proxies that **overwrite** (not
concatenate untrusted) `X-Forwarded-For`. A client that can reach the gateway
directly could otherwise spoof the header unless your edge strips or replaces
it.

### Request body size

| Variable | Default | Meaning |
|----------|---------|---------|
| `DCC_MCP_GATEWAY_HTTP_BODY_LIMIT_BYTES` | `16777216` (16 MiB) | Hard cap on non-streaming request bodies (`tower_http::limit::RequestBodyLimitLayer`). Long-lived **`GET /mcp` SSE** streams are not subject to a short global HTTP timeout. |

### Backend retries and circuit breaker

| Variable | Default | Meaning |
|----------|---------|---------|
| `DCC_MCP_GATEWAY_READ_RETRY_MAX` | `2` | Extra attempts for **idempotent read** REST hops (`GET` and read-like `POST /v1/search`) after transport / 5xx / 429 failures, with jittered backoff. **Writes** (`POST /v1/call`, JSON-RPC `post`) are not retried. |
| `DCC_MCP_GATEWAY_CIRCUIT_FAILURE_THRESHOLD` | `5` | Consecutive transport-class failures per backend REST base before the circuit opens. |
| `DCC_MCP_GATEWAY_CIRCUIT_OPEN_SECS` | `30` | How long to short-circuit new calls to that backend base. |

### Durable admin audit / trace JSONL (optional)

When `DCC_MCP_GATEWAY_AUDIT_DIR` is set, audit rows and dispatch traces append to
JSONL files under that directory.

| Variable | Default | Meaning |
|----------|---------|---------|
| `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` | `5000` | Trim oldest lines when a file exceeds this row count. |
| `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES` | `52428800` (~50 MiB) | After row trim, drop oldest lines until each JSONL is under this size. |

### Prometheus (`GET /metrics`)

Build **`dcc-mcp-http`** / **`dcc-mcp-gateway`** with the `prometheus` Cargo
feature and expose `GET /metrics` on the gateway listener (see
`attach_gateway_metrics_route` in `crates/dcc-mcp-gateway`).

Gateway → backend hop failures increment:

**`dcc_mcp_gateway_backend_errors_total{kind="…"}`**

`kind` is a **small fixed vocabulary** (low cardinality). Typical values:

| `kind` | When |
|--------|------|
| `transport` | TCP/TLS/DNS errors, timeouts on send |
| `unreachable` | Readiness probe could not reach the backend |
| `booting` | `/v1/readyz` reports not ready |
| `http_4xx` / `http_5xx` / `http_other` | Non-success HTTP from the backend REST hop |
| `read_body` | Failed to read the HTTP response body |
| `invalid_json` | Response was not valid JSON where expected |
| `jsonrpc_backend` | JSON-RPC `error` object from the backend |
| `empty_result` | JSON-RPC success without `result` |
| `circuit_open` | Local circuit breaker is open for that backend base |
| `other` | REST string errors that do not match the patterns above |

Other series on the same registry (instance gauges, request histograms, etc.)
are documented in `crates/dcc-mcp-telemetry/src/prometheus.rs`.

### Admin dashboard

`GET /admin/api/health` includes `rss_bytes`, `limits` (echoing the env-backed
values above, including `xff_trusted_depth`), and a `circuits` snapshot
(`tracked_backends`, `circuits_open`).

## Dynamic Capability Index and Bounded Tool Exposure (#652-#657)

For large multi-DCC deployments, the gateway **never** publishes every backend
action directly through `tools/list`. The removed `GatewayToolExposure` enum,
`McpHttpConfig.gateway_tool_exposure`, `publishes_backend_tools`, and
`--gateway-tool-exposure` switch are pre-0.15 concepts. There is now one
unconditional surface:

| Surface | What appears in `tools/list` | Agent workflow |
|---------|------------------------------|----------------|
| Gateway MCP | Fixed workflow primitives: `search`, `describe`, `load_skill`, `call`. Instance registry, diagnostics, catalog, and the **agent workflow guide** are gateway-native resources (`gateway://instances`, `gateway://diagnostics/*`, `gateway://catalog`, `gateway://docs/agent-workflows`) read via `resources/read`, not tools | `resources/read uri=gateway://instances` (or skip it and go straight to `search` → `describe`), optional `load_skill` from `next_step.arguments`, then `call` with one `tool_slug` or an ordered `calls` batch. Optional: `resources/read uri=gateway://docs/agent-workflows` for MCP+resources+efficiency guidance |
| Gateway REST | `/v1/search`, `/v1/load_skill`, `/v1/unload_skill`, `/v1/describe`, `/v1/call`, `/v1/call_batch`, `/v1/instances`, plus `/v1/resources*`, `/v1/prompts*`, and `/v1/jobs*` | `POST /v1/search` → optional `/v1/load_skill` from `next_step.arguments` → `/v1/describe` → `/v1/call` (or `POST /v1/call_batch` for ordered batches); use resources/prompts/jobs routes for non-tool MCP primitives |
| Direct per-DCC MCP | One DCC server's skills and loaded tools | `search_skills` → `load_skill` → tool call |

The gateway capability index stores compact records keyed by
`<dcc>.<id8>.<tool>` and refreshes on demand, so the first agent query after
startup or `load_skill` sees fresh results without a polling delay. The fixed
MCP workflow tools are cursor-safe and stable; hidden compatibility wrappers
remain callable for pinned clients but are no longer advertised:

| Tool | Purpose |
|------|---------|
| `search` | Search compact capability records by query, DCC type, tags, instance, scene hint, and pagination options; `kind=skill` searches skills |
| `describe` | Fetch the full schema, annotations, and routing record for a selected `tool_slug`, or skill detail for `skill_name` |
| `load_skill` | Load a discovered skill or activate/deactivate one progressive tool group on a target backend |
| `call` | Invoke one `tool_slug` or run an ordered `{calls:[...]}` batch, using the same max-25 guardrail as `/v1/call_batch` |

Use this four-tool dynamic-capability flow whenever an agent is connected to the gateway.
Use the per-DCC Skills-First flow (`search_skills` → `load_skill` → tool call)
when the agent is connected directly to one DCC server.

For REST-only clients, `POST /v1/search` with `loaded_only=false` returns
unloaded hits with `load_state`, `available_groups` when the backend knows
them, and a machine-executable `next_step`. POST that `next_step.arguments`
object to `/v1/load_skill`, or call MCP `load_skill` with the `next_step.mcp`
arguments, then search or describe again. Gateway `load_skill` defaults to
lazy group activation (`activate_groups=false` unless supplied), so only
default-active/core groups become active automatically; use an explicit
`tool_group` activation for heavier groups.

### Gateway call wrapper payloads

Gateway MCP `call`, hidden MCP compatibility routes (`call_tool`, `call_tools`),
and REST `POST /v1/call` / `POST /v1/call_batch` all share the same wrapper contract:

```json
{
  "tool_slug": "maya.a1b2c3d4.maya_scripting__execute_python",
  "arguments": { "code": "cmds.polySphere()" },
  "meta": { "progressToken": "session-42" }
}
```

Only `tool_slug`, `arguments`, and `meta` belong at the wrapper top level.
Backend-specific fields (`code`, `script`, `file_path`, `radius`, …) must be
inside `arguments`. Missing / `null` / empty-string arguments normalize to `{}`;
object roots pass through; object-shaped JSON strings are accepted for connector
compatibility; arrays, numbers, booleans, and non-object strings are rejected by
`dcc-mcp-wire`. Host adapters and connectors should reuse
`dcc_mcp_core.host.normalize_tool_arguments()` / `normalize_tool_meta()` instead
of each reimplementing coercion.

## Resources and Prompts Aggregation (#731, #732, #818)

The gateway also forwards MCP resources and prompts so agents can exchange
hand-off artefacts and prompt templates across all live DCC instances without
opening per-backend sessions. Since #818 the gateway's backend hop is REST, not
backend JSON-RPC: `GET /v1/resources`, `GET /v1/resources/{uri}`,
`GET /v1/prompts`, and `GET /v1/prompts/{name}?args=<json>`.

**Resources workflow:**

1. Call `resources/list` on the gateway.
2. Treat every returned URI as opaque. Gateway-native resources use
   `gateway://instances`, `gateway://diagnostics/*`, and `gateway://catalog`.
   Forwarded backend resources use a gateway-routable prefix so reads and
   subscriptions can find the owning backend.
3. `resources/list` only emits root pointers for gateway-native families; it
   does not enumerate every `gateway://instances/{id}` or
   `gateway://catalog/{name}`. Read those single-entry URIs directly when you
   already know the id/name.
4. Pass the exact URI returned by `resources/list` to `resources/read`,
   `resources/subscribe`, or `resources/unsubscribe`. Do not strip the
   instance prefix or rebuild URIs manually.

**Prompts workflow:** use `prompts/list` on the gateway to browse prompt
templates from all live backends, then call `prompts/get` with the returned
namespaced prompt name. Any MCP `arguments` object is forwarded through the REST
`args` query parameter and rendered by the backend prompt provider. Backend
prompt changes are surfaced through `notifications/prompts/list_changed`.

## Code pointers

| Piece | File |
|-------|------|
| Subscriber manager, reconnect loop | `crates/dcc-mcp-gateway/src/gateway/sse_subscriber.rs` |
| Per-session SSE plumbing | `crates/dcc-mcp-gateway/src/gateway/handlers/` (`handle_gateway_get`) |
| `tools/call` correlation hooks | `crates/dcc-mcp-gateway/src/gateway/aggregator.rs` / `aggregator/` |
| Subscription watcher and runtime tasks | `crates/dcc-mcp-gateway/src/gateway/tasks.rs` |

## Waiting for terminal results from the gateway (#321)

The gateway applies two separate request budgets to an outbound
`tools/call`:

| Case | Timeout | Source |
|------|---------|--------|
| Sync call (no `_meta.dcc.async`, no `progressToken`) | `backend_timeout_ms` (default 120 s) | `McpHttpConfig` |
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
`jobs_get_status` to collect the eventual result.

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
