# AI Agent Guide — dcc-mcp-core

> **Quick orientation for AI agents**: This guide teaches you how to effectively use dcc-mcp-core to interact with DCC (Digital Content Creation) software like Maya, Blender, Houdini, etc.

## 🎯 Core Philosophy: Skills-First

**When interacting with DCC applications, ALWAYS prefer dcc-mcp-core Skills over raw CLI scripting or direct API calls.**

### Why Skills-First?

| Aspect | dcc-mcp-core Skills | Raw CLI / Scripting |
|--------|---------------------|---------------------|
| **Input Validation** | JSON Schema validated | None — garbage in, garbage out |
| **Safety** | `ToolAnnotations` (read-only, destructive, idempotent) | Unknown |
| **Discoverability** | `search_skills()` + semantic search | Manual exploration |
| **Follow-up Guidance** | `next-tools` chains | Manual discovery |
| **Progressive Loading** | Load only what you need | All or nothing |
| **Error Recovery** | Structured `error_result` with `prompt` suggestions | Unstructured stderr |
| **Traceability** | Audit log + telemetry | None |

## 🚀 Quick Start Workflow

### 1. Discover Relevant Skills

```python
from dcc_mcp_core import SkillCatalog, ToolRegistry, scan_and_load

# Always start by discovering what's available.
# Returns: (List[SkillMetadata], List[str] skipped_dirs).
skills, skipped = scan_and_load(dcc_name="maya")

# For AI agents: use search_skills for semantic discovery.
registry = ToolRegistry()
catalog = SkillCatalog(registry)
results = catalog.search_skills(query="create sphere geometry")
```

### 2. Load the Skill

```python
# Load a specific skill to expose its tools
catalog.load_skill("maya-geometry")
```

### 3. Call Tools with Validation

```python
# Tools are now available via the dispatcher
result = dispatcher.dispatch("maya-geometry__create_sphere", '{"radius": 2.0}')

# Always check the result structure
if result.get("success"):
    print(f"Tool succeeded: {result.get('message')}")
else:
    print(f"Tool failed: {result.get('error')}")
    print(f"Suggestion: {result.get('prompt')}")

# Over MCP, follow-up hints are attached to CallToolResult._meta["dcc.next_tools"].
# Use .on_success after successful calls and .on_failure after errors when present.
```

### 4. Follow next-tools Guidance

When an MCP `tools/call` response includes `CallToolResult._meta["dcc.next_tools"].on_success` or `.on_failure`, **always consider calling those tools next**. This creates a guided workflow chain; the declarations live per tool in sibling `tools.yaml`, not as top-level `SKILL.md` keys.

### Direct Per-DCC MCP Discovery

If your MCP connection is a direct Maya/Blender/Houdini/etc. server, do not
treat the first `tools/list` page as the complete tool index. `tools/list` is
paginated and may put a newly loaded tool on a later page.

Use this compact flow instead:

```python
# Direct per-DCC MCP workflow
hits = search_tools(query="capture viewport", limit=5)
info = get_skill_info(skill_name=hits["skill_candidates"][0]["skill_name"])
load_skill(skill_name=info["name"])
result = tools_call(name="maya_render__capture_viewport", arguments={})
```

Use `search_tools` for active tools and unloaded skill candidates. Use
`search_skills` when you are looking for a skill by intent rather than a known
tool name. Use `get_skill_info` to inspect a selected skill's full tool schemas
before loading it. If you intentionally call `tools/list`, follow every
`nextCursor` until it is absent.

### Gateway Dynamic-Capability / REST Surfaces

If your MCP connection is the multi-DCC gateway, do not expect backend actions to appear directly in `tools/list`. The gateway surface is intentionally fixed and bounded; use the dynamic-capability workflow instead:

