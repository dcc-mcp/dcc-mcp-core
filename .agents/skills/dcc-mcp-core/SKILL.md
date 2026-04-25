---
name: dcc-mcp-core
description: "Foundation library for the DCC Model Context Protocol (MCP) ecosystem. Provides Rust-powered action management, skills system, IPC transport, MCP Streamable HTTP server (2025-03-26 spec, with 2025-06-18 and 2025-11-25 awareness), sandbox security, shared memory, screen capture, USD scene support, and telemetry for AI-assisted DCC workflows. Use when working with Maya, Blender, Houdini, 3ds Max, or any DCC MCP integration."
allowed-tools: Bash Read Write Edit
compatibility: "Python 3.7-3.13; Rust 1.85+ required to build from source; zero runtime Python dependencies"
version: "0.14.13"  # x-release-please-version
---

# dcc-mcp-core — DCC MCP Ecosystem Foundation

The foundational library enabling AI assistants to interact with Digital Content Creation (DCC) software through the Model Context Protocol (MCP).

## Quick Decision Guide — Use the Right API

| Task | Use this | Not this |
|------|----------|----------|
| Return action result | `success_result()` / `error_result()` | raw dicts |
| Load skills | `scan_and_load()` → `(skills, skipped)` | manual file scanning |
| One-call MCP server | `create_skill_server("maya", McpHttpConfig(port=8765))` | manual wiring |
| Validate params | `ToolValidator.from_schema_json()` | isinstance checks |
| Connect to DCC | `IpcChannelAdapter.connect(name)` or `SocketServerAdapter(path)` | raw sockets |
| Define MCP tool | `ToolDefinition` + `ToolAnnotations` | raw JSON |
| Serve MCP over HTTP | `McpHttpServer(registry, McpHttpConfig(port=8765))` | raw HTTP server |
| Build DCC adapter | `DccServerBase(dcc_name, builtin_skills_dir)` | copy-paste boilerplate |
| Enable skill hot-reload | `DccSkillHotReloader(dcc_name, server)` | custom file watchers |
| Gateway failover | `DccGatewayElection(dcc_name, server)` | manual election logic |
| Write skill scripts | `skill_entry` + `skill_success` / `skill_error` | manual JSON output |

## What This Library Does

| Capability | Description |
|------------|-------------|
| **Action Management** | Register, validate, dispatch, and execute actions with typed inputs/outputs |
| **Skills System** | Zero-code script registration (Python/MEL/Batch/Shell/JS) as MCP tools via `SKILL.md` |
| **Transport Layer** | High-performance IPC via ipckit with DccLink framing (IpcChannelAdapter, SocketServerAdapter) |
| **MCP HTTP Server** | MCP Streamable HTTP (**2025-03-26 spec**) powered by axum/Tokio, runs in background thread |
| **Process Management** | Launch, monitor, auto-recover DCC processes (Maya, Blender, Houdini, etc.) |
| **Sandbox Security** | Policy-based access control, input validation, audit logging |
| **Shared Memory** | LZ4-compressed inter-process data exchange for large scenes |
| **Screen Capture** | Cross-platform DCC viewport capture for visual feedback |
| **USD Support** | Read/write Universal Scene Description for pipeline integration |
| **Telemetry** | Structured tracing and recording for observability |
| **MCP Protocol Types** | Complete Tool/Resource/Prompt schema implementations |
| **DCC Server Base** | Reusable base class for DCC adapters (hot-reload, gateway election, lifecycle) |
| **Gateway Failover** | Automatic gateway election when primary gateway becomes unreachable |
| **Skill Hot-Reload** | File-watching auto-reload for live skill development |

## Installation

```bash
pip install dcc-mcp-core
# Python 3.7-3.13, zero runtime dependencies
```

## Core Patterns

### Pattern 1: Skills-First — one-call MCP server (recommended)

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/opt/my-skills"

# One call: creates registry + dispatcher + catalog + discovers skills + server
server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Maya MCP server: {handle.mcp_url()}")

# Agents connect and use on-demand skill discovery:
# → search_skills(query="modeling") to find relevant skills
# → load_skill("maya-bevel") to activate
# → tools/call maya_bevel__bevel to execute
handle.shutdown()
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
from dcc_mcp_core import ToolValidator, error_result

