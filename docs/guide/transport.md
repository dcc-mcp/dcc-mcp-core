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
session_id = transport.get_or_create_session("maya", instance_id=id1)

session = transport.get_session(session_id)

transport.record_success(session_id, latency_ms=50)
transport.record_error(session_id, latency_ms=100, error="timeout")

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