```python
# Gateway MCP four-tool workflow
hits = search(kind="tool", query="create sphere", dcc_type="maya", limit=5)
info = describe(tool_slug=hits["hits"][0]["tool_slug"])
result = call(tool_slug=info["record"]["tool_slug"], arguments={"radius": 2.0})

# Ordered MCP batch flow (max 25 calls)
batch = call(
    calls=[
        {"tool_slug": info["record"]["tool_slug"], "arguments": {"radius": 2.0}},
        {"tool_slug": "maya.a1b2c3d4.assign_material", "arguments": {"name": "mat_blue"}},
    ],
    stop_on_error=True,
)
```

Use `search(kind="skill", ...)` to find unloaded skills, then `load_skill(skill_name="...", instance_id="...")` when a search hit's `next_step` asks for activation. Gateway `tools/list` advertises exactly `search`, `describe`, `load_skill`, and `call`. Hidden MCP compatibility routes still accept older `search_tools` / `describe_tool` / `call_tool` / `call_tools` names, but new agent workflows should use the four canonical tools.

Wrapper payloads accept only `tool_slug`, `arguments`, and optional `meta`. Put backend-specific inputs such as `code`, `script`, `file_path`, or `radius` inside `arguments`, never at the wrapper top level. `dcc-mcp-wire` normalizes missing / `null` / empty-string arguments to `{}` and rejects non-object roots; Python host wrappers can call `dcc_mcp_core.host.normalize_tool_arguments()` / `normalize_tool_meta()`.

For ad-hoc script execution, prefer typed tools first, then materialize source
on the DCC host and execute by path. Use
`dcc_mcp_core.materialize_script(content, dcc_type=..., instance_id=..., session_id=...)`
to write under the configurable `~/.dcc-mcp/<dcc_type>/temp/<instance_id>/<session_id>/`
store and receive a descriptor with `file_ref`, `file_path`, `sha256`,
`bytes`, TTL, session, tool-call, and correlation metadata. `write_temp_script`
is still available for compatibility, but the structured descriptor is the
auditable contract.

Core script execution helpers now normalize through
`script_materialization_policy = off | auto | require`. The default `auto`
mode transparently turns inline `code` into a materialized host-local
`file_path` before execution. Use `require` when an adapter boundary must reject
raw inline code, and use `off` only as a short-lived compatibility escape hatch.
Execution results should return `context.materialized_script` with `path`,
`file_ref`, `sha256`, `bytes`, and `reused` metadata; legacy spilled-script
context keys are migration aliases, not the preferred contract.

Agents can also call the `materialize_script` MCP/REST tool exposed by
`DccServerBase` adapters. Discover it with `search_tools("materialize script")`,
call it with `content` (or legacy `code`), then pass the returned `file_path`
to the execution tool. The tool returns FileRef/path/hash/TTL/session metadata
and never echoes raw source. Gateway traces and admin audit rows redact
script-source input fields by default and keep the descriptor metadata instead.

Pure HTTP clients use the same REST endpoints directly: `POST /v1/search`, `POST /v1/describe`, `POST /v1/call`, and gateway `POST /v1/call_batch`. Gateway REST returns compact TOON by default; send `Accept: application/json` or body `response_format: "json"` when a legacy JSON client needs compatibility. See `docs/guide/gateway.md` and `docs/guide/rest-api-surface.md`.

### Gateway workflow guide (`gateway://docs/agent-workflows`)

**`resources/read`** with **`uri=gateway://docs/agent-workflows`** is the **platform-agnostic** copy bundled with the gateway: MCP **tools** vs **`resources/list`/`read`** / **`prompts`**, using **`describe`** (schema, **affinity**, execution mode, timeouts), fewer redundant round-trips, optional **`call({calls:[...]})`** / **`POST /v1/call_batch`** (≤25 ordered steps), and reading **host-published help** URIs exactly as listed—never inventing schemes. Re-fetch in very long sessions if the contract might have fallen out of context.

### Gateway Instance Discovery

