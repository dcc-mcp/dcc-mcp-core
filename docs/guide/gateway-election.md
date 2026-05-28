# Gateway Election & Multi-Instance Support

> **[中文版](../zh/guide/gateway-election)**

## What is the Gateway?

The **gateway** is a single Rust HTTP server (running on `localhost:9765` by default) that:

- Discovers all running DCC instances (Maya, Blender, Houdini, Photoshop, etc.)
- Keeps one unified, bounded `/mcp` endpoint with four canonical workflow tools instead of fanning out every backend action
- Routes gateway MCP `search` / `describe` / `load_skill` / `call` and `/v1/*` REST calls to the selected backend capability
- Exposes skill lifecycle and execution through the canonical MCP tools plus REST (`/v1/load_skill`, `/v1/unload_skill`, `/v1/call`) while retaining hidden MCP compatibility routes for pinned clients
- Pushes progress, job/workflow, resource, and prompt notifications over SSE as instances come and go

**One gateway per machine**. It's started automatically when the first DCC instance registers.

## The Problem: First-Come-First-Served and Unsafe Preemption

Without version awareness, the oldest DCC wins the gateway role:

```
Maya v0.12.6 starts → binds port 9999 → becomes gateway
Maya v0.12.29 starts → port 9999 taken → becomes subordinate
❌ Old version controls routing; new features ignored
```

Pure version preemption has the opposite failure mode: a healthy existing
gateway can be asked to yield just because a newer adapter starts, dropping
long-lived MCP clients even though no failover is needed.

## Our Solution: Liveness-Aware Election

```
Maya v0.12.6 (gateway)           Maya v0.12.29 (new)
         │                                │
         │                   port 9999 taken
         │                                │
         │         read __gateway__ sentinel
         │         GET /health -> 200 OK
         │                                │
         │         healthy resident wins
         │                                │
                         register as plain instance
                         ✅ existing MCP sessions stay connected
```

If the resident does not answer `/health`, the newcomer enters challenger
mode and polls for the port so crash/TIME_WAIT recovery still works:

```
Maya v0.12.6 (gateway)           Maya v0.12.29 (new)
         │                                │
         │                   port 9999 taken
         │                                │
         │         read __gateway__ sentinel
         │         GET /health fails
         │         own version 0.12.29 > gw 0.12.6
         │                                │
         │ ←── POST /gateway/yield {"challenger_version": "0.12.29"}
         │
         │ (supports yield → graceful shutdown)
         │ yield_tx.send(true)
         │ release port 9999
         │
                          retry every 10s
                          port free → bind
                          register new sentinel
                          ✅ v0.12.29 is now gateway
```

### How It Works

**1. `__gateway__` Sentinel**

When a gateway starts, it writes a special entry to the FileRegistry:
```json
{
  "dcc_type":        "__gateway__",
  "version":         "0.14.18",
  "adapter_version": "0.3.0",
  "adapter_dcc":     "maya"
}
```

New instances read this to know who the gateway is, which `dcc-mcp-http`
crate it embeds, which adapter package shipped that crate, and which DCC
the adapter is bound to.

