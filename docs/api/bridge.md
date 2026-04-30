# Bridge API

Generic WebSocket bridge for non-Python DCCs. Implements the server-side of the dcc-mcp-core WebSocket JSON-RPC 2.0 bridge protocol.

**Exported symbols:** `DccBridge`, `BridgeError`, `BridgeConnectionError`,
`BridgeTimeoutError`, `BridgeRpcError`, `BridgeRetryPolicy`,
`BridgeTransportStrategy`, `BridgeFallbackClient`, `ReverseBridgeRequest`,
`ReverseBridgeSession`

## DccBridge

WebSocket bridge server that waits for a DCC plugin to connect.

### Constructor

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `host` | `str` | `"localhost"` | Bind address for the WebSocket server |
| `port` | `int` | `9001` | Port for the WebSocket server |
| `timeout` | `float` | `30.0` | Default timeout in seconds for `call()` |
| `server_name` | `str` | `"dcc-mcp-server"` | Name advertised in the hello_ack handshake |
| `server_version` | `str \| None` | package version | Version advertised in hello_ack |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `endpoint` | `str` | WebSocket endpoint URL (e.g. `"ws://localhost:9001"`) |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `connect(wait_for_dcc=False)` | `None` | Start the WebSocket server |
| `call(method, **params)` | `Any` | Synchronous RPC to the DCC plugin (thread-safe) |
| `disconnect()` | `None` | Shut down the WebSocket server |
| `is_connected()` | `bool` | Whether a DCC plugin has completed the handshake |

```python
from dcc_mcp_core import DccBridge

# Context manager
with DccBridge(port=9001) as bridge:
    info = bridge.call("ps.getDocumentInfo")
    layers = bridge.call("ps.listLayers", include_hidden=True)
```

## Exceptions

| Exception | Parent | Description |
|-----------|--------|-------------|
| `BridgeError` | `Exception` | Base class for all DccBridge errors |
| `BridgeConnectionError` | `BridgeError` | DCC plugin not connected or connection lost |
| `BridgeTimeoutError` | `BridgeError` | Call timed out |
| `BridgeRpcError` | `BridgeError` | DCC plugin returned JSON-RPC error; attributes: `.code`, `.message`, `.data` |

## Resilience And Fallback

Adapters with multiple bridge paths can wrap them in a fallback client:

```python
from dcc_mcp_core import BridgeFallbackClient, BridgeRetryPolicy, BridgeTransportStrategy

class WebSocketTransport(BridgeTransportStrategy):
    name = "websocket"
    def connect(self): ...
    def disconnect(self): ...
    def is_connected(self): ...
    def call(self, method, **params): ...

client = BridgeFallbackClient(
    [WebSocketTransport(), NamedPipeTransport()],
    retry_policy=BridgeRetryPolicy(attempts=3, initial_delay_secs=0.1),
)
result = client.call("scene.info")
```

`BridgeRetryPolicy` centralizes attempts and exponential backoff so DCC
adapters do not each invent slightly different retry loops.

## Reverse Bridge Sessions

Some plugin runtimes cannot host a listener or keep inbound sockets open. For
those cases, the host can enqueue work and let the plugin poll:

```python
from dcc_mcp_core import ReverseBridgeSession

session = ReverseBridgeSession(timeout=30)

# Host side
result = session.call("ps.document.info", include_layers=True)

# Plugin side
request = session.next_request(timeout=1.0)
if request is not None:
    response = run_in_host(request.method, **request.params)
    session.submit_response(request.id, result=response)
```

The request envelope can be serialized with `request.to_jsonrpc()` when the
polling transport is HTTP, a named pipe, or an application-specific queue.