Usually you do **not** need to enumerate instances: let gateway `search` and `call` route for you. When you must pick a concrete DCC session, inspect context metadata, or connect directly, read the gateway-native MCP resource instead of looking for instance-discovery tools:

```python
# MCP request shape; use your client's resources/read helper if it has one.
{"method": "resources/read", "params": {"uri": "gateway://instances"}}
{"method": "resources/read", "params": {"uri": "gateway://instances/{instance_id}"}}
```

Each entry carries `mcp_url`, so no separate connect verb is needed. The legacy `list_dcc_instances`, `get_dcc_instance`, `connect_to_dcc`, and non-standard `instances/list` surfaces were removed in #813 phase 1.

### Gateway Resources and Prompts


Use MCP resources for files, scene artefacts, thumbnails, diagnostics, and other hand-off data that should not be squeezed into tool text output:

1. Call `resources/list` and keep the returned URI exactly as-is. Gateway-prefixed URIs encode the owning DCC instance (`dcc://<type>/<id>` or `<scheme>://<id8>/<rest>`).
2. `resources/list` advertises `gateway://instances` as one root pointer; read `gateway://instances/{id}` directly when you know an instance id because per-instance URIs are intentionally not fanned out.
3. Call `resources/read` with that exact URI. Do not remove or rewrite the instance prefix client-side.
4. Optional: **`resources/read` `uri=gateway://docs/agent-workflows`** — same content as the subsection above; use one or the other as a reminder in long sessions.
5. Use `resources/subscribe` only when you need live `notifications/resources/updated` events, then call `resources/unsubscribe` when done.
6. Prefer resources over ad-hoc local file paths in tool messages; resources are portable across DCC hosts and easier for agents to trace.
7. For reusable prompt templates, call gateway `prompts/list` and then `prompts/get` with the returned namespaced prompt name.

### Gateway Admin Observability

When debugging routing, slow calls, or worker availability, use the elected gateway's read-only admin JSON APIs before guessing from logs: `GET /admin/api/instances`, `/tools`, `/calls`, `/traces`, `/traces/{request_id}`, `/stats?range=24h`, `/workers`, `/logs`, and `/health`. The `/logs` feed merges gateway contention events, on-disk `*.log` rows from `DCC_MCP_LOG_DIR` (or the platform default), and audited call summaries. The HTML dashboard remains `GET /admin`; disable it with `--no-admin`, `DCC_MCP_NO_ADMIN=true`, or `cfg.admin_enabled = False`. For restart-stable call/trace history, operators can set `DCC_MCP_GATEWAY_AUDIT_DIR` to persist `audit.jsonl` and `traces.jsonl`.

## 📚 Key Concepts You Must Understand

### 1. scan_and_load Returns a 2-Tuple

```python
# ✓ CORRECT - always unpack both values
skills, skipped = scan_and_load(dcc_name="maya")

# ✗ WRONG - don't iterate directly
for skill in scan_and_load(...):  # BREAKS - returns tuple, not list
```

### 2. ToolResult Structure

Always use the provided factories (`success_result`, `error_result`) — never hand-roll dicts:

```python
from dcc_mcp_core import success_result, error_result

# ✓ CORRECT - use factories
result = success_result("Created sphere", prompt="Add material next", count=5)
# result.to_dict() -> {"success": True, "message": "...", "context": {"count": 5}}

# ✗ WRONG - hand-rolled dict
result = {"success": True, "message": "..."}  # Missing context, not forward-compatible
```

### 3. Tool Annotations for Safety

Tools declare their safety hints via `ToolAnnotations`:

- `read_only_hint=True` — does not modify state (safe to call)
- `destructive_hint=True` — modifies state, possibly irreversible
- `idempotent_hint=True` — safe to call multiple times

**Always check annotations before calling tools on production scenes.**

### 4. Progressive Loading with Tool Groups

Skills can expose tools progressively:

