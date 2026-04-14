# HTTP API

`dcc_mcp_core` — MCP Streamable HTTP server (2025-03-26 spec).

## Overview

The `dcc-mcp-http` crate provides an MCP HTTP server that exposes your `ActionRegistry` over HTTP. MCP hosts (like Claude Desktop or other LLM integrations) connect via HTTP POST requests to the `/mcp` endpoint.

::: tip Background Thread
The server runs in a background Tokio thread and never blocks the DCC main thread. Safe to use in Maya/Blender/etc. plugins.
:::

## McpHttpConfig

Configuration for the HTTP server.

### Constructor

```python
from dcc_mcp_core import McpHttpConfig

cfg = McpHttpConfig(
    port=8765,                # TCP port (0 = random available)
    server_name="maya-mcp",   # Name in MCP initialize response
    server_version="1.0.0",    # Version in MCP initialize response
    enable_cors=False,         # CORS headers for browser clients
    request_timeout_ms=30000,  # Per-request timeout in ms
)
```

### Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `port` | `int` | `8765` | TCP port the server is listening on (`0` = OS-assigned) |
| `server_name` | `str` | `"dcc-mcp"` | Server name in MCP response |
| `server_version` | `str` | package version | Server version in MCP response |
| `gateway_port` | `int` | `0` | Gateway port to compete for (`0` = disabled). See [Gateway](#gateway) |
| `registry_dir` | `str \| None` | `None` | Directory for the shared `FileRegistry` JSON (defaults to OS temp dir) |
| `stale_timeout_secs` | `int` | `30` | Seconds without a heartbeat before an instance is considered stale |
| `heartbeat_secs` | `int` | `5` | Heartbeat interval in seconds (`0` = disabled) |
| `dcc_type` | `str \| None` | `None` | DCC type reported in registry (e.g. `"maya"`, `"blender"`) |
| `dcc_version` | `str \| None` | `None` | DCC version string reported in registry (e.g. `"2025"`) |
| `scene` | `str \| None` | `None` | Currently open scene file — improves gateway routing |

## ServerHandle

Returned by `McpHttpServer.start()`. Use it to get the MCP endpoint URL and shut down gracefully.

::: tip Alias
`ServerHandle` is also exported as `McpServerHandle` from `dcc_mcp_core`. Both names refer to the same class.

```python
from dcc_mcp_core import McpServerHandle  # alias for ServerHandle
```
:::

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `port` | `int` | Actual port server is bound to (useful when port=0) |
| `bind_addr` | `str` | Bind address, e.g. `"127.0.0.1:8765"` |
| `is_gateway` | `bool` | `True` if this process won the gateway port competition |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `mcp_url()` | `str` | Full MCP endpoint URL, e.g. `"http://127.0.0.1:8765/mcp"` |
| `shutdown()` | `None` | Graceful shutdown (blocks until stopped) |
| `signal_shutdown()` | `None` | Signal shutdown without blocking |

### Example

```python
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig

registry = ActionRegistry()
registry.register("get_scene_info", description="Get current scene info",
                  category="scene", tags=[], dcc="maya", version="1.0.0")

server = McpHttpServer(registry, McpHttpConfig(port=8765))
handle = server.start()

print(f"MCP HTTP server running at {handle.mcp_url()}")
# MCP host POSTs to http://127.0.0.1:8765/mcp

# Shutdown when done
handle.shutdown()
```

## McpHttpServer

MCP Streamable HTTP server (2025-03-26 spec).

### Constructor

```python
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig

server = McpHttpServer(
    registry,         # ActionRegistry instance
    config=None,      # McpHttpConfig (defaults to port=8765, no CORS)
)
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `start()` | `ServerHandle` | Start server in background thread and return handle |
| `register_handler(action_name, handler)` | `None` | Register a Python callable as the handler for an action |
| `has_handler(action_name)` | `bool` | Check if a handler is registered for an action |

### MCP Protocol Endpoints

The server implements the MCP 2025-03-26 spec:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp` | POST | MCP request (JSON-RPC 2.0) |
| `/mcp` | GET | SSE-compatible event stream |
| `/health` | GET | Health check |

### Request/Response Format

MCP requests use JSON-RPC 2.0:

```json
// POST /mcp
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/list",
  "params": {}
}
```

```json
// POST /mcp response
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {"name": "get_scene_info", "description": "Get current scene info", ...}
    ]
  }
}
```

### Supported MCP Methods

| Method | Description |
|--------|-------------|
| `initialize` | Protocol handshake, returns server capabilities |
| `tools/list` | List all registered actions from the registry |
| `tools/call` | Dispatch an action by name with parameters |
| `resources/list` | List available resources (empty in current impl) |
| `prompts/list` | List available prompts (empty in current impl) |
| `ping` | Liveness check |

## Full Example: Maya MCP Server

```python
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher, McpHttpServer, McpHttpConfig,
)

# Build action registry
registry = ActionRegistry()
registry.register(
    "get_scene_info",
    description="Get current Maya scene information",
    category="scene",
    tags=["query", "info"],
    dcc="maya",
    version="1.0.0",
    input_schema='{}',
)

# Register handler
dispatcher = ActionDispatcher(registry)

def get_scene_info(params):
    # In practice, query Maya via pymel/cmdx
    return {"scene_name": "untitled", "object_count": 0}

dispatcher.register_handler("get_scene_info", get_scene_info)

# Start HTTP server
config = McpHttpConfig(
    port=18812,
    server_name="maya-mcp",
    server_version="1.0.0",
)
server = McpHttpServer(registry, config)
handle = server.start()

print(f"Maya MCP server: {handle.mcp_url()}")
# Output: Maya MCP server: http://127.0.0.1:18812/mcp
```

## Gateway

When multiple DCC instances start simultaneously, one automatically becomes the **gateway** — a single well-known entry point that discovers and proxies to all running instances.

### How it works

- Every instance registers itself in a shared `FileRegistry` (JSON file on disk) and sends periodic heartbeats.
- The **first** process to bind `gateway_port` (default: `9765`) becomes the gateway; all others are plain instances.
- Mutual exclusion uses `SO_REUSEADDR=false` (via `socket2`), so the first-wins semantics are reliable across platforms including Windows.
- The gateway automatically evicts stale instances (no heartbeat within `stale_timeout_secs`).
- When the process exits, `ServerHandle` is dropped and the instance is automatically deregistered.

### Gateway endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/instances` | GET | JSON list of all live instances |
| `/health` | GET | `{"ok": true}` health check |
| `/mcp` | POST | Gateway's own MCP endpoint (discovery meta-tools) |
| `/mcp/{instance_id}` | POST | Transparent proxy to a specific instance |
| `/mcp/dcc/{dcc_type}` | POST | Proxy to the best instance of a DCC type |

### Gateway MCP meta-tools

The gateway exposes three discovery tools via its own `/mcp` endpoint:

| Tool | Description |
|------|-------------|
| `list_dcc_instances` | List all live DCC servers (type, port, scene, status) |
| `get_dcc_instance` | Get info for a specific instance (by id or `dcc_type+scene`) |
| `connect_to_dcc` | Return the direct MCP URL for a DCC instance |

### Python example

```python
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig

registry = ActionRegistry()
registry.register("get_scene_info", description="Get scene info", category="scene", dcc="maya")

config = McpHttpConfig(port=0, server_name="maya-mcp")
config.gateway_port = 9765    # join gateway competition; 0 = disabled
config.dcc_type = "maya"
config.dcc_version = "2025"
config.scene = "/proj/shot01.ma"  # optional: helps routing by scene

server = McpHttpServer(registry, config)
handle = server.start()

print(handle.is_gateway)        # True if this process won the gateway port
print(handle.mcp_url())         # direct MCP URL for this instance
# → gateway at http://127.0.0.1:9765/ (if is_gateway=True)
# → instance at http://127.0.0.1:<port>/mcp
```

::: tip Multiple DCCs, one endpoint
Start any number of DCC servers — the first one wins the gateway port. Agents always connect to `http://localhost:9765/mcp` and use `list_dcc_instances` / `connect_to_dcc` to discover and route to specific DCC processes.
:::

::: info Skills-First + gateway
`create_skill_manager()` does **not** configure `gateway_port` by default. Set it explicitly on the `McpHttpConfig` passed to `create_skill_manager()` if you want gateway participation:

```python
import os
from dcc_mcp_core import create_skill_manager, McpHttpConfig

config = McpHttpConfig(port=0, server_name="maya")
config.gateway_port = 9765
config.dcc_type = "maya"

server = create_skill_manager("maya", config)
handle = server.start()
```
:::

## CORS Configuration

Enable CORS for browser-based MCP clients:

```python
cfg = McpHttpConfig(port=8765, enable_cors=True)
server = McpHttpServer(registry, cfg)
handle = server.start()
print(handle.mcp_url())
```

## Error Handling

The server returns JSON-RPC error responses:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32602,
    "message": "Invalid params: missing 'radius'",
    "data": null
  }
}
```

Common error codes:

| Code | Meaning |
|------|---------|
| -32600 | Invalid Request |
| -32602 | Invalid Params |
| -32603 | Internal Error |
| -32000 | Action not found |
| -32001 | Action validation failed |
| -32002 | Action handler error |

## Performance Notes

- Server runs in background Tokio thread — no DCC main thread blocking
- Request timeout applies per-call (default 30s)
- No connection pooling on the HTTP layer (each POST is stateless)
- Use `TransportManager` for persistent IPC sessions with DCC
- Gateway `FileRegistry` flushes to disk on every mutation — safe for multi-process but not high-frequency writes
