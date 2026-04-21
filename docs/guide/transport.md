# Transport Layer

> **v0.14 replaced the legacy transport stack (issue #251).**
>
> The old classes — `TransportManager`, `FramedChannel`, `FramedIo`,
> `IpcListener` (Python), `ListenerHandle`, `RoutingStrategy`,
> `ConnectionPool`, `InstanceRouter`, `CircuitBreaker`, `MessageEnvelope`,
> `encode_request` / `encode_response` / `encode_notify` / `decode_envelope`,
> `connect_ipc` — have been removed. Use the DccLink adapters built on
> `ipckit` documented below.

The Transport layer (`dcc-mcp-transport` crate) provides IPC communication between MCP servers and DCC application instances using DccLink framing over Named Pipes (Windows) or Unix Domain Sockets (macOS/Linux).

## Overview

The new transport API is built around **DccLink adapters** — thin wrappers over `ipckit` IPC channels that use a binary wire format (`[u32 len][u8 type][u64 seq][msgpack body]`) for efficient framed communication.

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

# Server side: create a named channel and wait for a client
server = IpcChannelAdapter.create("my-dcc")
server.wait_for_client()

# Client side: connect to the server
client = IpcChannelAdapter.connect("my-dcc")

# Send a frame
frame = DccLinkFrame(msg_type=1, seq=0, body=b"hello")
client.send_frame(frame)

# Receive a frame
received = server.recv_frame()
print(received.body)  # b"hello"
```

## DccLinkFrame

Binary wire frame for DCC-Link protocol. Wire format: `[u32 len][u8 type][u64 seq][msgpack body]`.

### Message Types

| Tag | Type | Description |
|-----|------|-------------|
| 1 | Call | Request invocation |
| 2 | Reply | Successful response |
| 3 | Err | Error response |
| 4 | Progress | Progress update |
| 5 | Cancel | Cancellation signal |
| 6 | Push | Server-pushed message |
| 7 | Ping | Heartbeat request |
| 8 | Pong | Heartbeat response |

### Constructor

```python
from dcc_mcp_core import DccLinkFrame

frame = DccLinkFrame(msg_type=1, seq=0, body=b"hello")
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `msg_type` | `int` | Message type tag (1-8) |
| `seq` | `int` | Sequence number |
| `body` | `bytes \| None` | Payload bytes (defaults to `b""`) |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `msg_type` | `int` | Message type tag (1=Call, 2=Reply, 3=Err, 4=Progress, 5=Cancel, 6=Push, 7=Ping, 8=Pong) |
| `seq` | `int` | Sequence number |
| `body` | `bytes` | Payload bytes |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `encode()` | `bytes` | Encode the frame to `[len][type][seq][body]` bytes |
| `decode(data)` | `DccLinkFrame` | Decode a frame from bytes including the 4-byte length prefix (static) |

```python
frame = DccLinkFrame(msg_type=1, seq=0, body=b"payload")
encoded = frame.encode()
decoded = DccLinkFrame.decode(encoded)
assert decoded.msg_type == frame.msg_type
assert decoded.seq == frame.seq
assert decoded.body == frame.body
```

## IpcChannelAdapter

Thin adapter over `ipckit::IpcChannel` using DCC-Link framing. Supports 1:1 connections over Named Pipes (Windows) or Unix Domain Sockets (macOS/Linux).

### Creating a Server

```python
from dcc_mcp_core import IpcChannelAdapter

server = IpcChannelAdapter.create("my-dcc")
server.wait_for_client()  # blocks until a client connects
```

### Connecting as a Client

```python
from dcc_mcp_core import IpcChannelAdapter

client = IpcChannelAdapter.connect("my-dcc")
```

### Sending and Receiving Frames

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

# Server side
server = IpcChannelAdapter.create("my-dcc")
server.wait_for_client()

# Client side
client = IpcChannelAdapter.connect("my-dcc")

# Client sends a Call frame
call_frame = DccLinkFrame(msg_type=1, seq=0, body=b"execute_python")
client.send_frame(call_frame)

# Server receives the frame
received = server.recv_frame()  # blocking; returns None if channel closed
if received is not None:
    print(received.msg_type)  # 1
    print(received.body)      # b"execute_python"

    # Server sends a Reply frame
    reply = DccLinkFrame(msg_type=2, seq=0, body=b"ok")
    server.send_frame(reply)

# Client receives the reply
response = client.recv_frame()
```

### Static Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `create(name)` | `IpcChannelAdapter` | Create a server-side IPC channel |
| `connect(name)` | `IpcChannelAdapter` | Connect to an existing IPC channel |

### Instance Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `wait_for_client()` | `None` | Wait for a client to connect (server-side only) |
| `send_frame(frame)` | `None` | Send a `DccLinkFrame` to the peer |
| `recv_frame()` | `DccLinkFrame \| None` | Receive a frame (blocking). Returns `None` if channel closed |

## GracefulIpcChannelAdapter

Extends `IpcChannelAdapter` with graceful shutdown and DCC main-thread integration. Use this in DCC plugins that need to process IPC messages on the main thread without blocking.

### Creating a Graceful Server

```python
from dcc_mcp_core import GracefulIpcChannelAdapter

server = GracefulIpcChannelAdapter.create("my-dcc")
server.bind_affinity_thread()  # call once on the DCC main thread
server.wait_for_client()
```

### Pumping Messages on the Main Thread

In DCC applications, IPC messages must often be processed on the main thread. Use `pump_pending()` from an idle callback:

```python
# Maya example: use scriptJob idleEvent
import maya.cmds as cmds

def on_idle():
    processed = server.pump_pending(budget_ms=50)
    # returns number of items processed

cmds.scriptJob(idleEvent="python(\"on_idle()\")")
```

### Graceful Shutdown

```python
server.shutdown()  # signals the channel to shut down gracefully
```

### Static Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `create(name)` | `GracefulIpcChannelAdapter` | Create a server-side graceful IPC channel |
| `connect(name)` | `GracefulIpcChannelAdapter` | Connect to an existing graceful IPC channel |

### Instance Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `wait_for_client()` | `None` | Wait for a client to connect (server-side only) |
| `send_frame(frame)` | `None` | Send a `DccLinkFrame` to the peer |
| `recv_frame()` | `DccLinkFrame \| None` | Receive a frame (blocking). Returns `None` if channel closed |
| `shutdown()` | `None` | Signal the channel to shut down gracefully |
| `bind_affinity_thread()` | `None` | Bind the current thread as the affinity thread. Call **once** on the DCC main thread |
| `pump_pending(budget_ms=100)` | `int` | Drain pending work items on the affinity thread within the budget. Returns items processed |

## SocketServerAdapter

Multi-client IPC server using Unix Domain Sockets (macOS/Linux) or Named Pipes (Windows). Supports a bounded connection pool.

### Creating a Socket Server

```python
from dcc_mcp_core import SocketServerAdapter

server = SocketServerAdapter(
    path="/tmp/my-dcc.sock",  # Unix socket path or Windows pipe name
    max_connections=10,        # maximum concurrent connections
    connection_timeout_ms=30000,  # connection timeout in ms
)

print(server.socket_path)      # the path this server is listening on
print(server.connection_count) # number of currently connected clients

server.shutdown()  # gracefully shut down
```

### Constructor

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `path` | `str` | — | Socket path (Unix) or pipe name (Windows) |
| `max_connections` | `int` | `10` | Maximum concurrent connections |
| `connection_timeout_ms` | `int` | `30000` | Connection timeout in milliseconds |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `socket_path` | `str` | The socket path this server is listening on |
| `connection_count` | `int` | Number of currently connected clients |

### Instance Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `shutdown()` | `None` | Gracefully shut down the server (blocks until stopped) |
| `signal_shutdown()` | `None` | Signal shutdown without blocking |

## Transport Helpers

### TransportAddress

Protocol-agnostic transport endpoint. Supports TCP, Named Pipes (Windows), and Unix Domain Sockets (macOS/Linux).

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

Strategy for choosing the optimal communication channel:

| Constant | Description |
|----------|-------------|
| `AUTO` | Auto-select best transport (Named Pipe on Windows, Unix Socket on *nix) |
| `TCP_ONLY` | Always use TCP |
| `PREFER_NAMED_PIPE` | Prefer Named Pipe, fall back to TCP |
| `PREFER_UNIX_SOCKET` | Prefer Unix Socket, fall back to TCP |
| `PREFER_IPC` | Prefer any IPC, fall back to TCP |

```python
from dcc_mcp_core import TransportScheme

addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812, pid=12345)
```

### ServiceEntry

Represents a discovered DCC service instance.

| Property | Type | Description |
|----------|------|-------------|
| `dcc_type` | `str` | DCC application type (e.g. `"maya"`) |
| `instance_id` | `str` | UUID string |
| `host` | `str` | Host address |
| `port` | `int` | TCP port |
| `version` | `str \| None` | DCC version |
| `scene` | `str \| None` | Currently open scene/file |
| `metadata` | `dict[str, str]` | Arbitrary string-only metadata |
| `extras` | `dict[str, Any]` | JSON-typed DCC metadata |
| `status` | `ServiceStatus` | Instance status |
| `transport_address` | `TransportAddress \| None` | Preferred IPC address |
| `last_heartbeat_ms` | `int` | Last heartbeat timestamp (Unix ms) |

### ServiceStatus

DCC service health status:

| Constant | Description |
|----------|-------------|
| `AVAILABLE` | Ready to accept requests |
| `BUSY` | Processing; may accept more |
| `UNREACHABLE` | Not responding to heartbeats |
| `SHUTTING_DOWN` | Graceful shutdown in progress |

---

## End-to-End Example

### DCC Plugin (Server)

```python
# Inside a Maya plugin
from dcc_mcp_core import GracefulIpcChannelAdapter, DccLinkFrame

server = GracefulIpcChannelAdapter.create("maya-ipc")
server.bind_affinity_thread()  # call once on main thread
server.wait_for_client()

# In Maya idle callback:
def on_idle():
    processed = server.pump_pending(budget_ms=50)

# Main message loop
while True:
    frame = server.recv_frame()
    if frame is None:
        break  # channel closed
    if frame.msg_type == 1:  # Call
        # Process the request...
        reply = DccLinkFrame(msg_type=2, seq=frame.seq, body=b"ok")
        server.send_frame(reply)

server.shutdown()
```

### MCP Agent (Client)

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

client = IpcChannelAdapter.connect("maya-ipc")

# Send a Call frame
call = DccLinkFrame(msg_type=1, seq=0, body=b"get_scene_info")
client.send_frame(call)

# Receive the Reply
reply = client.recv_frame()
if reply and reply.msg_type == 2:
    print(f"Result: {reply.body}")
```