```python
# List all declared groups as (skill_name, group_name, active) tuples.
groups = catalog.list_groups()

# Activate/deactivate by group name.
catalog.activate_group("advanced")
catalog.deactivate_group("experimental")
active = catalog.active_groups()
```

### 5. Lifecycle Hooks — Observe and Control

`LifecycleHooks` provides a typed, fail-safe observer system for skill/tool/session events:

- **Policy events** (`BEFORE_SKILL_LOAD`, `BEFORE_TOOL_CALL`, `BEFORE_SEARCH`): Raise `HookDeny` to veto
- **Observation events** (`AFTER_*`, `SESSION_*`): Log and analytics only — exceptions are swallowed

```python
from dcc_mcp_core import LifecycleHooks, HookEvent, HookDeny

hooks = LifecycleHooks()

@hooks.on(HookEvent.BEFORE_TOOL_CALL)
def block_dangerous(ctx):
    if "dangerous" in ctx.payload.get("tool_name", ""):
        raise HookDeny("blocked", hint="use the safe alternative")

server.register_lifecycle_hooks(hooks)
```

### 6. Agent Memory — Automatic Context Retention

`MemoryRecorder` automatically records skill/tool outcomes and injects memory
summaries into search and tool-call context — no manual logging needed:

```python
from dcc_mcp_core import InMemoryMemoryStore, MemoryRecorder

store = InMemoryMemoryStore()
MemoryRecorder(store).install(hooks)  # wires 6 lifecycle events
# From now on: skill loads → EPHEMERAL, tool calls → WORKING,
# session end → compacted to LONGTERM patterns
# BEFORE_SEARCH and BEFORE_TOOL_CALL auto-inject memory_summary
```

## 🔧 Common Tasks — Which API to Use

| Task | Use this API |
|------|---------------|
| **Expose DCC tools over MCP** | `DccServerOptions.from_env(...)` → `DccServerBase(opts)` → `start()` |
| **Zero-code tool registration** | agentskills.io `SKILL.md` + `metadata.dcc-mcp.tools` pointing at sibling `tools.yaml` + `scripts/` |
| **Return structured results** | `success_result()` / `error_result()` |
| **Rich error with traceback** | `skill_error_with_trace()` |
| **Bridge non-Python DCC** | `DccBridge` (WebSocket JSON-RPC 2.0) |
| **Register lifecycle hooks** | `LifecycleHooks()` + `server.register_lifecycle_hooks(hooks)` |
| **Enable agent memory** | `MemoryRecorder(InMemoryMemoryStore()).install(hooks)` |
| **Register all built-in tools** | `register_all_builtin_skills(server, dcc_name=..., skills=...)` |
| **IPC between processes** | `IpcChannelAdapter` / `SocketServerAdapter` |
| **Hand off files between tools** | `FileRef` + `artefact_put_file()` / `artefact_get_bytes()` |
| **Multi-DCC gateway** | `McpHttpConfig(gateway_port=9765)` |
| **Long-lived cancellation support** | `check_cancelled()` / `check_dcc_cancelled()` |

## 🎭 Skill Authoring for AI Agents

When creating skills, optimize for AI agent discoverability:

### Description Pattern (Required)

Every skill `description` must follow this 3-part structure (max 1024 chars):

```
<Layer> skill — <one-sentence what + scope keywords>. Use when <trigger>.
Not for <counter-example> — use <other-skill> for that.
```

**Example (Domain skill):**
```yaml
description: >-
  Domain skill — Maya polygon geometry: create spheres, cubes, cylinders;
  bevel and extrude polygon components. Use when the user asks to create or
  modify 3D meshes in Maya. Not for USD export pipelines — use
  maya-pipeline for that. Not for raw USD file inspection — use usd-tools for that.
```

### search-hint Optimization

Include specific keywords that AI agents will match against:

```yaml
metadata:
  dcc-mcp:
    search-hint: "polygon modeling, bevel, extrude, mesh creation, Maya geometry"
```

