# Gateway

## Default topology: Runtime Supervisor → Central Gateway → Per-DCC Registration

The gateway follows a **three-layer architecture** that runs without
manual orchestration on a single workstation:

1. **Runtime Supervisor** — every per-DCC process (`dcc-mcp-server`,
   `dcc-mcp-server auto`, `dcc-mcp-server serve`, `dcc-mcp-server sidecar`,
   and Python `DccServerBase` adapters) acts as a daemon lifecycle manager.
   On startup it checks whether a machine-wide gateway daemon is already
   healthy; if not, it uses a **single-flight lock** (`gateway-launch.lock`)
   so that N concurrent DCC launches still spawn at most one gateway process.
   A **guardian watchdog** in each backend continuously probes `/health`
   (default every 5 s); after two consecutive misses it re-evaluates the
   daemon's liveness using the same single-flight lock and restarts the
   daemon when necessary.

2. **Central Gateway** — the single `dcc-mcp-server gateway` daemon that
   owns the well-known port (default `9765`). It hosts only the gateway
   plane: discovery, multi-source instance aggregation, bounded `tools/list`
   with four canonical workflow primitives, dynamic capability indexing,
   SSE multiplexing, REST routing, and the read-only admin UI. It never
   executes a tool inline — every `tools/call` is forwarded to the
   owning DCC backend.

3. **Per-DCC Registration** — each backend process stamps its
   `FileRegistry` row with `gateway_runtime_mode=daemon-backed`,
   `gateway_guardian_enabled=true`, and the
   `gateway_recovery_driver=daemon_guardian` annotation, so
   Admin and `gateway://instances` can show which registered services
   can revive the gateway and what fallback strategy is active.

The legacy first-wins election where a per-DCC process binds the gateway
port directly is still available as `--legacy-gateway-election`. It is the
fallback for environments where the binary was built without the
`gateway-daemon` feature.

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

Per-DCC servers and sidecars auto-launch that process when
`GET /health` is not reachable. They use a single-flight
`gateway-launch.lock` in the registry directory so three DCCs starting at
once still spawn at most one gateway. If the process that owns the launch
attempt crashes before releasing the file, any later DCC can reclaim a
stale lock after `DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS` seconds (default
`30`) and retry the daemon launch. Use `--no-ensure-gateway` to disable
auto-launch, or `--legacy-gateway-election` to restore the old per-DCC
first-wins election.

Python `DccServerBase` adapters, `dcc-mcp-server sidecar`,
`dcc-mcp-server` implicit/`auto`/`serve` backends, and registered
`dcc-mcp-server translate` bridges also keep a lightweight daemon guardian
running after startup in daemon-backed mode. If `/health` later becomes
unreachable for consecutive probes, the guardian reuses the same single-flight
launch lock and re-runs the standalone daemon ensure path, so any surviving
DCC or bridge instance can restore the shared gateway URL without blocking or
restarting the host process. Daemon-backed `dcc-mcp-server` backend and
translate rows stamp `gateway_runtime_mode=daemon-backed` plus
`gateway_guardian_enabled=true` into their FileRegistry metadata; opt-out and
legacy modes stamp a non-daemon mode with `gateway_guardian_enabled=false` so
Admin and `gateway://instances` can show which registered services can revive
the gateway.

If a routed backend call returns the terminal `host-died` envelope, the gateway
does not wait for ordinary heartbeat or stale cleanup. It records the event,
drops that instance's dynamic capabilities, and deregisters the matching
FileRegistry or HTTP registration row immediately so the next search/call
routes around the dead host.

For the standalone `dcc-mcp-server` binary, the run mode is explicit:

```bash
dcc-mcp-server                  # implicit auto mode; ensures daemon + registers backend
dcc-mcp-server auto --app maya  # explicit daemon-backed backend registration
dcc-mcp-server serve --app maya # per-DCC server; ensures daemon + registers backend
dcc-mcp-server serve --no-auto-gateway --app maya
dcc-mcp-server auto --legacy-gateway-election --app maya
dcc-mcp-server translate --stdio "uvx mcp-server-git" --app-type git
dcc-mcp-server gateway --port 9765
```

Use `serve --no-auto-gateway` or `translate --no-register` when an external
daemon owns the shared gateway port and this process should never try to bind
or register with it.

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

# Detached, with a pidfile that records the gateway child PID
dcc-mcp-server gateway --host 127.0.0.1 --port 9765 \
    --daemon --pidfile /var/run/dcc-mcp-gateway.pid

