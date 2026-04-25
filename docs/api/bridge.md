# Bridge API

Generic WebSocket bridge for non-Python DCCs. Implements the server-side of the dcc-mcp-core WebSocket JSON-RPC 2.0 bridge protocol.

**Exported symbols:** `DccBridge`, `BridgeError`, `BridgeConnectionError`, `BridgeTimeoutError`, `BridgeRpcError`

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
