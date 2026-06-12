# HTTP API

`dcc_mcp_core` — MCP Streamable HTTP server (2025-03-26 spec).

## Overview

The `dcc-mcp-http` crate provides an MCP HTTP server that exposes your `ToolRegistry` over HTTP. MCP hosts (like Claude Desktop or other LLM integrations) connect via HTTP POST requests to the `/mcp` endpoint.

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
| `host` | `str` | `"127.0.0.1"` | IP address to bind (localhost only per MCP security spec) |
| `endpoint_path` | `str` | `"/mcp"` | MCP endpoint path |
| `server_name` | `str` | `"dcc-mcp"` | Server name in MCP response |
| `server_version` | `str` | package version | Server version in MCP response |
| `max_sessions` | `int` | `100` | Maximum concurrent SSE sessions |
| `request_timeout_ms` | `int` | `30000` | Per-request timeout in milliseconds |
| `enable_cors` | `bool` | `False` | Enable CORS headers for browser clients |
| `session_ttl_secs` | `int` | `3600` | Idle session TTL in seconds (`0` = disable eviction) |
| `lazy_actions` | `bool` | `False` | Opt-in: surface only 3 meta-tools (`list_actions`, `describe_action`, `call_action`) instead of all tools in `tools/list` |
| `gateway_port` | `int` | `9765` (Python) | Gateway port to compete for (`0` = disabled). See [Gateway](#gateway) |
| `admin_enabled` | `bool` | `True` | Elected gateway serves the local Admin UI (`GET /admin`) |
| `admin_path` | `str` | `"/admin"` | URL prefix for the Admin UI |
| `registry_dir` | `str \| None` | `None` | Directory for the shared `FileRegistry` JSON (defaults to OS temp dir) |
| `stale_timeout_secs` | `int` | `30` | Seconds without a heartbeat before an instance is considered stale |
| `heartbeat_secs` | `int` | `5` | Heartbeat interval in seconds (`0` = disabled) |
| `dcc_type` | `str \| None` | `None` | DCC type reported in registry (e.g. `"maya"`, `"blender"`) |
| `dcc_version` | `str \| None` | `None` | DCC version string reported in registry (e.g. `"2025"`) |
| `scene` | `str \| None` | `None` | Currently open scene file — improves gateway routing |
| `spawn_mode` | `str` | `"dedicated"` | Listener spawn strategy: `"ambient"` (standalone binary) or `"dedicated"` (PyO3-embedded; own OS thread + current_thread runtime). Fixes issue #303 |
| `self_probe_timeout_ms` | `int` | `200` | Max ms to wait when self-probing a freshly bound listener. 0 disables. Issue #303 guard |
| `bare_tool_names` | `bool` | `True` | Publish unique action names without `<skill>__` prefix in `tools/list` (#307). Collisions fall back to the client-safe `<skill>__<action>` form. |
| `enable_tool_cache` | `bool` | `True` | Connection-scoped `tools/list` cache (#438). Per-session snapshot avoids redundant registry scans on sequential calls. Invalidated on skill load/unload, group activation/deactivation, session eviction, or `_meta.dcc.refresh=true` |

::: tip Admin persistence
`McpHttpConfig` controls whether the elected gateway serves `/admin`; durable admin storage is intentionally environment-driven. Set `DCC_MCP_GATEWAY_AUDIT_DIR` to persist `/admin/api/calls` rows in `audit.jsonl` and `/admin/api/traces` rows in `traces.jsonl`; `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` caps each file.
:::

## McpServerHandle

Returned by `McpHttpServer.start()`. Use it to get the MCP endpoint URL and shut down gracefully.

::: tip Alias
`McpServerHandle` is the preferred public name. The internal `ServerHandle` remains available as a compatibility alias.

```python
from dcc_mcp_core import McpServerHandle  # preferred public handle name
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
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

registry = ToolRegistry()
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
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

server = McpHttpServer(
    registry,         # ToolRegistry instance
    config=None,      # McpHttpConfig (defaults to port=8765, no CORS)
)
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `start()` | `McpServerHandle` | Start server in background thread and return handle |
| `register_handler(tool_name, handler)` | `None` | Register a Python callable that receives decoded params (typically a `dict`) |
| `has_handler(tool_name)` | `bool` | Check if a handler is registered for a tool |
| `discover(extra_paths, dcc_name)` | `int` | Scan and populate the skill catalog; returns count of discovered skills |
| `load_skill(skill_name)` | `list[str]` | Load a skill, registering its tools; returns tool names |
| `unload_skill(skill_name)` | `bool` | Unload a skill, removing its tools |
| `search_skills(query, tags, dcc, scope, limit)` | `list[SkillSummary]` | Unified skill discovery. All arguments optional; empty call returns the top `limit` skills by scope precedence (Admin > System > User > Repo). |
| `list_skills(status)` | `list[SkillSummary]` | List skills with optional status filter (`"loaded"`/`"unloaded"`) |
| `is_loaded(skill_name)` | `bool` | Check if a skill is currently loaded |
| `loaded_count()` | `int` | Number of currently loaded skills |

### MCP Protocol Endpoints

The server implements the MCP 2025-03-26 spec:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp` | POST | MCP request (JSON-RPC 2.0) |
| `/mcp` | GET | SSE-compatible event stream |
| `/mcp` | DELETE | Terminate MCP session |
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
| `tools/list` | List all registered tools from the registry |
| `tools/call` | Dispatch a tool by name with parameters |
| `resources/list` | List available resources; gateways include the `gateway://instances` root pointer |
| `prompts/list` | List registered prompts; gateways return namespaced backend prompt templates |

| `ping` | Liveness check |

## Built-in Tools {#builtin-tools}

Every `McpHttpServer` emits a fixed set of built-in tools in `tools/list` in addition to whatever is registered on the `ToolRegistry` or loaded from skills. Their descriptions follow the 3-layer `what / When to use / How to use` structure defined in issue #341 (capped at 500 chars, enforced by `tests/test_tool_descriptions.py`).

### Discovery & lifecycle (always-on)

| Tool | Purpose | Typical follow-up |
|------|---------|-------------------|
| `search_skills` | Ranked keyword search over discovered skills (name, description, search-hint, tags, tool names). **Start here** when you don't know the skill name. | `load_skill(skill_name=...)` |
| `list_skills` | Flat dump of every discovered skill with load status. Use for browsing, not search. | `get_skill_info(...)` or `load_skill(...)` |
| `get_skill_info` | Full metadata + input schemas for one skill. | `load_skill(skill_name=...)` |
| `load_skill` | Loads one or more skills; emits `tools/list_changed`. Idempotent. | Call the specific tool by name |
| `unload_skill` | Unloads a skill; emits `tools/list_changed`. Idempotent. | `load_skill(...)` again if needed |
| `activate_tool_group` | Expands a `__group__<name>` stub into its member tools. | Re-call `tools/list`, then the tool |
| `deactivate_tool_group` | Collapses a tool group back to a stub to shrink the token footprint. | `activate_tool_group(...)` |
| `search_tools` | Full-text search over active tools plus unloaded skill candidates, returned without full schemas. | `get_skill_info(...)`, `load_skill(...)`, or call the matched active tool |
| `list_roots` | Returns the filesystem roots the client advertised via `roots/list`. Rarely needed. | — |

### Lazy-actions fast-path (opt-in)

Enabled via `McpHttpConfig.lazy_actions = True`. Replaces the per-tool `tools/list` expansion with three meta-tools so the tool surface stays small regardless of how many tools are registered:

| Tool | Purpose |
|------|---------|
| `list_actions` | Compact `{id, summary, tags}` records for every enabled action, no schemas. |
| `describe_action` | Full input schema for one action by id. |
| `call_action` | Generic dispatcher — invokes any action by id with args. Same code path as a direct `tools/call`. |

Typical flow: `list_actions(dcc="maya")` → pick an id → `describe_action(id=...)` → `call_action(id=..., args={...})`.

### Writing new built-in tools

Built-in tool descriptions are **hand-written**, not auto-generated. When adding a new built-in, follow the 3-layer pattern used by every entry in `build_core_tools_inner()`:

```
<1 sentence present-tense summary of what the tool does>

When to use: <1-2 sentences naming the situation, and explicitly contrasting the tool against its nearest sibling so an agent knows when NOT to pick it>

How to use:
- <precondition or common pitfall>
- <suggested follow-up tool, fully qualified>
```

Keep the whole string ≤ 500 chars; move any longer reference material here, under a stable heading, and link to it from the description. Per-parameter `description` fields in the input schema are single clauses ≤ 100 chars. `tests/test_tool_descriptions.py` enforces both bounds structurally so future tools stay consistent.

## Full Example: Maya MCP Server

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

# Build tool registry
registry = ToolRegistry()
registry.register(
    "get_scene_info",
    description="Get current Maya scene information",
    category="scene",
    tags=["query", "info"],
    dcc="maya",
    version="1.0.0",
    input_schema='{}',
)

def get_scene_info(params):
    # In practice, query Maya via pymel/cmdx
    return {"scene_name": "untitled", "object_count": 0}

server = McpHttpServer(registry, McpHttpConfig(
    port=18812,
    server_name="maya-mcp",
    server_version="1.0.0",
))
server.register_handler("get_scene_info", get_scene_info)

# Start HTTP server
handle = server.start()

print(f"Maya MCP server: {handle.mcp_url()}")
# Output: Maya MCP server: http://127.0.0.1:18812/mcp
```

## Gateway

When multiple DCC instances start simultaneously, one automatically becomes the **gateway** — a single well-known `/mcp` endpoint and `/v1/*` REST facade for all running instances. Since v0.15, the gateway does **not** merge every backend action into `tools/list`; it keeps MCP bounded and advertises only four canonical workflow tools: `search`, `describe`, `load_skill`, and `call`.

### How it works

- Every instance registers itself in a shared `FileRegistry` (JSON file on disk) and sends periodic heartbeats.
- The **first** process to bind `gateway_port` (default: `9765` for the Python API and `dcc-mcp-server`) becomes the gateway; all others are plain instances.
- Mutual exclusion uses `SO_REUSEADDR=false` (via `socket2`), so the first-wins semantics are reliable across platforms including Windows.
- The gateway probes `GET /v1/readyz` (falling back to `/health` for old backends) and evicts instances after consecutive failures.
- When the process exits, `McpServerHandle` is dropped and the instance is automatically deregistered.

### Gateway endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/instances` | GET | JSON list of all live instances |
| `/v1/instances` | GET | REST alias for instance discovery |
| `/health` | GET | `{"ok": true}` health check |
| `/mcp` | POST | Bounded MCP endpoint with gateway workflow primitives (`search`, `describe`, `load_skill`, `call`) |
| `/mcp` | GET | SSE stream for progress, job/workflow, resource, and prompt notifications |
| `/v1/search` | POST | Search compact backend capability records |
| `/v1/describe` | POST | Fetch schema, annotations, and routing record for one `tool_slug` |
| `/v1/call` | POST | Invoke one backend capability by `tool_slug` |
| `/v1/resources` | GET | Aggregate gateway-native and backend MCP resources |
| `/v1/resources/{uri}` | GET | Read one percent-encoded resource URI |
| `/v1/prompts` | GET | Aggregate backend prompt templates |
| `/v1/prompts/{name}` | GET | Render one prompt; `?args=<json>` forwards prompt arguments |
| `/v1/jobs/{id}/events` | GET | SSE stream for one async job |
| `/v1/jobs/{id}` | DELETE | Cancel one async job |
| `/mcp/{instance_id}` | POST | Transparent proxy to a specific instance (low-level escape hatch) |
| `/mcp/dcc/{dcc_type}` | POST | Proxy to the best instance of a DCC type |

### Bounded facade

`POST /mcp` on the gateway is a single MCP server that advertises only four canonical gateway tools in `tools/list`:

| Tier | Tools | Purpose |
|------|-------|---------|
| Discovery | `search`, `describe` | Search compact backend capability records and fetch schema/detail for one discovered `tool_slug` or `skill_name` |
| Activation | `load_skill` | Load a discovered skill or activate/deactivate a progressive tool group |
| Execution | `call` | Invoke one `tool_slug` or an ordered `{calls:[...]}` batch |

Gateway backend actions are addressed by `tool_slug` (`<dcc>.<id8>.<tool>`). Direct per-DCC REST uses `<dcc>.<skill>.<action>` without the instance id. Agents should not construct slugs by hand; obtain them from MCP `search` or `POST /v1/search`, inspect with MCP `describe` or `POST /v1/describe`, then execute via MCP `call`, `POST /v1/call`, or `POST /v1/call_batch`. Hidden MCP compatibility routes still accept older `search_tools` / `describe_tool` / `call_tool` / `call_tools` names, but they are not advertised.

#### `gateway://instances` — DCC registry as an MCP resource (#813 phase 1)

The live DCC registry is published as a gateway-native MCP resource, not as
a tool. Agents fetch it via `resources/read` instead of paying the
`tools/list` token cost for instance-discovery verbs.

```jsonc
// Request: list every parseable row in the registry directory
// (`$TEMP/dcc-mcp-registry/`), regardless of `dcc_type`. Stale sentinels
// surface with `status: "stale"` so operators can see why a registration
// is no longer routable instead of having it silently elided.
{"jsonrpc":"2.0","id":1,"method":"resources/read",
 "params":{"uri":"gateway://instances"}}

// Optional URI query: hide stale rows / show the raw registry view
// (default: stale visible, dead-PID rows pruned).
//   gateway://instances?include_stale=false
//   gateway://instances?include_dead=true
```

The payload returned in `contents[0].text` is a JSON document of shape:

```json
{
  "total": 3,
  "stale_count": 1,
  "evicted_dead": 0,
  "instances": [
    {
      "instance_id": "a1b2c3d4-…",
      "dcc_type": "maya",
      "host": "127.0.0.1",
      "port": 18812,
      "mcp_url": "http://127.0.0.1:18812/mcp",
      "status": "available",
      "scene": "/proj/shot01.ma",
      "documents": [],
      "pid": 1234,
      "display_name": "Maya-Rigging",
      "version": "2024",
      "adapter_version": "0.3.0",
      "adapter_dcc": "maya",
      "metadata": {},
      "stale": false
    },
    {
      "instance_id": "f9e8d7c6-…",
      "dcc_type": "maya",
      "host": "127.0.0.1",
      "port": 18813,
      "status": "stale",
      "stale": true,
      "...": "fields above also present"
    }
  ]
}
```

Each entry already carries `mcp_url`, so a client that has read this
resource has everything it needs to connect — no follow-up "connect"
verb is required. To inspect a single instance, read
`gateway://instances/{instance_id}` (full UUID or unique prefix).

The bookkeeping `__gateway__` sentinel and the gateway's own self-row are
always filtered.

### Python example

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

registry = ToolRegistry()
registry.register("get_scene_info", description="Get scene info", category="scene", dcc="maya")

config = McpHttpConfig(port=0, server_name="maya-mcp")
# Python default: gateway_port=9765, admin_enabled=True, admin_path="/admin".
# Set gateway_port=0 to disable gateway/admin, or admin_enabled=False to keep gateway only.
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
Start any number of DCC servers — the first one wins the gateway port. Agents connect to `http://localhost:9765/mcp`, call `search` to discover backend capabilities, use `describe` for schema inspection, then execute with `POST /v1/call`. Reading the `gateway://instances` MCP resource yields each backend's `mcp_url` directly when an agent wants a direct, un-proxied session.
:::

::: info Skills-First + gateway
`create_skill_server()` uses the provided `McpHttpConfig`. A freshly constructed Python `McpHttpConfig` joins gateway election by default (`gateway_port=9765`) and exposes Admin on the elected gateway. Set `gateway_port=0` for an isolated server:

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

config = McpHttpConfig(port=0, server_name="maya")
# config.gateway_port = 0        # uncomment to disable gateway/admin
# config.admin_enabled = False   # keep gateway, hide Admin UI
config.dcc_type = "maya"

server = create_skill_server("maya", config)
handle = server.start()
```
:::

## Job lifecycle notifications

Every `tools/call` is tracked by a [`JobManager`](../api/actions.md) instance, and transitions are surfaced to the client through three SSE channels (issue #326):

| Channel | Method | Fires when |
|---------|--------|-----------|
| A | `notifications/progress` | The call supplied `_meta.progressToken`. Echoes the token, and maps `pending=0`, `running=10`, terminal states=100. |
| B | `notifications/$/dcc.jobUpdated` | `enable_job_notifications` is `True` (default). One event per transition, payload carries `job_id`, `tool`, `status`, `started_at`, `completed_at`, `error`. |
| C | `notifications/$/dcc.workflowUpdated` | Same flag; emitted by the workflow executor (#348) on step enter / step terminal / workflow terminal. |

```python
cfg = McpHttpConfig(port=8765)
# Default True; set False to opt the whole server out of the $/dcc.* channels.
cfg.enable_job_notifications = True
```

Channel A follows the MCP 2025-03-26 spec exactly and is mandatory whenever a `progressToken` is provided — the flag only controls B and C.

### Built-in tools: `jobs_get_status`

Clients that can't consume the `$/dcc.jobUpdated` SSE stream (or simply prefer request/response) can poll job state via the always-registered `jobs_get_status` built-in tool (issue #319).

- **Name**: `jobs_get_status` — client-safe (validated with `TOOL_NAME_RE` at server startup; the build panics if the regex ever rejects the name).
- **Visibility**: surfaced in `tools/list` unconditionally, regardless of which skills are loaded or whether any jobs exist.
- **Annotations**: `readOnlyHint=true`, `destructiveHint=false`, `idempotentHint=true`, `openWorldHint=false`.

Input schema:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `job_id` | `string` | — (required) | UUID of the job to query |
| `include_logs` | `boolean` | `false` | Forward-compat flag; `JobManager` does not currently capture stdout/stderr, so the flag is a no-op and a `tracing::debug!` breadcrumb is emitted |
| `include_result` | `boolean` | `true` | When `true` **and** the job is terminal (`completed` / `failed`), include the final `ToolResult` JSON under `result`; otherwise the key is omitted |

Success envelope (returned inside a `CallToolResult` — text content mirrors `structuredContent`):

```json
{
  "job_id": "c8aa…",
  "parent_job_id": null,
  "tool": "scene.get_info",
  "status": "completed",
  "created_at": "2026-04-22T10:00:00+00:00",
  "started_at": "2026-04-22T10:00:00+00:00",
  "completed_at": "2026-04-22T10:00:01+00:00",
  "updated_at": "2026-04-22T10:00:01+00:00",
  "progress": {"current": 10, "total": 10, "message": "done"},
  "error": null,
  "result": { "...": "…final ToolResult…" }
}
```

Field semantics mirror the `$/dcc.jobUpdated` channel so polling and streaming clients observe the same shape. `started_at` is derived from `updated_at` once the job leaves `pending`; `completed_at` from `updated_at` once it reaches a terminal state.

Unknown job id → `CallToolResult { isError: true, content: [{type:"text", text:"No job found with id '<bad>'"}] }`. This is always an MCP tool-level error, **never** a JSON-RPC transport error — the response still carries a successful `result` field with `isError=true`.

Python example:

```python
body = post_mcp({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                 "params": {"name": "jobs_get_status",
                            "arguments": {"job_id": jid}}})
env = body["result"]["structuredContent"]
if env["status"] in {"completed", "failed", "cancelled", "interrupted"}:
    print("final:", env.get("result"))
```

### Built-in tools: `jobs_cleanup`

Prune terminal jobs from `JobManager` (and any attached `JobStorage` backend) once they age out. Complements [`jobs_get_status`](#built-in-tools-jobs_get_status) and the optional SQLite persistence backend (see [`docs/guide/job-persistence.md`](../guide/job-persistence.md), issue #328).

- **Name**: `jobs_cleanup` — client-safe, validated at server startup.
- **Visibility**: always surfaced in `tools/list`, independent of which skills are loaded.
- **Annotations**: `readOnlyHint=false`, `destructiveHint=true`, `idempotentHint=true`, `openWorldHint=false`.

Input schema:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `older_than_hours` | `integer ≥ 0` | `24` | Remove terminal jobs whose last `updated_at` is older than this many hours. Set to `0` to purge all terminal rows. |

Only terminal statuses (`completed`, `failed`, `cancelled`, `interrupted`) are eligible — `pending` and `running` rows are never removed regardless of age.

Success envelope (`CallToolResult.structuredContent`):

```json
{ "removed": 42, "older_than_hours": 24 }
```

## Optional SQLite job persistence

Set `McpHttpConfig.job_storage_path` to persist `JobManager` state so in-flight jobs survive a restart (issue #328). Requires the `job-persist-sqlite` Cargo feature; when the feature is absent but the path is set, `McpHttpServer.start()` returns a descriptive error rather than silently falling back to the in-memory store.

```python
cfg = McpHttpConfig(port=8765)
cfg.job_storage_path = "/var/lib/dcc-mcp/jobs.sqlite3"
cfg.job_recovery = "drop"        # default; "requeue" reserved (issue #567)
```

On startup, any `pending` / `running` rows from a previous run are rewritten to the new terminal `interrupted` status with `error = "server restart"` and surfaced via `$/dcc.jobUpdated`.

`McpHttpConfig.job_recovery` selects the policy applied to those rows (issue #567):

| Value       | Behaviour                                                                                                                                                                                                                                                                  |
|-------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `"drop"`    | **Default.** Rows are flipped to `interrupted`; clients re-running the work must re-submit explicitly.                                                                                                                                                                     |
| `"requeue"` | **Reserved.** Accepted today but degrades to `"drop"` with a `WARN` log (`requested_policy=requeue effective_policy=drop`). True requeue requires persisting the original tool arguments alongside the row, which lands in a future release. Setting this today is forward-compatible: the same config will pick up real requeue when it ships. |

Full design and operational guidance: [`docs/guide/job-persistence.md`](../guide/job-persistence.md).

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
| -32000 | Tool not found |
| -32001 | Tool validation failed |
| -32002 | Tool handler error |

## Context-Efficient Tool Loading (issue #405) {#context-efficient-loading}

When a skill catalog grows large (50+ tools × multiple skills), the initial
`tools/list` response becomes expensive in token terms. Three strategies let
you control how much of the schema surface is pushed into the agent's context.

### Comparison

| Strategy | `tools/list` content | Token cost | Best for |
|----------|---------------------|------------|----------|
| Default (eager) | All tool definitions + schemas | High | ≤ 20 tools |
| `lazy_actions = True` | 3 meta-tools only | Very low | Large static catalogs |
| Skill stubs (default) | `__skill__<name>` stubs for unloaded skills | Low | Dynamic loading workflows |
| `search` (gateway) / `search_tools` (direct server) | Compact search results; fetch schema with gateway `describe` or direct-server `get_skill_info` | Low | Large catalogs and multi-DCC gateways |

### 1. Default: Skill Stubs

Without any config, `tools/list` emits:

- A `__skill__<name>` stub for every **unloaded** skill (name + 1-line description, no input schemas)
- Full tool definitions for every **loaded** skill

Agent workflow: `search_tools(query)` or `search_skills(query)` →
`get_skill_info(skill_name=...)` for the selected skill → `load_skill(name)` →
call the typed tool. If a client needs to inspect `tools/list`, it must follow
every `nextCursor`; the first page is not a complete index.

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
# tools/list returns __skill__<name> stubs for each discovered skill
# Until load_skill() is called, no input schemas are sent to the agent
```

### 2. `lazy_actions = True` (3 meta-tools)

Surfaces only `list_actions`, `describe_action`, `call_action`.

```python
cfg = McpHttpConfig(port=8765)
cfg.lazy_actions = True
server = create_skill_server("maya", cfg)
```

Token footprint for `tools/list` is essentially constant (3 tool entries)
regardless of catalog size. The agent must call `list_actions(dcc="maya")`
first to discover available tools.

### 3. `lazy_tool_schemas = True` (planned, issue #405)

> **Note**: `lazy_tool_schemas` is planned for a future release. Track issue #405.

When enabled, `tools/list` will return tool entries with `description` only,
omitting `inputSchema`. A subsequent `tools/describe` call returns the full
schema for a single tool. This aligns with the pattern Anthropic describes as
cutting tool-definition tokens by 85%+ while maintaining high selection
accuracy.

```python
# Planned API (not yet available):
cfg = McpHttpConfig(port=8765)
cfg.lazy_tool_schemas = True  # omit inputSchema from tools/list (planned)
```

### 4. `search_tools` and gateway dynamic search

Direct per-DCC servers expose `search_tools` for active tools and unloaded skill
candidates. Use `get_skill_info(skill_name=...)` to inspect the selected
skill's full declared tool schemas before `load_skill`, then call the concrete
tool by name. `tools/list` remains MCP-compatible and paginated, but production
agents should not treat its first page as a complete discovery index.

The gateway advertises the canonical MCP workflow `search` → `describe` →
`load_skill` (when needed) → `call`. REST `/v1/search`, `/v1/describe`,
`/v1/load_skill`, `/v1/call`, and `/v1/call_batch` remain the pure HTTP twin.
Gateway search returns compact records only; `describe` fetches one full schema
on demand, keeping `tools/list` bounded even when many backends are live. The
gateway REST workflow returns compact TOON by default; clients that require
legacy JSON should send `Accept: application/json` or body
`response_format: "json"`.

### Token Usage Decision Guide

```
Catalog size  ┌─────────────────────────────────────────────────────┐
< 20 tools    │ Default (eager) — simplest, no configuration needed  │
20-100 tools  │ Skill stubs (default) — search_skills + load_skill   │
100+ tools    │ lazy_actions=True  — constant 3 meta-tools           │
> 500 tools   │ lazy_tool_schemas=True (planned) — 85%+ reduction    │
              └─────────────────────────────────────────────────────┘
```

## Connection-Scoped Tool Cache (issue #438) {#connection-scoped-cache}

When an MCP agent loop calls `tools/list` repeatedly between sequential tool calls
(e.g. "create sphere → assign material → add light → render"), each call
rebuilds the full tool list from scratch — scanning the `ToolRegistry`,
resolving bare names, converting every `ToolMeta` into an `McpTool`, and
fetching unloaded skill stubs. With 100+ registered tools, this per-request
overhead becomes measurable.

The connection-scoped cache stores a per-session snapshot of the `tools/list`
result. On subsequent calls, if the registry has not changed, the cached
snapshot is returned directly — skipping all redundant computation.

### How it works

1. On the **first** `tools/list` call in a session, the full tool list is
   built normally and stored as a `ToolListSnapshot` on the `McpSession`.
2. On **subsequent** calls, the server checks the current registry generation
   against the snapshot's generation:
   - **Match** → return the cached snapshot (fast path)
   - **Mismatch** → rebuild and store a fresh snapshot (slow path)
3. Cursor pagination is applied on the cached snapshot just like on a fresh list.

### Cache invalidation

The cache is automatically invalidated when any of these events occur:

| Event | Mechanism |
|-------|-----------|
| Skill loaded (`load_skill`) | Registry generation bumped; all session caches cleared |
| Skill unloaded (`unload_skill`) | Registry generation bumped; all session caches cleared |
| Tool group activated/deactivated | Registry generation bumped; all session caches cleared |
| Session evicted (TTL expiry) | Session and its cache are dropped |
| Client sends `_meta.dcc.refresh = true` | Cache bypassed for that single request |

### Configuration

```python
from dcc_mcp_core import McpHttpConfig

cfg = McpHttpConfig(port=8765)
# Tool cache is enabled by default (True). Disable to force a full
# rebuild on every tools/list call (useful for debugging).
cfg.enable_tool_cache = False
```

### Forcing a refresh

MCP clients can request a fresh tool list by including `_meta.dcc.refresh = true`
in the `tools/list` request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/list",
  "params": {},
  "_meta": { "dcc": { "refresh": true } }
}
```

### Expected impact

| Metric | Before | After (cache hit) |
|--------|--------|-------------------|
| `tools/list` response time (100+ tools) | Full registry scan + bare-name resolution + McpTool construction | Snapshot clone + pagination (~0ms resolution overhead) |
| Agent loop throughput | N independent requests | Session-aware sequential optimization |

### Gateway behavior

The cache is per-session on the individual DCC instance. Gateway requests are
proxied to the backend instance, so the cache operates transparently — the
gateway itself does not cache tool lists.

## Performance Notes

- Server runs in background Tokio thread — no DCC main thread blocking
- Request timeout applies per-call (default 30s)
- No connection pooling on the HTTP layer (each POST is stateless)
- Use `IpcChannelAdapter` for persistent IPC sessions with DCC
- Gateway `FileRegistry` flushes to disk on every mutation — safe for multi-process but not high-frequency writes