# Bind a LAN listener as well so peers on the same subnet can join
dcc-mcp-server gateway --remote-host 0.0.0.0 --remote-port 59765
```

`--daemon` re-executes the current binary as a detached gateway child and then
exits the parent; it does not fork inside the async runtime. `--pidfile`
implies daemon mode, records that detached child PID, and fails before the
parent exits if the child cannot be spawned or the pidfile cannot be written.

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
| `--daemon` | `DCC_MCP_DAEMON` | `false` |
| `--pidfile` | `DCC_MCP_PIDFILE` | none |
| `--gateway-persist` | `DCC_MCP_GATEWAY_PERSIST` | `false` |
| `--gateway-idle-timeout-secs` | `DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS` | `30` |
| `--discover-mdns` | `DCC_MCP_DISCOVER_MDNS` | `false` when built with `mdns` |
| `--relay-source ADMIN_URL=PUBLIC_BASE_URL` | `DCC_MCP_RELAY_SOURCES` | none |

Additional environment knobs:

- `DCC_MCP_GATEWAY_ADMIN_DB` — explicit path for the admin SQLite store
  (defaults to a workspace-anchored location).
- `DCC_MCP_GATEWAY_ADMIN_RETENTION_DAYS` — admin SQLite retention,
  clamped to `[1, 3650]`, default `30`.
- `DCC_MCP_GATEWAY_GUARDIAN_INTERVAL` — seconds between post-startup
  daemon guardian probes, default `5`.
- `DCC_MCP_GATEWAY_GUARDIAN_TIMEOUT` — per-probe `/health` timeout in
  seconds, default `0.5`.
- `DCC_MCP_GATEWAY_GUARDIAN_FAILURES` — consecutive failed probes before
  a Python adapter or Rust sidecar re-runs daemon ensure, default `2`.
- `DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS` — age after which a leftover
  `gateway-launch.lock` is reclaimed by a later DCC instance, default
  `30`.
- `DCC_MCP_GATEWAY_PERSIST` — keep the daemon alive when no backends
  remain. Default `false`; set to `1` for studio/headless deployments
  where backends start and stop independently.
- `DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS` — grace period in seconds before
  the daemon shuts down after the last routable backend exits. The manual
  `dcc-mcp-server gateway` CLI default is `30`; daemon auto-ensure passes
  `300` by default to cover slow backend startup registration. `0`
  disables the timer (same as `PERSIST=1`).

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
  in `crates/dcc-mcp-sidecar/src/gateway_daemon.rs`.
- **Coexists with auto-gateway.** A DCC server built with
  `dcc-mcp-http` default features still elects itself when a daemon is
  absent (issue #1357 made the auto-gateway path a default-on cargo
  feature; turning it off lets the binary skip the gateway runtime
  entirely).

### When to use which mode

| Scenario | Recommended mode |
|----------|------------------|
| Single artist machine, one DCC | `dcc-mcp-server` or `dcc-mcp-server auto --app <dcc>`; the server ensures a local gateway daemon and registers as a backend |
| Workstation hosting multiple DCCs | `auto` / `serve`; each backend ensures the same daemon and then registers |
| Workstation with a manually managed gateway owner | `dcc-mcp-server serve --no-ensure-gateway --app <dcc>` plus `dcc-mcp-server gateway` |
| Studio render node / shared host / CI | `dcc-mcp-server gateway` daemon, sidecars launch DCCs |
| Headless agent without any DCC installed | `dcc-mcp-server gateway` daemon — DCCs are reached via `FileRegistry`, HTTP registration, mDNS, or relay sources |
| Legacy per-DCC first-wins election | `dcc-mcp-server auto --legacy-gateway-election --app <dcc>` — the first DCC to bind the gateway port becomes the gateway |

### Runtime layers

| Layer | Component | Lifecycle |
|-------|-----------|-----------|
| **Runtime Supervisor** | `spawn_gateway_guardian()` in every daemon-backed backend | Probes `/health` every 5 s; re-uses single-flight lock to restart daemon after 2 consecutive misses |
| **Central Gateway** | `dcc-mcp-server gateway` daemon process | Auto-launched by the supervisor; survives backend restarts; idle-timeout after the last routable backend exits (manual CLI default 30 s; daemon auto-ensure default 300 s), unless `DCC_MCP_GATEWAY_PERSIST=1` |
| **Per-DCC Registration** | `FileRegistry` row + 5 s heartbeat | Each backend stamps `gateway_runtime_mode`, `gateway_guardian_enabled`, `gateway_recovery_driver` into its row so admin tools can answer "which services can revive the gateway?" |

### Python daemon helpers (PIP-513)

The `dcc_mcp_core.daemon_launch` module and the extended `ensure_gateway_daemon`
API provide three tiers of daemon control from Python, usable by any adapter or
studio pipeline service.

| Mode | API | Typical use |
|------|-----|-------------|
| **Gateway ensure** | `ensure_gateway_daemon(gateway_persist=True, ...)` | DCC adapter startup auto-ensures a machine-wide gateway daemon with a 300 s idle grace by default; single-flight lock prevents duplicate spawns |
| **Gateway launch** | `launch_gateway_daemon(gateway_host=..., ...)` | Alias for `ensure_gateway_daemon` with explicit daemon naming |
| **Arbitrary command detach** | `launch_detached(["my-svc", "--flag"])` | Studio-owned pipeline adapters, sidecars, custom MCP hosts |

```python
from dcc_mcp_core import ensure_gateway_daemon, launch_detached

