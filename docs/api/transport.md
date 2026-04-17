# Transport API

`dcc_mcp_core` — TransportManager, TransportAddress, TransportScheme, RoutingStrategy, ServiceStatus, ServiceEntry, IpcListener, ListenerHandle, FramedChannel, connect_ipc.

## Overview

The transport module provides **cross-platform IPC and TCP communication** between AI agents and DCC applications. Key design decisions:

- **Named Pipes** (Windows) and **Unix Domain Sockets** (macOS/Linux) are preferred for same-machine connections — sub-millisecond latency, zero configuration.
- **TCP** is the fallback for cross-machine or when IPC is unavailable.
- `TransportAddress.default_local(dcc_type, pid)` auto-selects the optimal transport for the current platform.
- `TransportManager.bind_and_register()` is the recommended one-call setup for DCC plugin authors.

## TransportAddress

Protocol-agnostic transport endpoint. Supports TCP, Named Pipes (Windows), and Unix Domain Sockets (macOS/Linux).

### Factory Methods

```python
from dcc_mcp_core import TransportAddress

# TCP
addr = TransportAddress.tcp("127.0.0.1", 18812)

# Named Pipe (Windows)
addr = TransportAddress.named_pipe("dcc-maya-12345")

# Unix Domain Socket (macOS/Linux)
addr = TransportAddress.unix_socket("/tmp/dcc-maya-12345.sock")

# Auto-select optimal local transport for current platform
addr = TransportAddress.default_local("maya", pid=os.getpid())

# Parse from URI string
addr = TransportAddress.parse("tcp://127.0.0.1:18812")
addr = TransportAddress.parse("pipe://dcc-maya-12345")
addr = TransportAddress.parse("unix:///tmp/dcc-maya.sock")
```

### Static Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `tcp(host, port)` | `TransportAddress` | Create TCP address |
| `named_pipe(name)` | `TransportAddress` | Create Named Pipe address (Windows) |
| `unix_socket(path)` | `TransportAddress` | Create Unix Socket address |
| `default_local(dcc_type, pid)` | `TransportAddress` | Auto-select optimal local transport |
| `default_pipe_name(dcc_type, pid)` | `TransportAddress` | Named Pipe for DCC instance |
| `default_unix_socket(dcc_type, pid)` | `TransportAddress` | Unix Socket for DCC instance |
| `parse(s)` | `TransportAddress` | Parse URI string (`tcp://`, `pipe://`, `unix://`) |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `scheme` | `str` | Transport scheme: `"tcp"`, `"pipe"`, or `"unix"` |
| `is_local` | `bool` | Whether this is a same-machine transport |
| `is_tcp` | `bool` | Whether this is a TCP transport |
| `is_named_pipe` | `bool` | Whether this is a Named Pipe transport |
| `is_unix_socket` | `bool` | Whether this is a Unix Socket transport |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `to_connection_string()` | `str` | URI string, e.g. `"tcp://127.0.0.1:18812"` |

### Example

```python
import os
from dcc_mcp_core import TransportAddress

# Optimal local transport (IPC on all platforms)
addr = TransportAddress.default_local("maya", os.getpid())
print(addr.scheme)   # "pipe" on Windows, "unix" on macOS/Linux
print(addr.is_local) # True
print(addr)          # e.g. "pipe://dcc-maya-12345"
```

## TransportScheme

Strategy for choosing the optimal communication channel.

### Constants

| Constant | Description |
|----------|-------------|
| `AUTO` | Auto-select best transport |
| `TCP_ONLY` | Always use TCP |
| `PREFER_NAMED_PIPE` | Prefer Named Pipe, fall back to TCP |
| `PREFER_UNIX_SOCKET` | Prefer Unix Socket, fall back to TCP |
| `PREFER_IPC` | Prefer any IPC (Pipe or Unix Socket), fall back to TCP |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `select_address(dcc_type, host, port, pid=None)` | `TransportAddress` | Select optimal address |

```python
from dcc_mcp_core import TransportScheme

addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812, pid=12345)
```

## RoutingStrategy

Strategy for selecting among multiple DCC instances.

### Constants

