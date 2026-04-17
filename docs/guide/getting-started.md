# Getting Started

## Installation

### From PyPI

```bash
pip install dcc-mcp-core
```

### From Source (requires Rust toolchain)

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

::: tip
Building from source requires the Rust toolchain. Install it from [rustup.rs](https://rustup.rs/).
The build is handled by [maturin](https://www.maturin.rs/) which compiles the Rust core and installs the Python package.
:::

## Requirements

- **Python**: >= 3.7 (CI tests 3.7, 3.8, 3.9, 3.10, 3.11, 3.12, 3.13)
- **Rust**: >= 1.85 (for building from source)
- **License**: MIT
- **Python Dependencies**: Zero — everything is in the compiled Rust extension

## Quick Start

### Skills-First: `create_skill_server` (recommended since v0.12.12)

The fastest way to expose scripts as MCP tools. Create a `SKILL.md` in your script folder, then use `create_skill_server` to wire everything in one call:

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

# Point to your skill directories (per-app env var)
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/my-skills"

# One call: discover skills + start MCP HTTP server
server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Maya MCP server at {handle.mcp_url()}")
# AI clients (Claude Desktop, etc.) connect to http://127.0.0.1:8765/mcp
```

Or use `SkillCatalog` directly for more control:

```python
import os
from dcc_mcp_core import SkillCatalog, ToolRegistry

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/my-skills"

registry = ToolRegistry()
catalog = SkillCatalog(registry)

discovered = catalog.discover(dcc_name="maya")
print(f"Discovered {discovered} skills")

# Load a skill and inspect the registered tool names
tool_names = catalog.load_skill("maya-geometry")
print(tool_names)
```

See the [Skills System guide](/guide/skills) for writing `SKILL.md` files and advanced options.

### Tool Registry

```python
from dcc_mcp_core import ToolRegistry

registry = ToolRegistry()
registry.register(
    name="create_sphere",
    description="Creates a sphere in the scene",
    category="geometry",
    tags=["geometry", "creation"],
    dcc="maya",
)

tool = registry.get_action("create_sphere")
print(tool)  # dict with tool metadata

maya_tools = registry.list_actions(dcc_name="maya")
```

:::: info Action → Tool terminology
In v0.13+, the project renamed "action" → "tool" at the conceptual level. However, some Rust API method names (`get_action`, `list_actions`, `search_actions`) still use "action" for backward compatibility. These are not bugs — they are compatibility aliases.
::::

### Tool Results

```python
from dcc_mcp_core import success_result, error_result

result = success_result("Created 5 spheres", prompt="Use modify next", count=5)
print(result.success)  # True
print(result.message)  # "Created 5 spheres"
print(result.context)  # {"count": 5}

err = error_result("Failed", "File not found", prompt="Check path")
print(err.success)  # False
```

### Event Bus

```python
from dcc_mcp_core import EventBus

bus = EventBus()
sid = bus.subscribe("scene.changed", lambda: print("Scene updated!"))
bus.publish("scene.changed")
bus.unsubscribe("scene.changed", sid)
```

### MCP HTTP Server

Expose your registry to AI clients (Claude Desktop, etc.) over HTTP in one call:

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

registry = ToolRegistry()
# ... register tools or load skills ...

config = McpHttpConfig(port=8765)
server = McpHttpServer(registry, config)
handle = server.start()

print(f"MCP server running at {handle.mcp_url()}")
# handle.shutdown() to shut down
```

### Instance-Bound Diagnostics

When multiple DCC instances run side-by-side (two Maya processes, Maya +
Blender, etc.), each adapter server should be bound to **its own** DCC
process so diagnostics (screenshot, audit log, metrics) target the right
window and PID.

`DccServerBase` accepts three optional instance-binding kwargs and exposes
four `diagnostics__*` MCP tools:

```python
from dcc_mcp_core import DccServerBase

class MayaServer(DccServerBase):
    def __init__(self, pid: int, window_title: str):
        super().__init__(
            dcc_name="maya",
            builtin_skills_dir=None,
            dcc_pid=pid,                   # owner DCC PID
            dcc_window_title=window_title, # fallback match when PID lookup fails
            # dcc_window_handle=0x00A1B2,  # or pass an HWND directly
        )

server = MayaServer(pid=12345, window_title="Autodesk Maya 2024")
handle = server.start()  # exposes diagnostics__screenshot / audit_log /
                         # action_metrics / process_status tools bound to
                         # this Maya instance only
```

If the PID can change at runtime (e.g. the user relaunches Maya), pass a
lazy `resolver` callable instead of `dcc_pid`:

```python
def current_maya_pid() -> int | None:
    return _find_maya_pid()    # evaluated on every diagnostics call

server = DccServerBase("maya", resolver=current_maya_pid, ...)
```

For low-level servers built around `McpHttpServer` directly, call
`register_diagnostic_mcp_tools(server, dcc_name=..., dcc_pid=...)` **before**
`server.start()` — per the "register all actions before start" rule.

## Development Setup

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# Install with vx (recommended)
vx just install

# Or manual setup
pip install maturin
maturin develop
```

## Running Tests

```bash
vx just test
vx just lint
```

## Next Steps

- Learn about [Tools & Registry](/guide/actions) — the tool registration layer
- Explore [Events & Telemetry](/api/events) for lifecycle hooks and lightweight execution metrics
- Check out the [Skills System](/guide/skills) for zero-code script registration
- Expose tools with [MCP HTTP Server](/api/http)
- See the [Transport Layer](/guide/transport) for DCC communication
- Understand the [Architecture](/guide/architecture) of the 14-crate Rust workspace
- Learn [Skill Scopes & Policies](/guide/skill-scopes-policies) for trust-based skill management

## Building a DCC Adapter with DccServerBase

`DccServerBase` is the recommended base class for building DCC adapters. It bundles all the boilerplate that every adapter needs:

```python
from pathlib import Path
from dcc_mcp_core import DccServerBase

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

# That's it — skill management, hot-reload, gateway election are all inherited.
server = BlenderMcpServer(gateway_port=9765)
server.register_builtin_actions()  # discover and load skills
server.enable_hot_reload()         # optional: auto-reload on file changes
handle = server.start()            # returns McpServerHandle
print(f"Running at {handle.mcp_url()}")
```

For zero-boilerplate adapters, use `make_start_stop`:

```python
from dcc_mcp_core import make_start_stop

start_server, stop_server = make_start_stop(
    BlenderMcpServer,
    hot_reload_env_var="DCC_MCP_BLENDER_HOT_RELOAD",
)
```