# Gateway auto-ensure with persist/idle-timeout flags
result = ensure_gateway_daemon(
    gateway_host="127.0.0.1",
    gateway_port=9765,
    registry_dir=None,
    dcc_type="ftrack",
    gateway_persist=True,
    gateway_idle_timeout_secs=0,
)
assert result["ok"]

# Detach an arbitrary pipeline service without blocking
spawn = launch_detached(["python", "-m", "my_pipeline.sidecar"])
assert spawn["ok"]
print(f"Spawned PID: {spawn['pid']}")
```

The `build_gateway_daemon_command()` function is also exported for inspection:

```python
from dcc_mcp_core import build_gateway_daemon_command

cmd, env = build_gateway_daemon_command(
    gateway_host="127.0.0.1",
    gateway_port=9765,
    registry_dir="/tmp/reg",
    dcc_type="maya",
    gateway_persist=True,
)
# cmd == ["dcc-mcp-server", "gateway", "--host", "127.0.0.1",
#         "--port", "9765", "--gateway-persist"]
# env["DCC_MCP_GATEWAY_PERSIST"] == "1"
```

## Topology

```
┌─── Runtime Supervisor (per backend) ──────────────────────────────────────────┐
│  Maya sidecar        Blender sidecar        Houdini sidecar                   │
│  ┌─────────────┐     ┌─────────────┐        ┌─────────────┐                  │
│  │ guardian     │     │ guardian    │        │ guardian    │                  │
│  │ watchdog     │     │ watchdog    │        │ watchdog    │                  │
│  │ /health poll │     │ /health poll│        │ /health poll│                  │
│  └──────┬──────┘     └──────┬──────┘        └──────┬──────┘                  │
│         │  single-flight   │                      │                          │
│         └──────┬───────────┴──────────────────────┘                          │
│                │  gateway-launch.lock                                         │
│                ▼                                                              │
├─── Central Gateway (machine-wide daemon) ─────────────────────────────────────┤
│  dcc-mcp-server gateway --port 9765                                          │
│  ┌──────────────────────────────────────────────────────────────┐            │
│  │  POST /mcp  (tools/list, tools/call)                         │            │
│  │  GET  /mcp  (SSE — MCP 2025-03-26)                           │            │
│  │  GET  /admin (read-only dashboard)                            │            │
│  │  GET  /v1/readyz (readiness probe + lifecycle diagnostics)    │            │
│  │  backend SSE sub: one per backend URL                         │            │
│  └────────┬──────────┬──────────┬───────────────────────────────┘            │
│           │          │          │                                              │
├─── Per-DCC Backend Registration ──────────────────────────────────────────────┤
│  Maya @ :18812      Blender @ :18813      Houdini @ :18814                    │
│  dcc_type: maya     dcc_type: blender    dcc_type: houdini                    │
│  gateway_runtime_mode: daemon-backed                                          │
│  gateway_guardian_enabled: true                                               │
│  gateway_recovery_driver: daemon_guardian                                     │
└───────────────────────────────────────────────────────────────────────────────┘

  client_A ──▶│  talks to Maya through the same /mcp URL                      │
  client_B ──▶│  SSE subscribers: per-client broadcast sink                   │
```

**Key invariants:**

- Guardians from multiple DCC types share one `gateway-launch.lock` — at most one daemon spawn.
- The daemon hosts the gateway plane only; DCC backends handle tool execution.
- If the daemon crashes, any surviving guardian restarts it within ~10-15 s (two probe misses + re-ensure time).
- When all routable backends exit, the daemon shuts down after `DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS` (manual CLI default 30 s; daemon auto-ensure default 300 s), unless `DCC_MCP_GATEWAY_PERSIST=1`.

## Topology recipes (issue #1366)

Four named recipes cover the supported deployment shapes. Each is a
complete, copy-pasteable command set; pick the one that matches your
constraints and read the migration guide for the path between them
([`docs/guide/migration/from-embedded-to-daemon.md`](migration/from-embedded-to-daemon.md)).

### Recipe 1 — Single workstation (daemon-backed auto-gateway)

Default zero-config flow. The first DCC ensures the machine-wide gateway
daemon is running; every DCC, including that first one, registers as a
backend behind the same gateway URL.

```bash
# Maya plugin host:
dcc-mcp-server --app maya