| Constant | Description |
|----------|-------------|
| `FIRST_AVAILABLE` | Use first available instance |
| `ROUND_ROBIN` | Rotate across all available instances |
| `LEAST_BUSY` | Prefer instances with lowest load |
| `SPECIFIC` | Target a specific instance by ID |
| `SCENE_MATCH` | Prefer instance with matching open scene |
| `RANDOM` | Random selection |

```python
from dcc_mcp_core import RoutingStrategy, TransportManager

mgr = TransportManager("/tmp/dcc-mcp")
session_id = mgr.get_or_create_session_routed(
    "maya",
    strategy=RoutingStrategy.ROUND_ROBIN,
)
```

## ServiceStatus

Enum for DCC service instance status.

### Constants

| Constant | Description |
|----------|-------------|
| `AVAILABLE` | Accepting connections (default) |
| `BUSY` | Processing a request |
| `UNREACHABLE` | Health check failed |
| `SHUTTING_DOWN` | Shutting down |

```python
from dcc_mcp_core import ServiceStatus, TransportManager

mgr = TransportManager("/tmp/dcc-mcp")
mgr.update_service_status("maya", instance_id, ServiceStatus.BUSY)
```

## ServiceEntry

Represents a discovered DCC service instance.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `dcc_type` | `str` | DCC application type (e.g. `"maya"`) |
| `instance_id` | `str` | UUID string |
| `host` | `str` | Host address |
| `port` | `int` | TCP port |
| `version` | `str \| None` | DCC version |
| `scene` | `str \| None` | Currently open scene/file |
| `metadata` | `dict[str, str]` | Arbitrary string-only metadata |
| `extras` | `dict[str, Any]` | JSON-typed DCC metadata (e.g. `cdp_port`, `pid`, nested config) — empty dict when unset |
| `status` | `ServiceStatus` | Instance status |
| `transport_address` | `TransportAddress \| None` | Preferred IPC address |
| `last_heartbeat_ms` | `int` | Last heartbeat timestamp (Unix ms) |
| `is_ipc` | `bool` | Whether using IPC transport |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `effective_address()` | `TransportAddress` | IPC address or TCP fallback |
| `to_dict()` | `dict` | Serialize to dict |

### Example

```python
entry = mgr.find_best_service("maya")
print(entry.dcc_type)         # "maya"
print(entry.status)           # ServiceStatus.AVAILABLE
print(entry.effective_address())  # e.g. TransportAddress("pipe://dcc-maya-12345")

# Check idle time
import time
idle_sec = (time.time() * 1000 - entry.last_heartbeat_ms) / 1000
if idle_sec > 300:
    mgr.deregister_service("maya", entry.instance_id)
```

## TransportManager

Transport layer manager with service discovery, smart routing, sessions, and connection pooling.

### Constructor

```python
from dcc_mcp_core import TransportManager

mgr = TransportManager(
    registry_dir="/tmp/dcc-mcp",
    max_connections_per_dcc=10,
    idle_timeout=300,
    heartbeat_interval=5,
    connect_timeout=10,
    reconnect_max_retries=3,
)
```

### Service Discovery

| Method | Returns | Description |
|--------|---------|-------------|
| `register_service(dcc_type, host, port, version=None, scene=None, metadata=None, transport_address=None, extras=None)` | `str` | Register a service, returns instance_id (UUID). `extras` accepts a `dict[str, Any]` of JSON-typed metadata (nested dicts / lists / numbers allowed) |
| `deregister_service(dcc_type, instance_id)` | `bool` | Deregister a service by key |
| `list_instances(dcc_type)` | `list[ServiceEntry]` | List all instances for a DCC type |
| `list_all_services()` | `list[ServiceEntry]` | List all registered services |
| `list_all_instances()` | `list[ServiceEntry]` | Alias for `list_all_services()` |
| `get_service(dcc_type, instance_id)` | `ServiceEntry \| None` | Get a specific instance |
| `heartbeat(dcc_type, instance_id)` | `bool` | Update heartbeat timestamp |
| `update_service_status(dcc_type, instance_id, status)` | `bool` | Set instance status |

#### `register_service` — IPC transport parameter

Pass `transport_address` to enable Named Pipe / Unix Socket for lower-latency same-machine connections:

```python
import os
from dcc_mcp_core import TransportManager, TransportAddress

mgr = TransportManager("/tmp/dcc-mcp")
addr = TransportAddress.default_local("maya", os.getpid())
instance_id = mgr.register_service(
    "maya", "127.0.0.1", 18812,
    version="2025",
    transport_address=addr,
)
```

