# Gateway Election & Multi-Instance Support

> **[中文版](../zh/guide/gateway-election)**

## What is the Gateway?

The **gateway** is a single Rust HTTP server (running on `localhost:9765` by default) that:

- Discovers all running DCC instances (Maya, Blender, Houdini, Photoshop, etc.)
- Aggregates every live backend's tools into one unified `/mcp` endpoint (namespaced by `{instance_short}__{name}`)
- Fans out skill-management calls (`search_skills`, `list_skills`) and routes targeted calls (`load_skill`) to a specific instance
- Pushes `tools/list_changed` and `resources/list_changed` over SSE as skills load/unload or instances come and go

**One gateway per machine**. It's started automatically when the first DCC instance registers.

## The Problem: First-Come-First-Served

Without version awareness, the oldest DCC wins the gateway role:

```
Maya v0.12.6 starts → binds port 9999 → becomes gateway
Maya v0.12.29 starts → port 9999 taken → becomes subordinate
❌ Old version controls routing; new features ignored
```

## Our Solution: Version-Aware Election

```
Maya v0.12.6 (gateway)           Maya v0.12.29 (new)
         │                                │
         │                   port 9999 taken
         │                                │
         │         read __gateway__ sentinel
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
{"dcc_type": "__gateway__", "version": "0.12.29"}
```

New instances read this to know who the gateway is and what version it runs.

**2. Semantic Version Comparison**

Versions are compared numerically (not alphabetically):
```
0.12.6  vs  0.12.29
↓              ↓
[0, 12, 6]  [0, 12, 29]
                 29 > 6 → v0.12.29 is newer ✓
```

**3. Voluntary Yield**

The cleanup task (every 15s) checks for newer challengers. If found, it shuts down gracefully.

**4. Challenger Retry Loop**

New instances poll the port every 10s for up to 120s. As soon as the port is free, they take over.

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

# Through the aggregating gateway, both instances' tools appear in a single
# tools/list with distinct 8-char prefixes, so the agent can target either:
#   a1b2c3d4__set_keyframe   ← maya-animation
#   e5f6g7h8__mirror_joints  ← maya-rigging
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