# Same workstation, second DCC — registers behind the same gateway daemon:
dcc-mcp-server --app blender
```

`tools/list` against `http://127.0.0.1:9765/mcp` exposes the gateway's
bounded set of discovery primitives; routing fans them out across both
DCCs.

### Recipe 2 — Multi-workstation LAN with a daemon gateway + HTTP registration

The daemon owns the gateway port on a chosen host; every DCC adapter on
every workstation registers via the HTTP API (#1361).

```bash
# Host A — runs the gateway only:
dcc-mcp-server gateway --host 0.0.0.0 --port 9765 --registry-dir /var/lib/dcc-mcp

# Host B — runs a DCC, never bids for the gateway port:
dcc-mcp-server serve --no-auto-gateway --app maya \
    --register-url http://host-a.lan:9765/v1/instances/register \
    --heartbeat-secs 5

# Host C — different DCC, same registration target:
dcc-mcp-server serve --no-auto-gateway --app photoshop \
    --register-url http://host-a.lan:9765/v1/instances/register
```

`gateway://instances` on Host A lists both Hosts B and C with
`source: "http"`.

### Recipe 3 — LAN with mDNS auto-discovery

Use when configuring a `--register-url` per host is unwieldy. Build with
`--features mdns`; the gateway browses `_dcc-mcp._tcp.local`, probes
each discovered endpoint, and surfaces the survivors as `source: "mdns"`
(#1362).

```bash
# Host A — daemon listens for advertised DCC sidecars on the LAN:
dcc-mcp-server gateway --host 0.0.0.0 --port 9765 --discover-mdns

# Host B (or C, D, …) — each DCC sidecar advertises itself:
dcc-mcp-server serve --no-auto-gateway --app blender --advertise-mdns

dcc-mcp-server serve --no-auto-gateway --app houdini --advertise-mdns
```

Security stance: mDNS is for *address discovery only*. Every discovered
endpoint must still pass the gateway's auth chain before any call is
routed through it.

### Recipe 4 — Internet-facing topology via tunnel relay

The DCC sits behind NAT / firewall. A `dcc-mcp-tunnel-agent` opens a WSS
back-channel to a publicly reachable `dcc-mcp-tunnel-relay`; the gateway
polls the relay's admin API and surfaces healthy tunnels as
`source: "relay"` (#1363).

```bash
# Public relay (e.g. fly.io, k8s ingress, …):
dcc-mcp-tunnel-relay \
    --agent-bind 0.0.0.0:9090 \
    --frontend-bind 0.0.0.0:9091 \
    --admin-bind 127.0.0.1:9092

# DCC host behind NAT:
dcc-mcp-tunnel-agent \
    --relay-url wss://relay.example.com:9090 \
    --jwt $TUNNEL_JWT \
    --dcc photoshop \
    --local-target http://127.0.0.1:8765/mcp

# Gateway pointing at the relay's admin endpoint:
dcc-mcp-server gateway --host 0.0.0.0 --port 9765 \
    --relay-source http://relay.example.com:9092=https://relay.example.com:9091
```

Auth contract: the agent leg uses the tunnel JWT; the gateway leg uses
its own auth chain. Both must pass before a call is routed end-to-end.

## Discovery Topology

All discovery sources collapse into the same `gateway://instances` and
`GET /v1/instances` row shape. Existing clients can keep reading
`instances[*].mcp_url`; newer agents should also inspect `source`,
`source_meta`, and the top-level `by_source` counts.

```text
Local workstation
  DCC sidecar -> FileRegistry services.json -> gateway source: "file"

Routed HTTP backend
  DCC sidecar -> POST /v1/instances/register -> gateway source: "http"

Same LAN
  DCC sidecar --mDNS/DNS-SD--> gateway --health probe--> source: "mdns"

NAT / cross-subnet
  DCC sidecar -> tunnel agent -> relay /tunnels
  gateway --relay-source ADMIN=PUBLIC--> /tunnel/<id>/mcp -> source: "relay"
```

Conflict resolution is by `instance_id`, with this precedence:

```text
http > relay > mdns > file
```

The resource payload is additive:

```json
{
  "total": 2,
  "by_source": {"file": 1, "http": 0, "mdns": 0, "relay": 1},
  "instances": [
    {
      "instance_id": "11111111-1111-4111-8111-111111111111",
      "instance_short": "11111111",
      "dcc_type": "maya",
      "mcp_url": "http://127.0.0.1:8765/mcp",
      "source": "file",
      "source_meta": {}
    },
    {
      "instance_id": "22222222-2222-4222-8222-222222222222",
      "instance_short": "22222222",
      "dcc_type": "houdini",
      "mcp_url": "https://relay.example/tunnel/tun1/mcp",
      "source": "relay",
      "source_meta": {"tunnel_id": "tun1"}
    }
  ]
}
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

Rows also include a normalized `dispatch` object. For sidecars this tells
clients whether dispatch readiness has been reported (`reported`), whether the
backend is callable (`ready`), the current `status` (`ready`, `unavailable`, or
`not_reported`), and any host-RPC failure metadata. This keeps "registered" and
"callable" separate without requiring clients to parse raw metadata.

### Instance Row Metadata Fields

Every instance row in `gateway://instances` and `GET /v1/instances` carries
three structured sub-objects: `gateway`, `dispatch`, and `lifecycle`. These
replace ad-hoc raw metadata parsing with typed, stable fields.

#### `gateway` object

Describes the instance's relationship to the gateway daemon lifecycle.

| Field | Type | Description |
|-------|------|-------------|
| `runtime_mode` | `string \| null` | Gateway runtime mode: `"daemon-backed"`, `"embedded-fallback"`, or `null` (not registered) |
| `guardian_enabled` | `bool` | Whether the instance runs a post-startup daemon guardian |
| `recovery_driver` | `string` | How this instance can recover the gateway: `"daemon_guardian"`, `"embedded_election"`, or `"none"` |
| `registration_refresh_mode` | `string` | How the instance row is kept alive: `"file_registry_heartbeat"`, `"http_ttl_heartbeat"`, `"relay_poll"`, or `"mdns_discovery"` |

`recovery_driver="daemon_guardian"` means the instance runs periodic `/health`
probes against the gateway daemon and can re-launch it. The `readyz` endpoint
exposes aggregate counts so launchers and admin panels can answer whether at
least one live DCC service can restart the shared daemon without scanning each
row.

#### `dispatch` object

Separates "the DCC process is registered" from "the sidecar dispatcher is
actually callable."

| Field | Type | Description |
|-------|------|-------------|
| `reported` | `bool` | Whether the sidecar has published dispatch status metadata |
| `status` | `string` | Current dispatch state: `"ready"`, `"unavailable"`, or `"not_reported"` |
| `ready` | `bool \| null` | `true` when dispatch is ready AND `mcp_url` is set AND the instance is not stale; `null` when dispatch status hasn't been reported |
| `ready_at_unix` | `int \| null` | Unix timestamp of last dispatch-ready transition |
| `host_rpc_uri` | `string \| null` | Canonical host-RPC URI (`commandport://...`, `qtserver://...`) |
| `host_rpc_scheme` | `string \| null` | Scheme portion of the host-RPC URI |
| `failure_stage` | `string \| null` | If not ready, which stage failed (`launch`, `probe`, `rpc_connect`) |
| `failure_reason` | `string \| null` | Human-readable reason for the failure |

For sidecar-driven adapters, use the dispatch counters in `GET /v1/readyz`
(`dispatch_reported_instance_count`, `dispatch_ready_instance_count`,
`dispatch_not_ready_instance_count`) to distinguish "listed in registry" from
"actually callable." A `ready: false` instance with `reported: true` and
`status: "unavailable"` is a sidecar that started but whose dispatcher isn't
ready yet — wait and re-probe, don't route calls to it.

#### `lifecycle` object

| Field | Type | Description |
|-------|------|-------------|
| `pid` | `int \| null` | OS process ID of the DCC or sidecar |
| `dcc_pid` | `int \| null` | OS process ID of the DCC host (when different from sidecar) |
| `session` | `string \| null` | DCC session identifier (e.g. `"untitled"`) |
| `sidecar_pid` | `int \| null` | OS process ID of the sidecar process |
| `supports_safe_stop` | `bool` | Whether the instance advertises a safe-stop endpoint |
| `restartable` | `bool` | Whether the instance can be restarted (has sidecar_pid or restart/launch command) |

### Remote HTTP Instance Registration (#1361)

When a DCC adapter cannot share the gateway's local `FileRegistry` directory
(for example, a DCC process on another machine), it can register a TTL-scoped
row through the gateway REST plane:

```bash
curl -X POST http://gateway-host:9765/v1/instances/register \
  -H "content-type: application/json" \
  -d '{
    "instance_id": "11111111-1111-4111-8111-111111111111",
    "dcc_type": "maya",
    "mcp_url": "http://dcc-host:18812/mcp",
    "capabilities_fingerprint": "optional-stable-fingerprint",
    "scene": "shot.ma",
    "ttl_secs": 30
  }'
```

The response includes `heartbeat_interval_secs`; adapters should refresh before
the TTL expires:

```bash
curl -X POST http://gateway-host:9765/v1/instances/heartbeat \
  -H "content-type: application/json" \
  -d '{"instance_id":"11111111-1111-4111-8111-111111111111"}'
```

Shutdown should call `POST /v1/instances/deregister`. HTTP rows are merged into
the same `GatewayState::live_instances` view as file-backed rows, appear in
`GET /v1/instances` / `gateway://instances` with `source: "http"`, and win when
the same `instance_id` exists in both sources. Registration endpoints pass
through the same gateway router layers as the rest of `/v1/*` (body limit,
caller attribution, rate limiting, and future auth middleware), so they do not
create a separate security bypass.

### Optional mDNS / DNS-SD LAN Discovery (#1362)

Build with the `mdns` Cargo feature to enable LAN-local DNS-SD advertisement
and browsing for `_dcc-mcp._tcp.local.`. It is intentionally opt-in and
default-off: mDNS is a convenience discovery hint, not an auth boundary.

```bash
# DCC endpoint advertises its MCP URL on the local subnet.
dcc-mcp-server serve --no-auto-gateway --app maya --advertise-mdns

# Gateway daemon browses mDNS records and probes candidates before surfacing them.
dcc-mcp-server gateway --discover-mdns --remote-host 0.0.0.0 --remote-port 59765
```

Discovered rows use `source: "mdns"` in `GET /v1/instances` and
`gateway://instances`. They carry the advertised `dcc`, `instance_id`,
`version`, `adapter`, `auth`, and `mcp_path` TXT metadata, but the gateway only
adds the row after the resolved endpoint answers the HTTP health probe. Rows
expire from an in-memory registry if their DNS-SD TTL passes or the service is
removed.

Conflict order is deliberate: HTTP registration wins over mDNS, and mDNS wins
over a stale or duplicate file-backed registry row with the same `instance_id`.
For routed production traffic across subnets, prefer explicit HTTP registration
or a relay/tunnel source; use mDNS for same-LAN discovery where multicast is
allowed and operationally acceptable.

### Optional Relay-Backed Discovery (#1363)

When DCC workstations sit behind NAT or cannot receive inbound traffic, run a
tunnel agent next to each local DCC HTTP MCP server and configure the gateway to
poll the relay admin endpoint:

```bash
# Relay admin URL on the left, relay HTTP frontend URL on the right.
dcc-mcp-server gateway \
  --relay-source http://relay.example.com:9003=http://relay.example.com:9002
```

`DCC_MCP_RELAY_SOURCES` accepts the same `ADMIN_URL=PUBLIC_BASE_URL` format and
may contain comma-separated entries. The gateway polls `GET /tunnels`, maps
each active tunnel to `<PUBLIC_BASE_URL>/tunnel/<tunnel_id>/mcp`, probes the
candidate through `GET /v1/healthz`, then exposes it in `GET /v1/instances` and
`gateway://instances` with `source: "relay"`.

Relay-backed rows preserve the agent-provided `instance_id`,
`capabilities_fingerprint`, `adapter_version`, and `scene` when present. If an
agent omits `instance_id`, the gateway derives a stable UUID from the relay
source and tunnel id, so a reconnecting tunnel still appears as a normal
instance row during its lifetime.

Source precedence across duplicate `instance_id` values is:

```text
HTTP registration > relay source > mDNS > FileRegistry
```

Use HTTP registration when a backend has a routable, authenticated URL. Use
relay sources when the gateway must route through the tunnel data plane. Use
mDNS only as a same-LAN discovery hint.

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

### Event webhooks

Set `DCC_MCP_WEBHOOKS_CONFIG` to a YAML file path to forward EventBus
envelopes (`tool.*`, `skill.*`, `trace.*`, `gateway.*`) to external HTTP
endpoints. Each webhook specifies:

- `name` — stable identifier for logs and delivery-failed events.
- `url` — `http` / `https` endpoint.
- `events` — event name patterns (e.g. `["tool.*", "trace.*"]`).
- `filters` — optional dotted-path matchers with `*`-wildcard support
  (e.g. `attributes.tool_slug: "maya.*"`).
- `delivery.attempts` — retry count (default `3`).
- `delivery.timeout_ms` — per-attempt timeout (default `2_000` ms).
- `backoff_ms` — per-retry delay sequence (default `[200, 1000, 5000]` ms).
- `payload_template` — optional template string using double-braced
  `source.dcc_type` / `attributes.*` paths. When omitted, the raw event
  envelope is POSTed as JSON.

Example `webhooks.yaml` to forward analytics events:

```yaml
queue_capacity: 256
webhooks:
  - name: analytics-forwarder
    url: https://internal.example.com/api/dcc-analytics
    events:
      - "tool.*"
      - "skill.*"
      - "trace.*"
      - "gateway.instance.*"
    headers:
      Authorization: "Bearer ${ANALYTICS_WEBHOOK_TOKEN}"
    delivery:
      attempts: 3
      timeout_ms: 5000
    filters:
      - name: "tool.completed"
      - name: "skill.loaded"
```

The webhook runtime starts automatically when the env var points at a
valid YAML file. Headers support `${ENV_VAR}` interpolation so tokens
stay out of version control.

### Sentry error monitoring (Rust backend)

Set `DCC_MCP_SENTRY_DSN` to your Sentry project DSN. The SDK initialises
at server startup and captures panics automatically. Use
`sentry::capture_error` or `sentry::capture_message` for explicit
instrumentation points.

| Variable | Default | Purpose |
|----------|---------|---------|
| `DCC_MCP_SENTRY_DSN` | (disabled) | Sentry project DSN |
| `DCC_MCP_SENTRY_ENVIRONMENT` | `production` | Environment tag |
| `DCC_MCP_SENTRY_RELEASE` | crate version | Release identifier for source-map / commit correlation |
| `DCC_MCP_SENTRY_SAMPLE_RATE` | `1.0` | Error event sample rate (0.0–1.0) |

The feature is compiled by default and skips initialisation entirely when
`DCC_MCP_SENTRY_DSN` is absent, so zero-config deployments pay no overhead.
Build with `--no-default-features` and opt out of `sentry` to exclude the
crate from the binary entirely.

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
arguments, then search or describe again. Gateway `load_skill` activates all
declared groups by default (`activate_groups=true` when omitted). Pass
`activate_groups=false` for lazy loading, or use `tool_group` to activate one
group explicitly.

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

## Security (issue #1365)

When the gateway runs as a standalone daemon (Recipe 2/3/4) it sits on a
boundary that the local-trust `FileRegistry` does not cover anymore.
This section is the operator-facing contract for authn / authz / TLS on
that boundary.

### Authentication: bearer tokens on the registration plane

The Rust API exposes
[`dcc_mcp_gateway::GatewayAuth`](https://docs.rs/dcc-mcp-gateway) and
[`dcc_mcp_gateway::GatewayAuthToken`](https://docs.rs/dcc-mcp-gateway).
Operators wire them into the gateway by populating
`GatewayConfig::auth` before passing the config to `GatewayRunner`:

```rust
use dcc_mcp_gateway::{GatewayAuth, GatewayAuthToken, GatewayConfig};

let auth = GatewayAuth {
    tokens: vec![
        // One master token that may register any DCC family.
        GatewayAuthToken::any_dcc(std::env::var("DCC_MCP_GATEWAY_MASTER")?),
        // A studio-scoped token confined to Maya + Blender only.
        GatewayAuthToken::for_dcc(
            std::env::var("DCC_MCP_GATEWAY_STUDIO")?,
            ["maya", "blender"],
        ),
    ],
};
let cfg = GatewayConfig { auth, ..GatewayConfig::default() };
```

By default `GatewayAuth::disabled()` is used, which preserves the
historical zero-auth behaviour and remains the safe default for the
single-workstation daemon-backed auto-gateway (Recipe 1 in the topology
section).

### Wire format

Authenticated clients must send the standard `Authorization` header on
every request the auth layer protects:

```http
POST /v1/instances/register HTTP/1.1
Authorization: Bearer dcc-mcp-studio-token-...
Content-Type: application/json

{ "instance_id": "...", "dcc_type": "maya", "mcp_url": "..." }
```

The scheme is case-insensitive (`Bearer`, `bearer`, `BEARER`); the
token is byte-exact. The token comparison uses a constant-time loop to
avoid timing leaks.

### Authorisation: per-token `allowed_dcc` scope

Every `GatewayAuthToken` may declare an `allowed_dcc` set. On
`POST /v1/instances/register`, the gateway compares the request's
`dcc_type` against this set:

- `allowed_dcc == None` — token may register any DCC type.
- `allowed_dcc == Some({"maya", "blender"})` — only registrations with
  `dcc_type ∈ {"maya", "blender"}` succeed.

Other endpoints (call, read-resources, admin) currently re-use the
trust granted at registration time; per-call scope is tracked in
follow-ups under epic #1367.

### Error envelope

When auth fails, the gateway returns a structured JSON envelope. The
shape is agent-friendly and mirrors the rest of the `/v1/*` surface:

```json
{
  "ok": false,
  "success": false,
  "error": {
    "kind": "unauthorized",
    "message": "Authorization header is required for this endpoint."
  }
}
```

`error.kind` is one of:

| Kind                  | Status | Trigger                                                                              |
|-----------------------|--------|--------------------------------------------------------------------------------------|
| `unauthorized`        | 401    | Missing `Authorization` header, non-`Bearer` scheme, or token not in the allow-list. |
| `dcc_scope_mismatch`  | 403    | Token recognised, but `dcc_type` is outside the token's `allowed_dcc`.               |

`dcc_scope_mismatch` additionally carries `error.dcc_type` with the
rejected DCC name to keep the negative path debuggable from logs.

### TLS termination

The gateway daemon **does not** terminate TLS in-binary; that
intentionally matches the stance taken by `dcc-mcp-tunnel-relay`. Run
the daemon behind a reverse proxy (nginx, Caddy, a cloud load balancer)
that owns the certificate lifecycle, HTTP/2, rate limiting, and any
mTLS requirements your environment needs. The bearer-token contract
above continues to work end-to-end as long as the proxy forwards the
`Authorization` header verbatim.

### Hardening checklist for internet-exposed deployments

- TLS terminated by a reverse proxy in front of the daemon — never bind
  the daemon directly to a public interface.
- Bearer tokens stored as secrets (env var, secrets manager, mounted
  file) and never passed via process argv.
- `allowed_dcc` scope on every token unless one truly needs a master
  token for bootstrap.
- `AuditMiddleware` enabled so register / deregister / relay-attach
  lifecycle events land in `audit.jsonl`. The negative-path VRS trace
  at `tests/vrs/traces/core-1365-gateway-auth-negative.jsonl` covers
  the rejection envelopes and can be replayed against any gateway under
  test.
- Rate-limit + WAF rules on the reverse proxy for the `/v1/instances/*`
  paths so token brute-force is bounded.

## Event webhooks

Set `DCC_MCP_WEBHOOKS_CONFIG` to a YAML file path to forward EventBus
envelopes (`tool.*`, `skill.*`, `trace.*`, `gateway.*`) to external HTTP
endpoints. Each webhook specifies:

- `name` — stable identifier for logs and delivery-failed events.
- `url` — `http` / `https` endpoint.
- `events` — event name patterns (e.g. `["tool.*", "trace.*"]`).
- `filters` — optional dotted-path matchers with `*`-wildcard support.
- `delivery.attempts` — retry count (default `3`).
- `delivery.timeout_ms` — per-attempt timeout (default `2_000` ms).
- `backoff_ms` — per-retry delay sequence (default `[200, 1000, 5000]` ms).
- `payload_template` — optional template string using double-braced paths.

Example `webhooks.yaml`:

```yaml
queue_capacity: 256
webhooks:
  - name: analytics-forwarder
    url: https://internal.example.com/api/dcc-analytics
    events:
      - "tool.*"
      - "skill.*"
      - "trace.*"
      - "gateway.instance.*"
    headers:
      Authorization: "Bearer ${ANALYTICS_WEBHOOK_TOKEN}"
    delivery:
      attempts: 3
      timeout_ms: 5000
    filters:
      - name: "tool.completed"
      - name: "skill.loaded"
```

Headers support `${ENV_VAR}` interpolation so tokens stay out of version
control. The webhook runtime starts automatically when the env var points at a
valid YAML file.

## Sentry error monitoring (Rust backend)

Set `DCC_MCP_SENTRY_DSN` to your Sentry project DSN. The SDK initialises
at server startup and captures panics automatically. Use
`sentry::capture_error` or `sentry::capture_message` for explicit
instrumentation points.

| Variable | Default | Purpose |
|----------|---------|---------|
| `DCC_MCP_SENTRY_DSN` | (disabled) | Sentry project DSN |
| `DCC_MCP_SENTRY_ENVIRONMENT` | `production` | Environment tag |
| `DCC_MCP_SENTRY_RELEASE` | crate version | Release identifier for source-map / commit correlation |
| `DCC_MCP_SENTRY_SAMPLE_RATE` | `1.0` | Error event sample rate (0.0–1.0) |

The feature is compiled by default and skips initialisation entirely when
`DCC_MCP_SENTRY_DSN` is absent. Build with `--no-default-features` to
exclude the crate from the binary entirely.

See [sentry.md](sentry.md) for full reference including Python API and E2E
tests.

## Admin Integrations panel

The gateway admin dashboard exposes a read-only **Integrations** panel at
`GET /admin/api/integrations` (mirrored at `GET /v1/debug/integrations` for
agent access). The panel summarises the effective configuration for Sentry,
webhooks, and OTLP tracing:

| Integration | Configuration | Panel shows |
|-------------|---------------|-------------|
| Sentry | `DCC_MCP_SENTRY_DSN` | DSN status (set/unset), environment, sample rate |
| Webhooks | `DCC_MCP_WEBHOOKS_CONFIG` → YAML | Active webhook count and names |
| OTLP tracing | `OTEL_EXPORTER_OTLP_ENDPOINT` | Endpoint URL, service name |

All three integrations are configured through environment variables or
config files set before server startup — the panel is **read-only** and
flags `pending_restart` when a configuration change requires a gateway
restart to take effect. Secrets are never exposed in the JSON response.
See [admin-ui.md](admin-ui.md) for the full API reference.

## Non-goals

HTTP/2 multiplexing tuning and multi-backend failover for the routing
cache (routes are sticky) are out of scope for #320 / #321 / #322.
