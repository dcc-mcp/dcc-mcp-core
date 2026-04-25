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
| `gateway_port` | `int` | `0` | Gateway port to compete for (`0` = disabled). See [Gateway](#gateway) |
| `registry_dir` | `str \| None` | `None` | Directory for the shared `FileRegistry` JSON (defaults to OS temp dir) |
| `stale_timeout_secs` | `int` | `30` | Seconds without a heartbeat before an instance is considered stale |
| `heartbeat_secs` | `int` | `5` | Heartbeat interval in seconds (`0` = disabled) |
| `dcc_type` | `str \| None` | `None` | DCC type reported in registry (e.g. `"maya"`, `"blender"`) |
| `dcc_version` | `str \| None` | `None` | DCC version string reported in registry (e.g. `"2025"`) |
| `scene` | `str \| None` | `None` | Currently open scene file — improves gateway routing |
| `spawn_mode` | `str` | `"dedicated"` | Listener spawn strategy: `"ambient"` (standalone binary) or `"dedicated"` (PyO3-embedded; own OS thread + current_thread runtime). Fixes issue #303 |
| `self_probe_timeout_ms` | `int` | `200` | Max ms to wait when self-probing a freshly bound listener. 0 disables. Issue #303 guard |
| `bare_tool_names` | `bool` | `True` | Publish unique action names without `<skill>.` prefix in `tools/list` (#307). Collisions fall back to full form; `tools/call` accepts both shapes |
| `enable_tool_cache` | `bool` | `True` | Connection-scoped `tools/list` cache (#438). Per-session snapshot avoids redundant registry scans on sequential calls. Invalidated on skill load/unload, group activation/deactivation, session eviction, or `_meta.dcc.refresh=true` |

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
| `resources/list` | List available resources (empty in current impl) |
| `prompts/list` | List available prompts (empty in current impl) |
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
| `search_tools` | Full-text search over **already-registered** tools. If nothing matches, try `search_skills`. | Call the matched tool |
| `list_roots` | Returns the filesystem roots the client advertised via `roots/list`. Rarely needed. | — |

### Lazy-actions fast-path (opt-in)

Enabled via `McpHttpConfig.lazy_actions = True`. Replaces the per-action `tools/list` expansion with three meta-tools so the tool surface stays small regardless of how many actions are registered:

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

When multiple DCC instances start simultaneously, one automatically becomes the **gateway** — a single well-known `/mcp` endpoint that **aggregates** every running instance's tools into one unified MCP interface.

### How it works

- Every instance registers itself in a shared `FileRegistry` (JSON file on disk) and sends periodic heartbeats.
- The **first** process to bind `gateway_port` (default: `9765`) becomes the gateway; all others are plain instances.
- Mutual exclusion uses `SO_REUSEADDR=false` (via `socket2`), so the first-wins semantics are reliable across platforms including Windows.
- The gateway automatically evicts stale instances (no heartbeat within `stale_timeout_secs`).
- When the process exits, `McpServerHandle` is dropped and the instance is automatically deregistered.

### Gateway endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/instances` | GET | JSON list of all live instances |
| `/health` | GET | `{"ok": true}` health check |
| `/mcp` | POST | Aggregating MCP endpoint (merges every backend's tools) |
| `/mcp` | GET | SSE stream — `tools/list_changed` and `resources/list_changed` |
| `/mcp/{instance_id}` | POST | Transparent proxy to a specific instance (low-level escape hatch) |
| `/mcp/dcc/{dcc_type}` | POST | Proxy to the best instance of a DCC type |

### Aggregating facade

`POST /mcp` on the gateway is a single MCP server that exposes three tiers of tools merged into one `tools/list` response:

| Tier | Tools | Purpose |
|------|-------|---------|
| Discovery meta | `list_dcc_instances`, `get_dcc_instance`, `connect_to_dcc` | Enumerate / inspect live DCCs; get a direct MCP URL when needed |
| Skill management | `list_skills`, `search_skills`, `get_skill_info`, `load_skill`, `unload_skill` | Fan-out to every DCC (read ops) or target a specific instance via the `instance_id` / `dcc` argument (`load_skill` / `unload_skill`) |
| Backend tools | Every live DCC's own tools, prefixed with an 8-char instance id — e.g. `a1b2c3d4__create_sphere` | Routed to the originating backend by the prefix |

Each namespaced backend tool also carries `_instance_id`, `_instance_short`, and `_dcc_type` annotations so agents can disambiguate colliding names (e.g. `create_cube` on Maya and Blender appear as two distinct entries with different prefixes).

The gateway advertises `capabilities.tools.listChanged: true` and polls backends every 3 s; when the aggregated set changes (skill loaded / unloaded anywhere) it broadcasts `notifications/tools/list_changed` to every connected SSE client.

### Python example

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

registry = ToolRegistry()
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
Start any number of DCC servers — the first one wins the gateway port. Agents always connect to `http://localhost:9765/mcp` and see every backend's tools in a single `tools/list`, namespaced by instance. `list_dcc_instances` / `connect_to_dcc` are available when an agent wants a direct, un-proxied session.
:::

::: info Skills-First + gateway
`create_skill_server()` does **not** configure `gateway_port` by default. Set it explicitly on the `McpHttpConfig` passed to `create_skill_server()` if you want gateway participation:

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

config = McpHttpConfig(port=0, server_name="maya")
config.gateway_port = 9765
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

### Built-in tools: `jobs.get_status`

Clients that can't consume the `$/dcc.jobUpdated` SSE stream (or simply prefer request/response) can poll job state via the always-registered `jobs.get_status` built-in tool (issue #319).

- **Name**: `jobs.get_status` — SEP-986 compliant (validated with `TOOL_NAME_RE` at server startup; the build panics if the regex ever rejects the name).
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
                 "params": {"name": "jobs.get_status",
                            "arguments": {"job_id": jid}}})
env = body["result"]["structuredContent"]
if env["status"] in {"completed", "failed", "cancelled", "interrupted"}:
    print("final:", env.get("result"))
```

### Built-in tools: `jobs.cleanup`

Prune terminal jobs from `JobManager` (and any attached `JobStorage` backend) once they age out. Complements [`jobs.get_status`](#built-in-tools-jobsget_status) and the optional SQLite persistence backend (see [`docs/guide/job-persistence.md`](../guide/job-persistence.md), issue #328).

- **Name**: `jobs.cleanup` — SEP-986 compliant, validated at server startup.
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
```

On startup, any `pending` / `running` rows from a previous run are rewritten to the new terminal `interrupted` status with `error = "server restart"` and surfaced via `$/dcc.jobUpdated`. Full design and operational guidance: [`docs/guide/job-persistence.md`](../guide/job-persistence.md).

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
| `tools/search` (future) | Zero schemas on init; search → schema on demand | 85%+ reduction | Very large catalogs |

### 1. Default: Skill Stubs

Without any config, `tools/list` emits:

- A `__skill__<name>` stub for every **unloaded** skill (name + 1-line description, no input schemas)
- Full tool definitions for every **loaded** skill

Agent workflow: `search_skills(query)` → `load_skill(name)` → re-call `tools/list`.

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

### 4. `tools/search` endpoint (planned, issue #405)

> **Note**: A `tools/search` built-in tool is planned for a future release.

The planned endpoint will:

- Accept a natural language query
- Return matching tool definitions (name + description + inputSchema) ranked by relevance
- Keep the initial `tools/list` response minimal (names only, no schemas)

Until this is available, use the existing `search_skills(query)` → `load_skill(name)`
workflow which already provides skill-level search.

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
rebuilds the full tool list from scratch — scanning the `ActionRegistry`,
resolving bare names, converting every `ActionMeta` into an `McpTool`, and
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
