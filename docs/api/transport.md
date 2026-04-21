# Transport API

`dcc_mcp_core` — DccLinkFrame, IpcChannelAdapter, GracefulIpcChannelAdapter, SocketServerAdapter, TransportAddress, TransportScheme, ServiceEntry, ServiceStatus.

## Overview

The transport module provides **DccLink-based IPC communication** between AI agents and DCC applications. Key design decisions:

- **Named Pipes** (Windows) and **Unix Domain Sockets** (macOS/Linux) are preferred for same-machine connections — sub-millisecond latency, zero configuration.
- DccLink adapters wrap `ipckit` channels with a binary wire format: `[u32 len][u8 type][u64 seq][msgpack body]`.
- `IpcChannelAdapter.create(name)` + `wait_for_client()` is the recommended server setup.
- `IpcChannelAdapter.connect(name)` is the client-side entry point.
- `GracefulIpcChannelAdapter` adds graceful shutdown and DCC main-thread integration.
- `SocketServerAdapter` provides multi-client connections with a bounded connection pool.

## DccLinkFrame

A DCC-Link frame with `msg_type`, `seq`, and `body` fields.

Wire format: `[u32 len][u8 type][u64 seq][msgpack body]`.

Message type tags: 1=Call, 2=Reply, 3=Err, 4=Progress, 5=Cancel, 6=Push, 7=Ping, 8=Pong.

### Constructor

```python
from dcc_mcp_core import DccLinkFrame

frame = DccLinkFrame(msg_type=1, seq=0, body=b"hello")
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `msg_type` | `int` | — | Message type tag (1-8). Raises `ValueError` if invalid. |
| `seq` | `int` | — | Sequence number. |
| `body` | `bytes \| None` | `None` | Payload bytes. |

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
| `decode(data)` | `DccLinkFrame` | Decode a frame from bytes including the 4-byte length prefix (static). Raises `RuntimeError` if malformed. |

### Example

```python
frame = DccLinkFrame(msg_type=1, seq=0, body=b"payload")
encoded = frame.encode()
decoded = DccLinkFrame.decode(encoded)
assert decoded.msg_type == frame.msg_type
assert decoded.seq == frame.seq
assert decoded.body == frame.body
```

## IpcChannelAdapter

Thin adapter over `ipckit::IpcChannel` using DCC-Link framing. Provides 1:1 framed IPC connections via Named Pipes (Windows) or Unix Domain Sockets (macOS/Linux).

### Static Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `create(name)` | `IpcChannelAdapter` | Create a server-side IPC channel. Raises `RuntimeError` if creation fails. |
| `connect(name)` | `IpcChannelAdapter` | Connect to an existing IPC channel. Raises `RuntimeError` if connection fails. |

### Instance Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `wait_for_client()` | `None` | Wait for a client to connect (server-side only). Raises `RuntimeError` if the wait fails. |
| `send_frame(frame)` | `None` | Send a `DccLinkFrame` to the peer. Raises `RuntimeError` if the send fails. |
| `recv_frame()` | `DccLinkFrame \| None` | Receive a DCC-Link frame (blocking). Returns `None` if the channel is closed. Raises `RuntimeError` on unexpected errors. |

### Example: Server

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

server = IpcChannelAdapter.create("my-dcc")
server.wait_for_client()

frame = server.recv_frame()
if frame is not None:
    reply = DccLinkFrame(msg_type=2, seq=frame.seq, body=b"result")
    server.send_frame(reply)
```

### Example: Client

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

client = IpcChannelAdapter.connect("my-dcc")
call = DccLinkFrame(msg_type=1, seq=0, body=b"request")
client.send_frame(call)

reply = client.recv_frame()
if reply is not None:
    print(reply.body)
```

## GracefulIpcChannelAdapter

Graceful IPC channel adapter with shutdown and affinity-pump support. Extends `IpcChannelAdapter` with graceful shutdown and `bind_affinity_thread` / `pump_pending` for integrating with DCC main-thread idle callbacks.

For reentrancy-safe Python dispatch, prefer `DeferredExecutor` from `dcc_mcp_core._core` instead of `submit()`.

### Static Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `create(name)` | `GracefulIpcChannelAdapter` | Create a server-side graceful IPC channel. Raises `RuntimeError` if creation fails. |
| `connect(name)` | `GracefulIpcChannelAdapter` | Connect to an existing graceful IPC channel. Raises `RuntimeError` if connection fails. |

### Instance Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `wait_for_client()` | `None` | Wait for a client to connect (server-side only). Raises `RuntimeError` if the wait fails. |
| `send_frame(frame)` | `None` | Send a `DccLinkFrame` to the peer. Raises `RuntimeError` if the send fails. |
| `recv_frame()` | `DccLinkFrame \| None` | Receive a DCC-Link frame (blocking). Returns `None` if the channel is closed. Raises `RuntimeError` on unexpected errors. |
| `shutdown()` | `None` | Signal the channel to shut down gracefully. |
| `bind_affinity_thread()` | `None` | Bind the current thread as the affinity thread for reentrancy-safe dispatch. Call **once** on the DCC main thread. |
| `pump_pending(budget_ms=100)` | `int` | Drain pending work items on the affinity thread within the budget. Call from DCC host idle callback. Returns number of items processed. |

### Example

```python
from dcc_mcp_core import GracefulIpcChannelAdapter, DccLinkFrame

server = GracefulIpcChannelAdapter.create("my-dcc")
server.bind_affinity_thread()
server.wait_for_client()