#### `register_service` — JSON-typed `extras`

`metadata=` only accepts flat `dict[str, str]`. When the DCC needs to advertise
numeric ports, nested objects, or typed flags, pass them through `extras=`:

```python
instance_id = mgr.register_service(
    "photoshop", "127.0.0.1", 8888,
    version="2024",
    extras={
        "cdp_port": 9222,              # integer survives the round-trip
        "pid": os.getpid(),
        "features": {"webview": True}, # nested dict is preserved
    },
)

entry = mgr.get_service("photoshop", instance_id)
assert entry.extras["cdp_port"] == 9222
assert entry.extras["features"]["webview"] is True
```

`extras` defaults to `{}` and is omitted from the on-disk `services.json` when
empty, so legacy registries remain byte-identical.

### Smart Routing

#### `find_best_service()`

Returns the highest-priority live `ServiceEntry`. Priority: local IPC > local TCP > remote TCP. Within the same tier, `AVAILABLE` beats `BUSY`. Across equal-priority instances, round-robin load balancing is applied automatically.

```python
entry = mgr.find_best_service("maya")
session_id = mgr.get_or_create_session("maya", entry.instance_id)
```

#### `rank_services()`

Returns all live instances sorted by preference (lowest-score = best):

| Score | Tier |
|-------|------|
| 0 | Local IPC, AVAILABLE |
| 1 | Local IPC, BUSY |
| 2 | Local TCP, AVAILABLE |
| 3 | Local TCP, BUSY |
| 4 | Remote TCP, AVAILABLE |
| 5 | Remote TCP, BUSY |

`UNREACHABLE` and `SHUTTING_DOWN` instances are excluded.

```python
for entry in mgr.rank_services("maya"):
    print(entry.instance_id, entry.status, entry.effective_address())
    sid = mgr.get_or_create_session("maya", entry.instance_id)
    # dispatch work to this instance via session sid
```

#### `bind_and_register()`

One-call setup for DCC plugin authors. Binds a listener on the optimal transport and registers this DCC instance in one step:

```python
from dcc_mcp_core import TransportManager

mgr = TransportManager("/tmp/dcc-mcp")
instance_id, listener = mgr.bind_and_register("maya", version="2025")
local_addr = listener.local_address()
print(f"Listening on {local_addr}")  # e.g. unix:///tmp/dcc-mcp-maya-12345.sock

# Hand the listener to a serve loop (DCC plugin thread)
channel = listener.accept()
```

Transport selection priority: Named Pipe (Windows) / Unix Socket (macOS/Linux) → TCP on ephemeral port.

### Session Management

| Method | Returns | Description |
|--------|---------|-------------|
| `get_or_create_session(dcc_type, instance_id=None)` | `str` | Get/create a session (UUID). Picks first available if no instance_id |
| `get_or_create_session_routed(dcc_type, strategy=None, hint=None)` | `str` | Get/create session with routing strategy |
| `get_session(session_id)` | `dict \| None` | Get session info dict |
| `record_success(session_id, latency_ms)` | — | Record successful request |
| `record_error(session_id, latency_ms, error)` | — | Record failed request |
| `begin_reconnect(session_id)` | `int` | Begin reconnection, returns backoff ms |
| `reconnect_success(session_id)` | — | Mark reconnection as successful |
| `close_session(session_id)` | `bool` | Close a session |
| `list_sessions()` | `list[dict]` | List all active sessions |
| `list_sessions_for_dcc(dcc_type)` | `list[dict]` | List sessions for a specific DCC |
| `session_count()` | `int` | Number of active sessions |

### Connection Pool

| Method | Returns | Description |
|--------|---------|-------------|
| `acquire_connection(dcc_type, instance_id=None)` | `str` | Acquire connection (UUID) |
| `release_connection(dcc_type, instance_id)` | — | Release connection back to pool |
| `pool_size()` | `int` | Total connections in pool |
| `pool_count_for_dcc(dcc_type)` | `int` | Pool size for a specific DCC |

### Lifecycle

