# Transport API

`dcc_mcp_core.TransportManager`

## TransportManager

Python-facing wrapper for the Rust transport layer. Bridges async operations to synchronous calls via an internal Tokio runtime.

### Constructor

```python
TransportManager(
    registry_dir: str,
    max_connections_per_dcc: int = 10,
    idle_timeout: int = 300,
    heartbeat_interval: int = 5,
    connect_timeout: int = 10,
    reconnect_max_retries: int = 3,
)
```

### Service Discovery

| Method | Returns | Description |
|--------|---------|-------------|
| `register_service(dcc_type, host, port, version=None, scene=None, metadata=None)` | `str` | Register a service, returns instance_id (UUID) |
| `deregister_service(dcc_type, instance_id)` | `bool` | Deregister a service by key |
| `list_instances(dcc_type)` | `List[dict]` | List all instances for a DCC type |
| `list_all_services()` | `List[dict]` | List all registered services |
| `heartbeat(dcc_type, instance_id)` | `bool` | Update heartbeat timestamp |

### Session Management

| Method | Returns | Description |
|--------|---------|-------------|
| `get_or_create_session(dcc_type, instance_id=None)` | `str` | Get/create a session (UUID). If no instance_id, picks first available |
| `get_session(session_id)` | `dict?` | Get session info dict, or None |
| `record_success(session_id, latency_ms)` | — | Record successful request |
| `record_error(session_id, latency_ms, error)` | — | Record failed request |
| `begin_reconnect(session_id)` | `int` | Begin reconnection, returns backoff in ms |
| `reconnect_success(session_id)` | — | Mark reconnection as successful |
| `close_session(session_id)` | `bool` | Close a session |
| `list_sessions()` | `List[dict]` | List all active sessions |
| `session_count()` | `int` | Number of active sessions |

### Connection Pool

| Method | Returns | Description |
|--------|---------|-------------|
| `acquire_connection(dcc_type, instance_id=None)` | `str` | Acquire connection (UUID) |
| `release_connection(dcc_type, instance_id)` | — | Release connection back to pool |
| `pool_size()` | `int` | Total connections in pool |

### Lifecycle

| Method | Returns | Description |
|--------|---------|-------------|
| `cleanup()` | `(int, int, int)` | Returns (stale_services, closed_sessions, evicted_connections) |
| `shutdown()` | — | Graceful shutdown |
| `is_shutdown()` | `bool` | Check if transport is shut down |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__repr__` | `TransportManager(services=N, sessions=N, pool=N)` |
| `__len__` | Returns session count |

## Rust-Only Types

The following types are available in Rust but not directly exposed to Python:

### TransportConfig

| Field | Type | Default |
|-------|------|---------|
| `pool` | `PoolConfig` | — |
| `session` | `SessionConfig` | — |
| `connect_timeout` | `Duration` | 10s |
| `heartbeat_interval` | `Duration` | 5s |

### PoolConfig

| Field | Type | Default |
|-------|------|---------|
| `max_connections_per_type` | `usize` | 10 |
| `max_idle_time` | `Duration` | 300s |
| `max_lifetime` | `Duration` | 3600s |
| `acquire_timeout` | `Duration` | 30s |

### SessionConfig

| Field | Type | Default |
|-------|------|---------|
| `idle_timeout` | `Duration` | 300s |
| `reconnect_max_retries` | `u32` | 3 |
| `reconnect_backoff_base` | `Duration` | 1s |
| `max_session_lifetime` | `Duration` | 3600s |
| `heartbeat_interval` | `Duration` | 5s |

### TransportError

| Variant | Description |
|---------|-------------|
| `ConnectionFailed` | TCP connection failed |
| `ConnectionTimeout` | Connection timed out |
| `PoolExhausted` | All connections in use |
| `AcquireTimeout` | Timeout waiting for pooled connection |
| `ServiceNotFound` | Service not in registry |
| `ServiceAlreadyRegistered` | Duplicate registration |
| `Serialization` | MessagePack serialization error |
| `Io` | IO error |
| `RegistryFile` | Registry file error |
| `Shutdown` | Transport is shut down |
| `SessionNotFound` | Session not found |
| `InvalidSessionState` | Invalid state transition |
| `ReconnectionFailed` | Max retries exceeded |
| `Internal` | Generic internal error |

### ServiceStatus

| Value | Description |
|-------|-------------|
| `Available` | Accepting connections (default) |
| `Busy` | Processing a request |
| `Unreachable` | Health check failed |
| `ShuttingDown` | Shutting down |

### SessionState

| Value | Description |
|-------|-------------|
| `Connected` | Ready for requests |
| `Idle` | Idle timeout exceeded |
| `Reconnecting` | Reconnecting after failure |
| `Closed` | Terminal state |

### Wire Protocol

Messages use MessagePack serialization with a 4-byte big-endian length prefix:

```
[4-byte length][MessagePack payload]
```

- **Request**: `{ id: UUID, method: String, params: Vec<u8> }`
- **Response**: `{ id: UUID, success: bool, payload: Vec<u8>, error: Option<String> }`
