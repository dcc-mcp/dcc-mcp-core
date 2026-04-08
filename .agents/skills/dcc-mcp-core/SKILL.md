---
name: dcc-mcp-core
description: "Foundation library for the DCC Model Context Protocol (MCP) ecosystem. Provides Rust-powered action management, skills system, IPC transport, MCP Streamable HTTP server (2025-03-26 spec), sandbox security, shared memory, screen capture, USD scene support, and telemetry for AI-assisted DCC workflows. Use when working with Maya, Blender, Houdini, 3ds Max, or any DCC MCP integration."
allowed-tools: ["Bash", "Read", "Write", "Edit"]
compatibility: "Python 3.7-3.13; Rust 1.85+ required to build from source; zero runtime Python dependencies"
version: "0.12.9"
---

# dcc-mcp-core — DCC MCP Ecosystem Foundation

The foundational library enabling AI assistants to interact with Digital Content Creation (DCC) software through the Model Context Protocol (MCP).

## Quick Decision Guide — Use the Right API

| Task | Use this | Not this |
|------|----------|----------|
| Return action result | `success_result()` / `error_result()` | raw dicts |
| Load skills | `scan_and_load()` → `(skills, skipped)` | manual file scanning |
| Validate params | `ActionValidator.from_schema_json()` | isinstance checks |
| Connect to DCC | `connect_ipc(TransportAddress.default_local(...))` | raw sockets |
| Define MCP tool | `ToolDefinition` + `ToolAnnotations` | raw JSON |
| Serve MCP over HTTP | `McpHttpServer(registry, McpHttpConfig(port=8765))` | raw HTTP server |

## What This Library Does

| Capability | Description |
|------------|-------------|
| **Action Management** | Register, validate, dispatch, and execute actions with typed inputs/outputs |
| **Skills System** | Zero-code script registration (Python/MEL/Batch/Shell/JS) as MCP tools via `SKILL.md` |
| **Transport Layer** | High-performance IPC with connection pooling, circuit breaker, retry policies |
| **MCP HTTP Server** | MCP Streamable HTTP (**2025-03-26 spec**) powered by axum/Tokio, runs in background thread. 2025-11-05 draft will add JSON-RPC batching + resource links |
| **Process Management** | Launch, monitor, auto-recover DCC processes (Maya, Blender, Houdini, etc.) |
| **Sandbox Security** | Policy-based access control, input validation, audit logging |
| **Shared Memory** | LZ4-compressed inter-process data exchange for large scenes |
| **Screen Capture** | Cross-platform DCC viewport capture for visual feedback |
| **USD Support** | Read/write Universal Scene Description for pipeline integration |
| **Telemetry** | Structured tracing and recording for observability |
| **MCP Protocol Types** | Complete Tool/Resource/Prompt schema implementations |

## Installation

```bash
pip install dcc-mcp-core
# Python 3.7-3.13, zero runtime dependencies
```

## Core Patterns

### Pattern 1: Skills → MCP tools

```python
import os
from dcc_mcp_core import scan_and_load, ActionRegistry, ToolDefinition, ToolAnnotations
from pathlib import Path

os.environ["DCC_MCP_SKILL_PATHS"] = "/opt/my-skills"

# IMPORTANT: scan_and_load returns a 2-tuple
skills, skipped = scan_and_load(dcc_name="maya")

registry = ActionRegistry()
tools = []
for skill in skills:
    for script_path in skill.scripts:
        stem = Path(script_path).stem
        action_name = f"{skill.name.replace('-', '_')}__{stem}"
        registry.register(
            name=action_name,
            description=skill.description,
            dcc=skill.dcc,
            tags=skill.tags,
        )
        tools.append(ToolDefinition(
            name=action_name,
            description=skill.description,
            input_schema='{"type": "object"}',
            annotations=ToolAnnotations(read_only_hint=False),
        ))
```

### Pattern 2: Return structured results (always use factories)

```python
from dcc_mcp_core import success_result, error_result, from_exception

# All actions should return ActionResultModel
def my_action(params):
    try:
        result = do_work(params)
        return success_result(
            f"Created {result['name']}",
            prompt="Object created. You can now modify its properties.",
            object_name=result["name"],
        )
    except Exception as e:
        return from_exception(str(e), message="Action failed")
```