| Method | Returns | Description |
|--------|---------|-------------|
| `cleanup()` | `tuple[int, int, int]` | Returns (stale_services, closed_sessions, evicted_connections) |
| `shutdown()` | — | Graceful shutdown |
| `is_shutdown()` | `bool` | Check if transport is shut down |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__repr__` | `TransportManager(services=N, sessions=N, pool=N)` |
| `__len__` | Returns session count |

## IpcListener

Async IPC listener for DCC server-side applications. Supports TCP, Windows Named Pipes, and Unix Domain Sockets.

### Static Factory

```python
from dcc_mcp_core import IpcListener, TransportAddress

addr = TransportAddress.tcp("127.0.0.1", 0)  # port 0 = OS assigns free port
listener = IpcListener.bind(addr)
print(listener.local_address())  # e.g. "tcp://127.0.0.1:54321"
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `IpcListener.bind(addr)` | `IpcListener` | Bind to transport address |
| `local_address()` | `TransportAddress` | Get bound local address |
| `accept(timeout_ms=None)` | `FramedChannel` | Accept next connection (blocking) |
| `into_handle()` | `ListenerHandle` | Wrap for connection tracking (consumes listener) |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `transport_name` | `str` | Transport type: `"tcp"`, `"named_pipe"`, or `"unix_socket"` |

::: tip
`IpcListener.bind()` with port `0` lets the OS assign a free port. Call `local_address()` after binding to discover the actual port.
:::

## ListenerHandle

IPC listener handle with connection tracking and shutdown control.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `accept_count` | `int` | Number of connections accepted |
| `is_shutdown` | `bool` | Whether shutdown has been requested |
| `transport_name` | `str` | Transport type string |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `local_address()` | `TransportAddress` | Get bound local address |
| `shutdown()` | — | Request stop (idempotent) |

### Example

```python
addr = TransportAddress.tcp("127.0.0.1", 0)
listener = IpcListener.bind(addr)
handle = listener.into_handle()

print(handle.accept_count)   # 0
print(handle.is_shutdown)    # False

# ... accept connections in another thread ...

handle.shutdown()
```

## FramedChannel

Channel-based full-duplex framed communication for DCC connections. Wraps TCP/IPC with automatic Ping/Pong heartbeats and message buffering.

### Obtaining Instances

```python
from dcc_mcp_core import connect_ipc, IpcListener, TransportAddress

# Server side: accept from IpcListener
addr = TransportAddress.tcp("127.0.0.1", 0)
listener = IpcListener.bind(addr)
channel = listener.accept()

# Client side: connect to running DCC
addr = TransportAddress.tcp("127.0.0.1", 18812)
channel = connect_ipc(addr)
```

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `is_running` | `bool` | Whether background reader task is running |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `call(method, params=None, timeout_ms=30000)` | `dict` | Send request, wait for response (RPC) |
| `recv(timeout_ms=None)` | `dict \| None` | Receive next data message (blocking) |
| `try_recv()` | `dict \| None` | Receive without blocking |
| `ping(timeout_ms=5000)` | `int` | Send heartbeat, returns RTT ms |
| `send_request(method, params=None)` | `str` | Send request, returns request_id UUID |
| `send_response(request_id, success, payload=None, error=None)` | — | Send response |
| `send_notify(topic, data=None)` | — | Send one-way notification |
| `shutdown()` | — | Graceful shutdown (idempotent) |

### `call()` — Recommended RPC Pattern

The primary way to invoke DCC commands. Sends a `Request` and waits for the correlated `Response`:

```python
result = channel.call("execute_python", b'print("hello")', timeout_ms=10000)
if result["success"]:
    print(result["payload"])   # bytes
else:
    raise RuntimeError(result["error"])
```

`call()` return dict keys:

| Key | Type | Description |
|-----|------|-------------|
| `id` | `str` | UUID of the correlated request |
| `success` | `bool` | Whether the DCC executed successfully |
| `payload` | `bytes` | Serialized result data |
| `error` | `str \| None` | Error message when `success` is `False` |

::: tip
Unrelated messages (Notifications, other Responses) that arrive while `call()` is waiting are **not lost** — they remain available via `recv()`.
:::

### `recv()` — Event Loop Pattern

For server-side DCC plugins that need to handle multiple message types:

```python
while True:
    msg = channel.recv(timeout_ms=100)
    if msg is None:
        continue  # timeout or closed

    if msg["type"] == "request":
        handle_request(channel, msg)
    elif msg["type"] == "notify":
        handle_notification(msg)
    elif msg["type"] == "response":
        handle_response(msg)
```

