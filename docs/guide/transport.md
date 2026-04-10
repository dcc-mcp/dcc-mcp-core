# Transport Layer

The Transport layer (`dcc-mcp-transport` crate) provides async communication infrastructure for connecting MCP servers to DCC application instances. It includes connection pooling, service discovery, session management, and a wire protocol.

## Overview

```python
from dcc_mcp_core import TransportManager

transport = TransportManager("/path/to/registry")

# Register a DCC service
instance_id = transport.register_service("maya", "127.0.0.1", 18812, version="2025.1")

# Create a session
session_id = transport.get_or_create_session("maya")

# Use the connection
conn_id = transport.acquire_connection("maya")
# ... perform operations ...
transport.release_connection("maya", instance_id)

# Cleanup and shutdown
transport.cleanup()
transport.shutdown()
```

## Service Discovery

The transport layer uses file-based service discovery to track running DCC instances. Each instance registers with a `(dcc_type, instance_id)` key, enabling multiple instances of the same DCC.

```python
id1 = transport.register_service("maya", "127.0.0.1", 18812)
id2 = transport.register_service("maya", "127.0.0.1", 18813)
id3 = transport.register_service("blender", "127.0.0.1", 9090, version="4.0")

maya_instances = transport.list_instances("maya")
all_services = transport.list_all_services()

transport.heartbeat("maya", id1)
transport.deregister_service("maya", id1)
```

## Session Management

Sessions track connections to DCC instances with lifecycle state management and metrics:

```python
session_id = transport.get_or_create_session("maya", id1)

session = transport.get_session(session_id)
# session is a dict with keys: id, dcc_type, instance_id, state, request_count, error_count, last_error, created_at, last_request_at

transport.record_success(session_id, 50)
transport.record_error(session_id, 100, "timeout")

backoff_ms = transport.begin_reconnect(session_id)
transport.reconnect_success(session_id)

transport.close_session(session_id)
```

### Session States

| State | Description |
|-------|-------------|
| `connected` | Active and ready for requests |
| `idle` | Idle timeout exceeded, still valid |
| `reconnecting` | Reconnecting after failure |
| `closed` | Terminal state |

## Connection Pool

```python
conn_id = transport.acquire_connection("maya")
transport.release_connection("maya", id1)
transport.pool_size()
```

## Configuration

```python
transport = TransportManager(
    registry_dir="/path/to/registry",
    max_connections_per_dcc=10,
    idle_timeout=300,
    heartbeat_interval=5,
    connect_timeout=10,
    reconnect_max_retries=3,
)
```

## Lifecycle

```python
stale, sessions, evicted = transport.cleanup()
transport.shutdown()
transport.is_shutdown()
```

---

## Low-Level IPC API

For DCC plugins that need to act as a server or communicate directly over IPC (bypassing `TransportManager`), use the low-level classes.

### TransportAddress

Protocol-agnostic endpoint descriptor. Supports TCP, Windows Named Pipes, and Unix Domain Sockets.

```python
from dcc_mcp_core import TransportAddress

# Factory constructors
addr = TransportAddress.tcp("127.0.0.1", 18812)
addr = TransportAddress.named_pipe("maya-mcp")          # Windows
addr = TransportAddress.unix_socket("/tmp/maya.sock")   # macOS/Linux

# Platform-optimal local address (PID-unique)
addr = TransportAddress.default_local("maya", pid=12345)

# Parse from URI string
addr = TransportAddress.parse("tcp://127.0.0.1:18812")
```

| Property/Method | Returns | Description |
|-----------------|---------|-------------|
| `scheme` | `str` | `"tcp"`, `"pipe"`, or `"unix"` |
| `is_local` | `bool` | Whether this is a same-machine transport |
| `is_tcp` | `bool` | Whether this is TCP |
| `is_named_pipe` | `bool` | Whether this is a Named Pipe |
| `is_unix_socket` | `bool` | Whether this is a Unix Socket |
| `to_connection_string()` | `str` | URI string, e.g. `"tcp://127.0.0.1:18812"` |

### TransportScheme

Strategy enum for selecting the optimal transport type for a connection:

| Variant | Description |
|---------|-------------|
| `TransportScheme.AUTO` | Platform-optimal: Named Pipe on Windows, Unix Socket on Linux/macOS |
| `TransportScheme.TCP_ONLY` | Always use TCP |
| `TransportScheme.PREFER_NAMED_PIPE` | Named Pipe if same machine, TCP otherwise |
| `TransportScheme.PREFER_UNIX_SOCKET` | Unix socket if same machine, TCP otherwise |
| `TransportScheme.PREFER_IPC` | Any local IPC transport |

```python
from dcc_mcp_core import TransportScheme, TransportAddress

scheme = TransportScheme.AUTO
addr = scheme.select_address("maya", "127.0.0.1", 18812, pid=12345)
```

### IpcListener

Server-side listener. Used inside DCC plugins to accept incoming connections.

```python
from dcc_mcp_core import IpcListener, TransportAddress

# Bind to a transport address (port 0 = OS-assigned)
addr = TransportAddress.tcp("127.0.0.1", 0)
listener = IpcListener.bind(addr)

# Get the actual bound address (useful when port=0)
local_addr = listener.local_address()
print(f"Listening on {local_addr}")   # tcp://127.0.0.1:54321

# Accept a connection (blocking)
channel = listener.accept(timeout_ms=5000)  # → FramedChannel

# Or convert to a handle for connection tracking
handle = listener.into_handle()   # consumes listener; can only call once
```