# In DCC idle callback:
# processed = server.pump_pending(budget_ms=50)

frame = server.recv_frame()
if frame is not None:
    reply = DccLinkFrame(msg_type=2, seq=frame.seq, body=b"ok")
    server.send_frame(reply)

server.shutdown()
```

## SocketServerAdapter

Minimal wrapper for `ipckit::SocketServer` (multi-client Unix socket / named pipe). Supports a bounded connection pool.

### Constructor

```python
from dcc_mcp_core import SocketServerAdapter

server = SocketServerAdapter(
    path="/tmp/my-dcc.sock",
    max_connections=10,
    connection_timeout_ms=30000,
)
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `path` | `str` | — | Socket path (Unix) or pipe name (Windows). Raises `RuntimeError` if creation fails. |
| `max_connections` | `int` | `10` | Maximum concurrent connections. |
| `connection_timeout_ms` | `int` | `30000` | Connection timeout in milliseconds. |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `socket_path` | `str` | The socket path this server is listening on. |
| `connection_count` | `int` | Number of currently connected clients. |

### Instance Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `shutdown()` | `None` | Gracefully shut down the server (blocks until stopped). |
| `signal_shutdown()` | `None` | Signal shutdown without blocking. |

## TransportAddress

Protocol-agnostic transport endpoint for DCC communication. Supports TCP, Named Pipes (Windows), and Unix Domain Sockets (macOS/Linux).

### Static Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `tcp(host, port)` | `TransportAddress` | Create a TCP transport address |
| `named_pipe(name)` | `TransportAddress` | Create a Named Pipe transport address (Windows) |
| `unix_socket(path)` | `TransportAddress` | Create a Unix Domain Socket transport address |
| `default_local(dcc_type, pid)` | `TransportAddress` | Generate optimal local transport for the current platform |
| `default_pipe_name(dcc_type, pid)` | `TransportAddress` | Generate a default Named Pipe name for a DCC instance |
| `default_unix_socket(dcc_type, pid)` | `TransportAddress` | Generate a default Unix Socket path for a DCC instance |
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

addr = TransportAddress.default_local("maya", os.getpid())
print(addr.scheme)   # "pipe" on Windows, "unix" on macOS/Linux
print(addr.is_local) # True
```

## TransportScheme

Transport selection strategy for choosing the optimal communication channel.

### Constants

| Constant | Description |
|----------|-------------|
| `AUTO` | Auto-select best transport (Named Pipe on Windows, Unix Socket on *nix) |
| `TCP_ONLY` | Always use TCP |
| `PREFER_NAMED_PIPE` | Prefer Named Pipe, fall back to TCP |
| `PREFER_UNIX_SOCKET` | Prefer Unix Socket, fall back to TCP |
| `PREFER_IPC` | Prefer any IPC (Pipe or Unix Socket), fall back to TCP |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `select_address(dcc_type, host, port, pid=None)` | `TransportAddress` | Select optimal transport address |

```python
from dcc_mcp_core import TransportScheme

addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812, pid=12345)
```

## ServiceEntry

Represents a discovered DCC service instance.

### Attributes

| Attribute | Type | Description |
|-----------|------|-------------|
| `dcc_type` | `str` | DCC application type (e.g. `"maya"`) |
| `instance_id` | `str` | UUID string |
| `host` | `str` | Host address |
| `port` | `int` | TCP port |
| `version` | `str \| None` | DCC version |
| `scene` | `str \| None` | Currently open scene/file |
| `documents` | `list[str]` | Open documents |
| `pid` | `int \| None` | Process ID |
| `display_name` | `str \| None` | Display name |
| `metadata` | `dict[str, str]` | Arbitrary string-only metadata |
| `status` | `ServiceStatus` | Instance status |
| `transport_address` | `TransportAddress \| None` | Preferred IPC address |
| `last_heartbeat_ms` | `int` | Last heartbeat timestamp (Unix ms) |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `extras` | `dict[str, Any]` | Arbitrary DCC-specific extras with JSON-typed values. Unlike `metadata` (string-only), `extras` allows nested objects / arrays / numbers / booleans. Returns a fresh dict — mutating it does not update the registry. |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `effective_address()` | `TransportAddress` | IPC address or TCP fallback |
| `to_dict()` | `dict` | Serialize to dict |

## ServiceStatus

Enum for DCC service instance status.

### Constants

| Constant | Description |
|----------|-------------|
| `AVAILABLE` | Accepting connections (default) |
| `BUSY` | Processing a request |
| `UNREACHABLE` | Health check failed |
| `SHUTTING_DOWN` | Shutting down |

## Wire Protocol

DccLink frames use the following binary wire format:

```
[u32 len][u8 type][u64 seq][msgpack body]
```

- `len` — 4-byte big-endian total frame length (including type + seq + body)
- `type` — 1-byte message type tag (1-8)
- `seq` — 8-byte big-endian sequence number
- `body` — MessagePack-encoded payload

Message types:

| Tag | Type | Direction | Description |
|-----|------|-----------|-------------|
| 1 | Call | Client → Server | Request invocation |
| 2 | Reply | Server → Client | Successful response |
| 3 | Err | Server → Client | Error response |
| 4 | Progress | Server → Client | Progress update |
| 5 | Cancel | Client → Server | Cancellation signal |
| 6 | Push | Server → Client | Server-pushed message |
| 7 | Ping | Either | Heartbeat request |
| 8 | Pong | Either | Heartbeat response |