schema = json.dumps({
    "type": "object",
    "required": ["name", "radius"],
    "properties": {
        "name": {"type": "string", "maxLength": 64},
        "radius": {"type": "number", "minimum": 0.001},
    },
})
validator = ToolValidator.from_schema_json(schema)
ok, errors = validator.validate(json.dumps(params))
if not ok:
    return error_result("Invalid parameters", "; ".join(errors))
```

### Pattern 4: Connect to a running DCC via IPC

```python
from dcc_mcp_core import DccLinkFrame, IpcChannelAdapter, success_result, error_result

# Connect to a DCC process via named pipe / Unix domain socket
channel = IpcChannelAdapter.connect("dcc-mcp-maya-12345")
try:
    # Send a Call frame and receive the reply
    channel.send_frame(DccLinkFrame(msg_type=1, seq=1, body=b'{"method":"execute_python","params":"cmds.sphere()"}'))
    reply = channel.recv_frame()  # DccLinkFrame
    if reply.msg_type == 2:  # Reply
        return success_result(reply.body.decode())
    else:
        return error_result("DCC call failed", reply.body.decode())
finally:
    channel.shutdown() if hasattr(channel, 'shutdown') else None
```

### Pattern 5: Build a DCC adapter with DccServerBase

```python
from pathlib import Path
from dcc_mcp_core.server_base import DccServerBase

class BlenderMcpServer(DccServerBase):
    def __init__(self, port: int = 8765, **kwargs):
        super().__init__(
            dcc_name="blender",
            builtin_skills_dir=Path(__file__).parent / "skills",
            port=port,
            **kwargs,
        )

    def _version_string(self) -> str:
        import bpy
        return bpy.app.version_string

# All skill methods, hot-reload, gateway are ready:
server = BlenderMcpServer(port=8765)
server.register_builtin_actions()
handle = server.start()
print(f"MCP: {handle.mcp_url()}")
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
from dcc_mcp_core import ToolRegistry, ToolDispatcher

reg = ToolRegistry()
reg.register("create_sphere",
    input_schema=json.dumps({"type": "object", "required": ["radius"],
                              "properties": {"radius": {"type": "number", "minimum": 0.0}}}))

dispatcher = ToolDispatcher(reg)
dispatcher.register_handler("create_sphere", lambda params: {"created": True, "r": params["radius"]})

# Introspect handlers
dispatcher.has_handler("create_sphere")  # True
dispatcher.handler_count()               # 1
dispatcher.handler_names()               # ["create_sphere"]
dispatcher.remove_handler("create_sphere")  # True

result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0}))
# result == {"action": "create_sphere", "output": {"created": True, "r": 2.0}, "validation_skipped": False}
```

### Pattern 8: DCC main-thread safety with DeferredExecutor

Most DCC applications (Maya, Blender, Houdini) require scene API calls on their **main thread**.
McpHttpServer runs on Tokio worker threads — use `DeferredExecutor` to bridge:

```python
# DeferredExecutor is Rust-backed; import directly until added to public API
from dcc_mcp_core._core import DeferredExecutor
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

executor = DeferredExecutor(queue_depth=64)
# In DCC main event loop / timer callback:
def poll():
    executor.poll_pending()  # runs queued tasks on main thread

# Maya: maya.utils.executeDeferred(poll)
# Blender: bpy.app.timers.register(poll, persistent=True)
# Houdini: hou.ui.addEventLoopCallback(poll)

registry = ToolRegistry()
server = McpHttpServer(registry, McpHttpConfig(port=0, server_name="maya-mcp"))
handle = server.start()
```

### Pattern 9: Write skill scripts with skill_entry

```python
from dcc_mcp_core.skill import skill_entry, skill_success, skill_error, skill_exception

@skill_entry
def create_sphere(radius: float = 1.0, name: str = "sphere") -> dict:
    import maya.cmds as cmds
    obj = cmds.polySphere(r=radius, n=name)[0]
    return skill_success(
        f"Created sphere '{obj}' with radius {radius}",
        prompt="You can now adjust properties or add materials.",
        object_name=obj,
        radius=radius,
    )
```

## Creating a Custom Skill (Zero Python Code)

```bash
# 1. Create directory structure
mkdir -p my-tool/scripts/