### Pattern 3: Validate action inputs

```python
import json
from dcc_mcp_core import ActionValidator, error_result

schema = json.dumps({
    "type": "object",
    "required": ["name", "radius"],
    "properties": {
        "name": {"type": "string", "maxLength": 64},
        "radius": {"type": "number", "minimum": 0.001},
    },
})
validator = ActionValidator.from_schema_json(schema)
ok, errors = validator.validate(json.dumps(params))
if not ok:
    return error_result("Invalid parameters", "; ".join(errors))
```

### Pattern 4: Connect to a running DCC and call it

```python
from dcc_mcp_core import TransportAddress, connect_ipc, success_result, error_result

addr = TransportAddress.default_local("maya", pid=12345)
channel = connect_ipc(addr)
try:
    # Primary RPC: .call() sends request + waits for correlated response (v0.12.7+)
    result = channel.call("execute_python", b'import maya.cmds; cmds.sphere()', timeout_ms=10000)
    if result["success"]:
        payload = result.get("payload", b"")
        return success_result(payload.decode() if isinstance(payload, bytes) else str(payload))
    else:
        return error_result("DCC call failed", result.get("error", "Unknown error"))
finally:
    channel.shutdown()
```

### Pattern 5: Expose actions via MCP Streamable HTTP

```python
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig

registry = ActionRegistry()
registry.register("get_scene_info", description="Get DCC scene info",
                  category="scene", dcc="maya", version="1.0.0")

# Runs in background thread — safe to call from DCC main thread
server = McpHttpServer(registry, McpHttpConfig(port=8765, server_name="maya-mcp"))
handle = server.start()
print(f"MCP server: {handle.mcp_url()}")  # http://127.0.0.1:8765/mcp
# Claude Desktop / MCP host connects to handle.mcp_url()
# handle.shutdown() when done
```

### Pattern 6: Watch skills for live reload

```python
from dcc_mcp_core import SkillWatcher

watcher = SkillWatcher(debounce_ms=300)
watcher.watch("/my/dev/skills")  # immediate load + start watching

# Get always-up-to-date snapshot
current_skills = watcher.skills()  # -> List[SkillMetadata]
```

### Pattern 7: ActionDispatcher with handlers

```python
import json
from dcc_mcp_core import ActionRegistry, ActionDispatcher

reg = ActionRegistry()
reg.register("create_sphere",
    input_schema=json.dumps({"type": "object", "required": ["radius"],
                              "properties": {"radius": {"type": "number", "minimum": 0.0}}}))

dispatcher = ActionDispatcher(reg)
dispatcher.register_handler("create_sphere", lambda params: {"created": True, "r": params["radius"]})
result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0}))
# result == {"action": "create_sphere", "output": {"created": True, "r": 2.0}, "validation_skipped": False}
```

### Pattern 8: DCC main-thread safety with DeferredExecutor

Most DCC applications (Maya, Blender, Houdini) require scene API calls on their **main thread**.
McpHttpServer runs on Tokio worker threads — use `DeferredExecutor` to bridge:

```python
# DeferredExecutor is Rust-backed; import directly until added to public API
from dcc_mcp_core._core import DeferredExecutor
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig

executor = DeferredExecutor(queue_depth=64)
# In DCC main event loop / timer callback:
def poll():
    executor.poll_pending()  # runs queued tasks on main thread

# Maya: maya.utils.executeDeferred(poll)
# Blender: bpy.app.timers.register(poll, persistent=True)
# Houdini: hou.ui.addEventLoopCallback(poll)

registry = ActionRegistry()
server = McpHttpServer(registry, McpHttpConfig(port=0, server_name="maya-mcp"))
handle = server.start()
```

## Creating a Custom Skill (Zero Python Code)