| Method | Returns | Description |
|--------|---------|-------------|
| `IpcListener.bind(addr)` | `IpcListener` | Bind to address. Raises `RuntimeError` if port in use |
| `local_address()` | `TransportAddress` | Actual bound address |
| `transport_name` | `str` | `"tcp"`, `"named_pipe"`, or `"unix_socket"` |
| `accept(timeout_ms=None)` | `FramedChannel` | Accept next connection. Blocks until client connects |
| `into_handle()` | `ListenerHandle` | Wrap in a handle with connection tracking (consumes `self`) |

### ListenerHandle

Wraps `IpcListener` with connection tracking and shutdown control.

```python
from dcc_mcp_core import IpcListener, TransportAddress

addr = TransportAddress.default_local("maya", pid=12345)
listener = IpcListener.bind(addr)
handle = listener.into_handle()

print(handle.accept_count)   # 0
print(handle.is_shutdown)    # False

# Request shutdown (stop accepting new connections)
handle.shutdown()
```

| Property/Method | Returns | Description |
|-----------------|---------|-------------|
| `accept_count` | `int` | Connections accepted so far |
| `is_shutdown` | `bool` | Whether shutdown has been requested |
| `transport_name` | `str` | Transport type name |
| `local_address()` | `TransportAddress` | Bound address |
| `shutdown()` | `None` | Stop accepting new connections (idempotent) |

### FramedChannel

Full-duplex framed channel with a background reader loop. Handles Ping/Pong heartbeats automatically. Obtain via `IpcListener.accept()` (server) or `connect_ipc()` (client).

```python
from dcc_mcp_core import connect_ipc, TransportAddress

# Client-side: connect to a running DCC server
addr = TransportAddress.tcp("127.0.0.1", 18812)
channel = connect_ipc(addr, timeout_ms=10000)

# Liveness check
rtt_ms = channel.ping()          # int, round-trip time in ms

# Receive (blocking)
msg = channel.recv(timeout_ms=5000)
# msg: dict with "type" field → "request", "response", or "notify"

# Non-blocking receive
msg = channel.try_recv()         # None if buffer empty

# Send
req_id = channel.send_request("execute_python", params=b'{"code":"..."}')
channel.send_response(req_id, success=True, payload=b'{"result":1}')
channel.send_notify("scene_changed", data=b'{"scene":"untitled"}')

# Shutdown
channel.shutdown()
print(channel.is_running)        # False
```

| Method | Returns | Description |
|--------|---------|-------------|
| `recv(timeout_ms=None)` | `dict \| None` | Blocking receive. Returns `None` on timeout or close |
| `try_recv()` | `dict \| None` | Non-blocking receive. Returns `None` if buffer empty |
| `ping(timeout_ms=5000)` | `int` | Heartbeat ping; returns RTT ms. Data messages not lost |
| `send_request(method, params=None)` | `str` | Send request; returns UUID request ID |
| `send_response(request_id, success, payload=None, error=None)` | `None` | Send response for a request |
| `send_notify(topic, data=None)` | `None` | Send a one-way notification |
| `shutdown()` | `None` | Graceful shutdown (idempotent) |
| `is_running` | `bool` | Whether the background reader is still running |

### connect_ipc

Client-side connection factory:

```python
from dcc_mcp_core import connect_ipc, TransportAddress

channel = connect_ipc(
    addr=TransportAddress.tcp("127.0.0.1", 18812),
    timeout_ms=10000,    # default: 10000 ms
)
```

Raises `RuntimeError` if the connection cannot be established within the timeout.

### RoutingStrategy

Strategy for selecting a DCC instance when multiple are registered:

| Variant | Description |
|---------|-------------|
| `FIRST_AVAILABLE` | Pick the first reachable instance |
| `ROUND_ROBIN` | Cycle through instances |
| `LEAST_BUSY` | Instance with lowest session request count |
| `SPECIFIC` | Requires an explicit `instance_id` |
| `SCENE_MATCH` | Match by open scene name |
| `RANDOM` | Random instance selection |

### ServiceStatus

Enum for DCC service health:

| Variant | Meaning |
|---------|---------|
| `AVAILABLE` | Ready to accept requests |
| `BUSY` | Processing; may accept more |
| `UNREACHABLE` | Not responding to heartbeats |
| `SHUTTING_DOWN` | Graceful shutdown in progress |

---

## End-to-End Example: DCC Plugin Server

```python
# Inside a Maya plugin (server side)
import maya.cmds as cmds
from dcc_mcp_core import IpcListener, TransportAddress
import threading, os

addr = TransportAddress.default_local("maya", os.getpid())
listener = IpcListener.bind(addr)
print(f"Maya IPC server: {listener.local_address()}")

def serve():
    channel = listener.accept()
    while True:
        msg = channel.recv(timeout_ms=1000)
        if msg is None:
            break
        if msg["type"] == "request":
            result = cmds.ls()
            channel.send_response(msg["id"], success=True,
                                  payload=str(result).encode())

threading.Thread(target=serve, daemon=True).start()
```

```python
# Client side (MCP agent)
from dcc_mcp_core import connect_ipc, TransportAddress

addr = TransportAddress.default_local("maya", pid=12345)
channel = connect_ipc(addr)
req_id = channel.send_request("ls")
response = channel.recv()
# response["type"] == "response", response["payload"] == b"[...]"
channel.shutdown()
```
