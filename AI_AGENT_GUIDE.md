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
    # Check for next-tools guidance
    if "next-tools" in result:
        print(f"Suggested next tools: {result['next-tools']}")
else:
    print(f"Tool failed: {result.get('error')}")
    print(f"Suggestion: {result.get('prompt')}")
```

### 4. Follow next-tools Guidance

When a tool returns `next-tools.on-success` or `next-tools.on-failure`, **always consider calling those tools next**. This creates a guided workflow chain.

### Gateway / Slim / REST Surfaces

If your MCP connection is the multi-DCC gateway, especially with `gateway_tool_exposure="slim"` or `"rest"`, do not expect every backend tool to appear in `tools/list`. Use the bounded dynamic-capability workflow instead:

```python
# Gateway MCP wrapper flow
hits = search_tools(query="create sphere", dcc_type="maya", limit=5)
info = describe_tool(tool_slug=hits["hits"][0]["tool_slug"])
result = call_tool(tool_slug=info["tool_slug"], arguments={"radius": 2.0})
```

Non-MCP clients use the equivalent REST endpoints: `POST /v1/search`, `POST /v1/describe`, and `POST /v1/call`. See `docs/guide/gateway.md` and `docs/guide/dcc-rest-skill-api.md`.

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

## 🔧 Common Tasks — Which API to Use

| Task | Use this API |
|------|---------------|
| **Expose DCC tools over MCP** | `DccServerBase` → subclass → `start()` |
| **Zero-code tool registration** | `SKILL.md` + sibling `tools.yaml` + `scripts/` (agentskills.io-compatible format) |
| **Return structured results** | `success_result()` / `error_result()` |
| **Rich error with traceback** | `skill_error_with_trace()` |
| **Bridge non-Python DCC** | `DccBridge` (WebSocket JSON-RPC 2.0) |
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
  dcc-mcp.search-hint: "polygon modeling, bevel, extrude, mesh creation, Maya geometry"
```

### next-tools Chains

Always provide follow-up guidance:

```yaml
tools:
  - name: create_sphere
    next-tools:
      on-success: [maya-geometry__bevel_edges, maya-geometry__apply_material]
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
```

## 🚫 Top Traps — Memorize These

1. **`scan_and_load` returns a 2-tuple** → `skills, skipped = scan_and_load(...)`
2. **`success_result` kwargs become context** → `success_result("msg", count=5)` — never `context=`
3. **`ToolDispatcher` uses `.dispatch()`** → never `.call()`
4. **Register ALL handlers BEFORE `server.start()`**
5. **SKILL.md extensions use `metadata.dcc-mcp.<feature>`** → sibling files, never top-level extension keys
6. **Use `dcc_mcp_core.METADATA_*` / `LAYER_*` / `CATEGORY_*`** → re-exported at top level
7. **Return `ToolResult` from Python tool handlers** → `ToolResult.ok("...", **ctx).to_dict()`

## 📖 Further Reading

- **Navigation map**: [`AGENTS.md`](AGENTS.md) — start here for detailed rules
- **API index**: [`llms.txt`](llms.txt) — compressed API reference for AI agents
- **Skill authoring guide**: [`docs/guide/skills.md`](docs/guide/skills.md) — current SKILL.md + sibling-file pattern
- **Bundled templates/examples**: [`skills/README.md`](skills/README.md) and [`examples/skills/`](examples/skills/) — 15 complete SKILL.md packages
- **Detailed traps**: [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md)

## 💡 Pro Tips for AI Agents

1. **Always search before assuming** — use `search_skills()` to discover relevant tools
2. **Read tool annotations** — respect safety hints (`read_only`, `destructive`)
3. **Follow next-tools chains** — they guide you through complex workflows
4. **Handle errors gracefully** — check `error_result` and follow `prompt` suggestions
5. **Use progressive loading** — don't load all skills at once, activate groups as needed
6. **Prefer MCP tools over raw scripting** — they provide validation, safety, and traceability
7. **Check cancellations** — in long-running tools, periodically call `check_cancelled()`

---

**Remember**: When in doubt, read `AGENTS.md` → `docs/guide/agents-reference.md` → `llms.txt`. The documentation hierarchy is designed for progressive disclosure.