```bash
# 1. Create directory structure
mkdir -p my-tool/scripts/

# 2. Write SKILL.md (name and dcc are required fields)
cat > my-tool/SKILL.md << 'EOF'
---
name: my-tool
description: "My custom DCC automation tools"
allowed-tools: ["Bash"]
tags: ["automation", "custom"]
dcc: maya
version: "1.0.0"
---

# My Tool

Automation scripts for Maya workflow optimization.
EOF

# 3. Add a script
cat > my-tool/scripts/list_selected.py << 'PYEOF'
#!/usr/bin/env python3
"""List selected objects in the Maya scene."""
import json

result = {"selected": ["pSphere1", "pCube1"], "count": 2}
print(json.dumps(result))
PYEOF

# 4. Use it
export DCC_MCP_SKILL_PATHS="$(pwd)/my-tool"
python -c "
from dcc_mcp_core import scan_and_load
skills, _ = scan_and_load(dcc_name='maya')
print(f'Loaded: {[s.name for s in skills]}')
# Action: my_tool__list_selected
"
```

## Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│                   Python Layer                       │
│  dcc_mcp_core/__init__.py  →  _core (PyO3 cdyll)   │
│  ~130 public symbols re-exported from Rust core      │
└──────────────────────┬──────────────────────────────┘
                       │ PyO3 bindings
┌──────────────────────▼──────────────────────────────┐
│              Rust Core (12 Crates)                   │
│                                                      │
│  models → actions → skills → protocols              │
│  transport → http → process → sandbox → telemetry   │
│  shm → capture → usd → utils                        │
└─────────────────────────────────────────────────────┘
```

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `DCC_MCP_SKILL_PATHS` | Colon/semicolon-separated paths to scan for `SKILL.md` dirs |
| `MCP_LOG_LEVEL` | Log level override (`DEBUG`, `INFO`, `WARN`) |

## Key Files in This Repository

| File | Purpose |
|------|---------|
| `AGENTS.md` | AI agent guide — architecture, commands, pitfalls |
| `CLAUDE.md` | Claude-specific workflows and tips |
| `GEMINI.md` | Gemini-specific workflows and tips |
| `llms.txt` | Concise API reference for LLMs |
| `llms-full.txt` | Comprehensive API reference with all examples |
| `python/dcc_mcp_core/__init__.py` | Complete public API (ground truth for imports) |
| `python/dcc_mcp_core/_core.pyi` | Type stubs — authoritative parameter names |
| `examples/skills/` | 9 complete skill package examples |
| `tests/` | Python integration tests (executable usage examples) |

## Supported DCC Software

- **Autodesk Maya** — MEL/Python scripting  (`dcc: maya`)
- **Blender** — Python API  (`dcc: blender`)
- **SideFX Houdini** — HScript/Python  (`dcc: houdini`)
- **Autodesk 3ds Max** — MaxScript/Python  (`dcc: 3dsmax`)
- **Any DCC** — Generic Python wrapper  (`dcc: python`)

## Related Projects

- [dcc-mcp-rpyc](https://github.com/loonghao/dcc-mcp-rpyc) — RPyC bridge for remote DCC operations
- [dcc-mcp-maya](https://github.com/loonghao/dcc-mcp-maya) — Maya MCP server implementation

## MCP Specification Roadmap

The library currently implements **MCP 2025-03-26** (Streamable HTTP). The **2025-11-05 draft** introduces:

| Feature | Status | Notes |
|---------|--------|-------|
| JSON-RPC Batching | Draft | Multiple tool calls per round-trip; reduces latency for bulk ops |
| Resource Links in Tool Results | Draft | Tools can return resource URIs for dynamic discovery |
| Event Streams | Draft | Server-initiated push notifications for state changes |
| Improved Error Taxonomy | Draft | Finer-grained error codes for better client handling |

**AI Agents**: Do NOT implement these features manually. Wait for `dcc-mcp-core` to expose them via `McpHttpServer`. Track progress at the GitHub repository.

## Common Pitfalls

1. `scan_and_load` returns `(List[SkillMetadata], List[str])` — always unpack: `skills, skipped = scan_and_load(...)`
2. `DeferredExecutor` not in public API yet — use `from dcc_mcp_core._core import DeferredExecutor`
3. Register ALL actions before `server.start()` — server reads from registry at startup only
4. Use `FramedChannel.call()` for sync RPC — not `send_request()` + `recv()` (those are for async/multiplex)
5. `ActionDispatcher(registry)` takes ONE arg — no `validator=` parameter
6. Action naming: `{skill_name.replace('-','_')}__{script_stem}` (double underscore)