# 2. Write SKILL.md (name is required, follows agentskills.io spec)
cat > my-tool/SKILL.md << 'EOF'
---
name: my-tool
description: "My custom DCC automation tools. Use when automating scene setup or batch operations."
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
│  ~180 public symbols re-exported from Rust core      │
│  + Pure-Python: DccServerBase, DccGatewayElection,  │
│    DccSkillHotReloader, factory, skill helpers       │
└──────────────────────┬──────────────────────────────┘
                       │ PyO3 bindings
┌──────────────────────▼──────────────────────────────┐
│              Rust Core (15 Crates)                   │
│                                                      │
│  models → actions → skills → protocols              │
│  transport → http → process → sandbox → telemetry   │
│  shm → capture → usd → server → utils               │
└─────────────────────────────────────────────────────┘
```

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `DCC_MCP_SKILL_PATHS` | Colon/semicolon-separated paths to scan for `SKILL.md` dirs |
| `DCC_MCP_{APP}_SKILL_PATHS` | Per-app skill paths (e.g. `DCC_MCP_MAYA_SKILL_PATHS`) |
| `DCC_MCP_GATEWAY_PORT` | Gateway port for multi-DCC setup |
| `DCC_MCP_REGISTRY_DIR` | Directory for FileRegistry JSON |
| `MCP_LOG_LEVEL` | Log level override (`DEBUG`, `INFO`, `WARN`) |
| `DCC_MCP_IPC_ADDRESS` | IPC endpoint address (auto-set by register_diagnostic_handlers) |
| `DCC_MCP_GATEWAY_PROBE_INTERVAL` | Seconds between gateway health probes (default 5) |
| `DCC_MCP_GATEWAY_PROBE_TIMEOUT` | Timeout per probe in seconds (default 2) |
| `DCC_MCP_GATEWAY_PROBE_FAILURES` | Consecutive failures before election (default 3) |

## Key Files in This Repository

| File | Purpose |
|------|---------|
| `AGENTS.md` | AI agent guide — architecture, commands, pitfalls |
| `CLAUDE.md` | Claude-specific workflows and tips |
| `GEMINI.md` | Gemini-specific workflows and tips |
| `llms.txt` | Concise API reference for LLMs |
| `llms-full.txt` | Comprehensive API reference with all examples |
| `python/dcc_mcp_core/__init__.py` | Complete public API (~180 symbols, ground truth for imports) |
| `python/dcc_mcp_core/_core.pyi` | Type stubs — authoritative parameter names |
| `examples/skills/` | 11 complete skill package examples |
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

The library currently implements **MCP 2025-03-26** (Streamable HTTP). The ecosystem has since released:

| Version | Key Features | Status in dcc-mcp-core |
|---------|-------------|----------------------|
| 2025-03-26 | Streamable HTTP, Tool Annotations, OAuth 2.1 | **Implemented** |
| 2025-06-18 | Structured Tool Output, Elicitation, Resource Links, JSON-RPC batching removed, `MCP-Protocol-Version` header mandatory | Planned |
| 2025-11-25 | Icon metadata, Tasks (experimental), Sampling with tool calls, JSON Schema 2020-12, enhanced OAuth | Planned |

**AI Agents**: Do NOT implement draft features manually. Wait for `dcc-mcp-core` to expose them via `McpHttpServer`. Track progress at the GitHub repository.

## Common Pitfalls

1. `scan_and_load` returns `(List[SkillMetadata], List[str])` — always unpack: `skills, skipped = scan_and_load(...)`
2. `DeferredExecutor` not in public API yet — use `from dcc_mcp_core._core import DeferredExecutor`
3. Register ALL actions before `server.start()` — server reads from registry at startup only
4. Use `IpcChannelAdapter` + `DccLinkFrame` for IPC (v0.14+) — `FramedChannel`/`connect_ipc` were removed in #251
5. `ToolDispatcher(registry)` takes ONE arg — no `validator=` parameter
6. Action naming: `{skill_name.replace('-','_')}__{script_stem}` (double underscore)
7. SKILL.md `name` must match parent directory name (agentskills.io spec)
8. `allowed-tools` in SKILL.md is space-separated string, not a list (agentskills.io spec)
9. `DccServerBase` provides all skill/lifecycle/gateway/hot-reload methods — don't reimplement
10. MCP 2025-06-18 removes JSON-RPC batching — do not implement batch calls manually
11. `MCP-Protocol-Version` header is mandatory in 2025-06-18 — handled by McpHttpServer internally