### Ping / Health Check

```python
rtt_ms = channel.ping(timeout_ms=5000)
print(f"DCC ping: {rtt_ms}ms")
```

## connect_ipc()

Top-level function to create a client-side `FramedChannel` to a running DCC server.

```python
from dcc_mcp_core import connect_ipc, TransportAddress

addr = TransportAddress.default_local("maya", pid=12345)
channel = connect_ipc(addr)

rtt = channel.ping()
print(f"Connected, RTT: {rtt}ms")

result = channel.call("get_scene_info")
channel.shutdown()
```

## Full Integration Example

```python
import os
import time
from dcc_mcp_core import TransportManager, TransportAddress, RoutingStrategy

# --- DCC Plugin side (runs inside Maya/Blender) ---
def start_dcc_server(dcc_type: str):
    mgr = TransportManager("/tmp/dcc-mcp")
    instance_id, listener = mgr.bind_and_register(dcc_type, version="2025")
    print(f"DCC server bound to: {listener.local_address()}")

    # Accept client connections in a loop
    while True:
        channel = listener.accept(timeout_ms=1000)
        if channel:
            msg = channel.recv()
            if msg and msg["type"] == "request":
                channel.send_response(
                    msg["id"],
                    success=True,
                    payload=b'{"status": "ok"}',
                )


# --- Agent side (AI tool, external script) ---
def connect_to_maya():
    mgr = TransportManager("/tmp/dcc-mcp")

    # Find best Maya instance (IPC-first, then TCP)
    entry = mgr.find_best_service("maya")
    print(f"Connecting to {entry.effective_address()}")

    # Round-robin across instances for load distribution
    for entry in mgr.rank_services("maya")[:3]:
        sid = mgr.get_or_create_session_routed(
            "maya",
            strategy=RoutingStrategy.ROUND_ROBIN,
        )
```

## Wire Protocol Notes

Messages use MessagePack serialization with a 4-byte big-endian length prefix:

```
[4-byte length][MessagePack payload]
```

- **Request**: `{ id: UUID, method: String, params: Vec<u8> }`
- **Response**: `{ id: UUID, success: bool, payload: Vec<u8>, error: Option<String> }`
- **Notify**: `{ topic: String, data: Vec<u8> }`
- **Ping/Pong**: handled automatically by `FramedChannel`

## Low-Level Frame Encoding

For advanced use cases where you need to encode/decode raw frames (e.g. implementing a custom transport or testing):

### `encode_request()`

```python
from dcc_mcp_core import encode_request

frame = encode_request("execute_python", b'cmds.sphere()')
# bytes: [4-byte BE length][MessagePack payload]
```

### `encode_response()`

```python
from dcc_mcp_core import encode_response

frame = encode_response(
    request_id="550e8400-e29b-41d4-a716-446655440000",
    success=True,
    payload=b'{"result": "pSphere1"}',
)

# Error response
frame = encode_response(
    request_id="550e8400-e29b-41d4-a716-446655440000",
    success=False,
    error="Action failed: object not found",
)
```

### `encode_notify()`

```python
from dcc_mcp_core import encode_notify

frame = encode_notify("scene_changed", b'{"change": "object_added"}')
frame = encode_notify("render_complete")  # data optional
```

### `decode_envelope()`

Decode a raw MessagePack payload (length prefix already stripped) into a message dict:

```python
from dcc_mcp_core import encode_request, decode_envelope

frame = encode_request("ping", b"")
msg = decode_envelope(frame[4:])  # strip 4-byte length prefix

print(msg["type"])    # "request"
print(msg["method"]) # "ping"
```

Returned dict structure by `"type"`:

| Type | Fields |
|------|--------|
| `"request"` | `id` (str), `method` (str), `params` (bytes) |
| `"response"` | `id` (str), `success` (bool), `payload` (bytes), `error` (str\|None) |
| `"notify"` | `id` (str\|None), `topic` (str), `data` (bytes) |
| `"ping"` | `id` (str), `timestamp_ms` (int) |
| `"pong"` | `id` (str), `timestamp_ms` (int) |
| `"shutdown"` | `reason` (str\|None) |
