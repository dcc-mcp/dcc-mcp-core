# Bridge API

Generic WebSocket bridge for non-Python DCCs. Implements the server-side of the dcc-mcp-core WebSocket JSON-RPC 2.0 bridge protocol.

**Exported symbols:** `DccBridge`, `BridgeError`, `BridgeConnectionError`,
`BridgeTimeoutError`, `BridgeRpcError`, `BridgeRetryPolicy`,
`BridgeTransportStrategy`, `BridgeFallbackClient`, `ReverseBridgeRequest`,
`ReverseBridgeSession`

## DccBridge

Install the WebSocket transport before creating a bridge:

```bash
pip install 'dcc-mcp-core[bridge]'
```

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

`BridgeRetryPolicy` controls connection-establishment attempts and exponential
backoff in this client. It is not an operation-replay policy.

Bridge errors expose a request-level `outcome`: `unknown` (the default) or
`not_dispatched`. `BridgeFallbackClient` replays a call on another transport
only when its `BridgeConnectionError` explicitly reports `not_dispatched`.
Existing unclassified connection errors, send failures after possible dispatch,
and timeouts must be surfaced to the caller for state inspection, without
replay. Do not infer safety from a method name or a fresh request ID. A strategy
may report `not_dispatched` only with evidence that this request never reached
its dispatch boundary; it says nothing about earlier workflow steps.

`DccBridge` serializes before registering pending work, binds each send to its
selected socket, and settles send failures promptly. Serialization/scheduling
failures are pre-dispatch; a failing WebSocket send has unknown outcome. A
queued request that already timed out is not sent later. Pending state is
removed on every call exit. The experimental design bridge's `indeterminate`
means `unknown` here; this change does not alter that bridge's envelope.

The shared v1 capacity contract, final UTF-8 byte budgets, and Rust/Python
conformance vectors are delivered below. No receive limit or adapter
authorization policy changes in this outcome/fallback slice.

## Shared Capacity Contract (v1)

Issue #2821 defines an additive, contract-only surface shared by Python and
Rust. It is not a runtime handshake. An endpoint describes its bounded receive
capacity in the existing `hello.capabilities` map under
`dcc-mcp.capacity.v1`. Each direction is independent:

```json
{
  "dcc-mcp.capacity.v1": {
    "version": 1,
    "receive": {"incoming": 131072, "outgoing": 16384}
  }
}
```

The legacy flat form `{"receive_max_bytes": 65535}` remains readable. An
absent declaration resolves to the documented 65,535-byte legacy profile; an
alternate key such as `transport.max_message_bytes` does not participate in
this contract. Declared values are clamped to the local policy, so a peer
cannot raise a local hard limit. The contract rejects booleans, strings,
non-positive values, conflicting receive shapes, and unsupported versions.

The byte budget covers the complete final serialized UTF-8 JSON message,
including escaping, request IDs, result/error envelopes, and added metadata.
Callers serialize once and check the exact bytes they intend to send. The
Python module `dcc_mcp_core.wire.capacity` and the Rust module
`dcc_mcp_protocols::capacity_contract` provide declaration parsing, policy
clamping, byte-budget helpers, and structured error data. Shared vectors live
in `contracts/bridge-capacity-v1.conformance.json`; Python and Rust
serializers may produce different valid byte lengths.

Structured capacity error data uses `direction` (`request` or `response`),
`max_bytes`, and execution `outcome`. `not_dispatched` is a structured
statement about the described request and phase. It requires a `request`
direction and an `actual_bytes` value strictly greater than `max_bytes`; a
response or a missing/non-oversized size is rejected. An oversized result or a
failure after possible dispatch uses `unknown`, which may omit `actual_bytes`.
Callers inspect state before retrying. This slice does not introduce a new
envelope or error code.

Compact errors must carry only `code`, `message`, `data`, `outcome`, and
`actual_bytes` when applicable. Both implementations serialize the complete
error and reject it when it exceeds the shared `error_budget_bytes` value.
Correlation IDs and request bodies are never echoed into the error, so a long
association ID cannot inflate the serialized error beyond its own budget.

This slice is contract-only: it does not wire runtime WebSocket receive/send
limits, modify the hello/ack structs, change adapter behavior, or alter HTTP
and DCC-Link paths. Runtime enforcement, negotiated connection state, and
adapter adoption are outside this delivered contract.

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