### next-tools Chains

Always provide follow-up guidance in the sibling `tools.yaml` referenced by `metadata.dcc-mcp.tools`:

```yaml
# tools.yaml
tools:
  - name: create_sphere
    next-tools:
      on-success: [maya_geometry__bevel_edges, maya_geometry__apply_material]
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
```

## 🚫 Top Traps — Memorize These

1. **`scan_and_load` returns a 2-tuple** → `skills, skipped = scan_and_load(...)`
2. **`success_result` kwargs become context** → `success_result("msg", count=5)` — never `context=`
3. **`ToolDispatcher` uses `.dispatch()`** → never `.call()`
4. **Register ALL handlers BEFORE `server.start()`**
5. **SKILL.md extensions use `metadata.dcc-mcp.<feature>`** → sibling files, never top-level extension keys
6. **Use `dcc_mcp_core.METADATA_*` / `LAYER_*` / `CATEGORY_*`** → re-exported at top level
7. **Gateway wrappers accept only `tool_slug`, `arguments`, `meta`** → backend inputs go inside `arguments`
8. **Return `ToolResult` from Python tool handlers** → `ToolResult.ok("...", **ctx).to_dict()`
9. **Lifecycle hooks: policy events veto, observation events don't** → `BEFORE_*` events propagate `HookDeny`; `AFTER_*` events swallow it
10. **Agent memory: `install()` is mandatory** → `MemoryRecorder` does nothing until wired to `LifecycleHooks` via `.install(hooks)`

## 📖 Further Reading

- **Navigation map**: [`AGENTS.md`](AGENTS.md) — start here for detailed rules
- **API index**: [`llms.txt`](llms.txt) — compressed API reference for AI agents
- **Skill authoring guide**: [`docs/guide/skills.md`](docs/guide/skills.md) — current SKILL.md + sibling-file pattern
- **Skill ownership policy**: [`docs/POLICY_SKILL_OWNERSHIP.md`](docs/POLICY_SKILL_OWNERSHIP.md) — avoid duplicating bundled adapter file-operation skills
- **Bundled examples**: [`examples/skills/`](examples/skills/) — complete SKILL.md packages
- **Detailed traps**: [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md)
- **Lifecycle hooks reference**: [`docs/guide/agents-reference.md#lifecycle-hooks-typed-observerpub-sub-1337`](docs/guide/agents-reference.md#lifecycle-hooks-typed-observerpub-sub-1337)
- **Agent memory reference**: [`docs/guide/agents-reference.md#agent-memory-three-tier-1334`](docs/guide/agents-reference.md#agent-memory-three-tier-1334)

## 💡 Pro Tips for AI Agents

1. **Always search before assuming** — use `search_skills()` to discover relevant tools
2. **Read tool annotations** — respect safety hints (`read_only`, `destructive`)
3. **Follow next-tools chains** — they guide you through complex workflows
4. **Handle errors gracefully** — check `error_result` and follow `prompt` suggestions
5. **Use progressive loading** — don't load all skills at once, activate groups as needed
6. **Prefer MCP tools over raw scripting** — they provide validation, safety, and traceability
7. **Check cancellations** — in long-running tools, periodically call `check_cancelled()`
8. **Wire lifecycle hooks for policy control** — use `BEFORE_TOOL_CALL` + `HookDeny` to block dangerous operations without modifying tool code
9. **Enable agent memory for smarter searches** — `MemoryRecorder` auto-injects `memory_prefer_tools`/`memory_avoid_tools` so search ranking improves over time
10. **Use `register_all_builtin_skills` for a complete baseline** — one call registers diagnostics, introspection, feedback, recipes, UI inspector, and script materialization tools

---

**Remember**: When in doubt, read `AGENTS.md` → `docs/guide/agents-reference.md` → `llms.txt`. The documentation hierarchy is designed for progressive disclosure.