**2. Three-Tier Election Comparison** (issue maya#137)

The challenger still compares its own profile against the resident sentinel,
but that comparison only gates the cooperative yield request. A healthy
resident keeps the gateway role so co-existing DCC launches do not interrupt
active clients; missing or unhealthy residents enter challenger mode so
recovery can still happen.

When cooperative yield is considered, the profile comparison runs in this order
— each tier is only consulted when the previous tier ties:

| Tier | Field             | Rule                                                                 |
|------|-------------------|----------------------------------------------------------------------|
| 1    | `version`         | `dcc-mcp-http` crate semver — newer wins.                            |
| 2    | `adapter_version` | Adapter package semver — newer wins; `None` is below any value.      |
| 3    | `adapter_dcc`     | Real DCC (`"maya"`, `"houdini"`…) preempts `None` / `"unknown"`.     |

The third tier resolves the production failure reported in maya#137: a
generic standalone `dcc-mcp-server` pinned to a newer crate could keep
the gateway forever, so the freshly-installed Maya plugin never got to
serve its own tools. With the tiebreaker the Maya adapter wins at equal
crate + adapter versions, while two real DCCs at identical versions
remain tied (the existing first-wins port-bind contract takes over).

```
0.14.18  vs  0.14.18
adapter 0.3.0 vs adapter 0.3.0
adapter_dcc "maya" vs "unknown" → maya wins ✓
```

Versions are still parsed numerically (`"2024"` from a Maya host version
field would otherwise look newer than the `0.14.18` crate version — see
issue #228 — so only the `__gateway__` sentinel row contributes to the
self-yield decision).

**3. Resident Health Probe**

When the gateway port is occupied and a sentinel exists, new instances probe
`GET /health` on the resident gateway before considering challenger mode.
`200` means "stay plain"; non-success or transport failure means "recover or
fail over". If no sentinel exists, the process still enters challenger mode to
cover the post-crash TIME_WAIT race.

**4. Voluntary Yield**

The cleanup task (every 15s) checks for newer challengers. If found, it shuts
down gracefully. Healthy-resident startup no longer creates such a challenger
sentinel.

**5. Challenger Retry Loop**

New instances poll the port every 10s for up to 120s. As soon as the port is free, they take over.

### Stamping the Sentinel

`McpHttpConfig` exposes the two new fields so adapters can declare them
when wiring up the server:

```rust
let cfg = McpHttpConfig::new("maya", 18812)
    .with_gateway(9765)
    .with_adapter_version(env!("CARGO_PKG_VERSION"))
    .with_adapter_dcc("maya");
```

In Python:

```python
McpHttpConfig(
    dcc_type="maya",
    port=18812,
    gateway_port=9765,
    adapter_version="0.3.0",
    adapter_dcc="maya",
)
```

Both fields are optional; leaving them `None` reproduces the legacy
crate-version-only comparison and is treated as the lowest tier in the
new election.

## Multi-Instance Registration

Multiple DCC instances of the same type can coexist. Each DCC adapter
(`DccServerBase`) registers itself automatically when you call `.start()`;
if you need a lower-level view of the registry, use `ServiceEntry` plus
the `FileRegistry` exposed by `dcc_mcp_transport::discovery`.

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

# Maya #1 — animation work
cfg = McpHttpConfig(port=0, server_name="maya-animation")
cfg.gateway_port = 9765
cfg.dcc_type = "maya"
cfg.scene = "shot_001.ma"
cfg.dcc_version = "2025"
anim_handle = create_skill_server("maya", cfg).start()

# Maya #2 — rigging work (in a different process)
cfg = McpHttpConfig(port=0, server_name="maya-rigging")
cfg.gateway_port = 9765
cfg.dcc_type = "maya"
cfg.scene = "character_rig.ma"
cfg.dcc_version = "2025"
rig_handle = create_skill_server("maya", cfg).start()

# List instances from a third process — query the shared FileRegistry
# via the gateway's HTTP endpoint:
#   GET http://127.0.0.1:9765/instances
```

> **Legacy `TransportManager` is gone (issue #251, v0.14).** The custom
> connection pool / session manager / routing layer was removed together
> with `FramedChannel` and `IpcListener`. The replacement for per-connection
> framing is `IpcChannelAdapter` (see [Transport guide](transport.md));
> service discovery remains available through `FileRegistry` but is now
> accessed either via the gateway HTTP API or the `ServiceEntry` Python
> class directly.

### PID-liveness auto-eviction (issue #523)

`FileRegistry::read_alive(allow_pid_zero)` returns the same entries as
`read()` but **transparently drops** any entry whose `pid` no longer maps to
a live process and rewrites the registry file to persist the eviction. Use
this when you build dashboards or routing tables that must not enumerate
crashed-host stragglers — there is no separate cleanup loop to run.

```rust
use dcc_mcp_transport::discovery::FileRegistry;

let registry = FileRegistry::new("/var/run/dcc-mcp/registry.json")?;
// allow_pid_zero=true keeps pid=0 entries (sentinels like __gateway__).
let alive = registry.read_alive(true)?;
```

The DCC-Gateway HTTP `/instances` route already calls this internally, so
clients consuming the gateway API see the same auto-evicted view.

## Listener lifecycle under PyO3-embedded hosts (issue #303)

On Windows `mayapy` and any other PyO3-embedded interpreter, the default
`McpHttpConfig.spawn_mode` is **`"dedicated"`**. That runs each HTTP
listener (instance MCP endpoint and the optional gateway) on its own
OS thread that owns a single-threaded Tokio runtime. `tokio::spawn` onto
the parent multi-threaded runtime is unreliable under embedded Python —
once `block_on` returns the worker pool can be starved long enough that
the accept loop never gets to run, producing the "`is_gateway=True` but
the port refuses connections" symptom reported in issue #303.

The standalone `dcc-mcp-server` binary runs under `#[tokio::main]` and
keeps `spawn_mode = "ambient"` — the main thread drives the runtime for
the process lifetime, so a dedicated thread would be pure overhead.

If you need to override the default:

```python
cfg = McpHttpConfig(port=0)
cfg.spawn_mode = "ambient"      # or "dedicated"
cfg.self_probe_timeout_ms = 200  # per-attempt probe timeout
```

`McpHttpServer.start()` self-probes the listener before returning the
handle and refuses to return a misreporting `is_gateway=True`; the
corresponding Rust invariants are enforced by
`crates/dcc-mcp-http/tests/gateway_reachability.rs` and the Python
suite `tests/test_gateway_reachability.py`.

## Document Tracking

For multi-document DCCs (Photoshop, After Effects), track all open files:

```python
# Photoshop registers with initial documents
iid = mgr.register_service(
    "photoshop", "127.0.0.1", 18820,
    pid=55001,
    display_name="PS-Marketing",
    scene="logo.psd",
    documents=["logo.psd", "banner.psd"],
)

# User opens a new document
mgr.update_documents(
    "photoshop", iid,
    active_document="icon.psd",
    documents=["logo.psd", "banner.psd", "icon.psd"],
)

# User switches active document
mgr.update_documents(
    "photoshop", iid,
    active_document="banner.psd",
    documents=["logo.psd", "banner.psd", "icon.psd"],
)

entry = mgr.get_service("photoshop", iid)
print(entry.scene)      # "banner.psd" (active)
print(entry.documents)  # ["logo.psd", "banner.psd", "icon.psd"]
```

## Session Isolation

Each AI session is **pinned to one instance**:

```python
# AI Agent A talks only to Maya-Animation
session_a = mgr.get_or_create_session("maya", iid_anim)

# AI Agent B talks only to Maya-Rigging
session_b = mgr.get_or_create_session("maya", iid_rig)

# Sessions are different — no context bleeding
assert session_a != session_b

# Through the gateway, the agent targets an instance by choosing a
# tool_slug returned from MCP search or /v1/search:
#   maya.a1b2c3d4.set_keyframe   ← maya-animation
#   maya.e5f6g7h8.mirror_joints  ← maya-rigging
```

## Instance Health

```python
# Keep instance alive with heartbeats
mgr.heartbeat("maya", iid)  # → True if alive, False if not found

# Update instance status
from dcc_mcp_core import ServiceStatus
mgr.update_service_status("maya", iid, ServiceStatus.BUSY)

# Cleanup when DCC exits
mgr.deregister_service("maya", iid)
```

## Backward Compatibility

Older DCC versions that don't support `/gateway/yield` return 404 — that's OK. The challenger enters a polling retry loop and waits until the port is naturally freed (when old DCC exits or crashes). No hard failures; graceful degradation.

## DccGatewayElection (Python API)

`DccGatewayElection` is a pure-Python class that provides **automatic gateway failover** for non-gateway DCC instances. When the current gateway becomes unreachable, the election thread automatically attempts to take over.

### How It Works

1. A background daemon thread periodically probes the gateway's `/health` endpoint
2. Counts consecutive probe failures
3. When failures exceed the threshold, attempts a first-wins TCP port check
4. If the port is free, signals the server to upgrade to gateway mode

### Constructor

```python
from dcc_mcp_core import DccGatewayElection

election = DccGatewayElection(
    dcc_name="blender",           # Short DCC identifier for logs
    server=blender_server,        # DCC server instance (must expose is_gateway, is_running, _handle)
    gateway_host="127.0.0.1",     # Gateway bind address
    gateway_port=9765,            # Gateway port to compete for
    probe_interval=5,             # Seconds between health probes
    probe_timeout=2.0,            # Timeout per probe in seconds
    probe_failures=3,             # Consecutive failures before election attempt
    on_promote=None,              # Optional callable: () -> bool, overrides server._upgrade_to_gateway()
)
```

### Configuration via Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DCC_MCP_GATEWAY_PROBE_INTERVAL` | `5` | Seconds between health probes |
| `DCC_MCP_GATEWAY_PROBE_TIMEOUT` | `2` | Timeout per probe in seconds |
| `DCC_MCP_GATEWAY_PROBE_FAILURES` | `3` | Consecutive failures before election |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `is_running` | `bool` | Whether the election thread is active |
| `consecutive_failures` | `int` | Current consecutive gateway probe failure count |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `start()` | `None` | Start the background election thread (idempotent) |
| `stop()` | `None` | Gracefully stop the thread (waits up to 5s) |
| `get_status()` | `dict` | Returns `{running, consecutive_failures, gateway_host, gateway_port}` |

### Promotion Path

When the election wins (port is free), it resolves the promotion path in order:

1. The `on_promote` callable passed to `__init__` (if any)
2. `server._upgrade_to_gateway()` method on the bound server (if it exposes one)
3. Fallback: logs a warning and returns `False`

### Usage with DccServerBase

`DccServerBase` integrates `DccGatewayElection` automatically:

```python
from dcc_mcp_core import DccServerBase

class BlenderMcpServer(DccServerBase):
    def __init__(self, **kwargs):
        super().__init__(dcc_name="blender", builtin_skills_dir=..., **kwargs)

server = BlenderMcpServer(gateway_port=9765)
server.register_builtin_actions()
handle = server.start()        # election thread starts automatically
print(server._election.get_status())  # inspect election state
```

### Standalone Usage

```python
from dcc_mcp_core import DccGatewayElection

# With a custom promotion callback
def promote():
    # Restart the MCP server with gateway port
    return True

election = DccGatewayElection(
    dcc_name="blender",
    server=my_server,
    gateway_port=9765,
    on_promote=promote,
)
election.start()

# Later...
status = election.get_status()
# {"running": True, "consecutive_failures": 0, "gateway_host": "127.0.0.1", "gateway_port": 9765}

election.stop()
```

## Failover Diagnostics (issue #1355)

Standalone gateway exit vs. embedded adapter promotion are easy to confuse
when debugging a multi-DCC session. To make the state observable from any
MCP client, `DccServerBase` registers a built-in
`dcc_diagnostics__gateway_failover` MCP tool that returns the current
election state for the bound adapter:

| Field                    | Type   | Meaning                                                                                              |
|--------------------------|--------|------------------------------------------------------------------------------------------------------|
| `enabled`                | bool   | Adapter opted into automatic gateway failover (`_enable_gateway_failover`).                          |
| `running`                | bool   | The election daemon thread is alive.                                                                 |
| `consecutive_failures`   | int    | Probe failures since the last successful health check.                                               |
| `gateway_host`           | str?   | Bind address the election would race for.                                                            |
| `gateway_port`           | int    | Bind port. `0` means failover cannot run even when `enabled=True`.                                   |
| `is_gateway`             | bool   | This server currently owns the gateway port.                                                         |
| `reason`                 | str    | One of `failover_disabled_by_adapter`, `gateway_port_not_configured`, `election_thread_not_started`, `election_active`, `active_gateway`, `failover_resolver_not_registered`. |
| `timestamp_ms`           | int    | Wall-clock time of the snapshot.                                                                     |

The `reason` field is the canonical answer to *"why didn't a backend take
over after the standalone gateway exited?"*:

- `failover_disabled_by_adapter` — the adapter explicitly disabled failover
  at construction time. Embedded DCC plugins that should never bid for the
  gateway port (e.g. read-only viewers, asset browsers) intentionally land
  here.
- `gateway_port_not_configured` — failover is enabled but `gateway_port==0`
  in `McpHttpConfig`. The standalone `dcc-mcp-server gateway` daemon owns
  the gateway plane; embedded DCC servers without a `gateway_port` will
  never promote themselves.
- `election_thread_not_started` — failover is enabled, port is configured,
  but the election thread has not yet been started (e.g. `server.start()`
  has not been called, or startup failed and is being retried).
- `election_active` — election thread is running and probing the gateway.
  `consecutive_failures` reflects how close it is to triggering promotion.
- `active_gateway` — this instance currently owns the gateway port. Most
  often the result of a successful failover, but also the steady state of
  the very first embedded adapter to start when no daemon is running.

### Standalone gateway exit vs. embedded promotion

When `dcc-mcp-server gateway` runs as a separate process and exits cleanly,
its `__gateway__` sentinel file is removed and any embedded adapter with
`enabled=True` + `gateway_port>0` will promote on the next election tick
(default `DCC_MCP_GATEWAY_PROBE_INTERVAL=5s`). If you see
`reason="failover_disabled_by_adapter"` from every embedded backend in
that scenario, the daemon must be restarted out-of-band — embedded
adapters are intentionally opting out and will never recover the gateway
plane themselves.
